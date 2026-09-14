#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use sonde::{
    database::{device_identity, query},
    domain::telemetry::{
        AttributeMap, Attributes, Batch, EventInput, HistogramInput, MetricInput, MetricType,
        ValidateTelemetry,
    },
    ingest_signature,
};

fn sign(key: &str, timestamp: i64, nonce: &str, method: &str, path: &str, body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body);
    let body_hash = hasher.finalize();
    let mut body_hash_hex = [0_u8; 64];
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (index, byte) in body_hash.iter().enumerate() {
        body_hash_hex[index * 2] = HEX[usize::from(byte >> 4)];
        body_hash_hex[index * 2 + 1] = HEX[usize::from(byte & 0x0f)];
    }

    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).expect("hmac key");
    mac.update(b"sonde-hmac-sha256-v2\n");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b"\n");
    mac.update(nonce.as_bytes());
    mac.update(b"\n");
    mac.update(method.as_bytes());
    mac.update(b"\n");
    mac.update(path.as_bytes());
    mac.update(b"\n");
    mac.update(&body_hash_hex);
    hex::encode(mac.finalize().into_bytes())
}

fn ingest_signature(criterion: &mut Criterion) {
    let key = "sec_benchmark_signing_key";
    let timestamp = chrono::Utc::now().timestamp_millis();
    let nonce = "nonce-benchmark-01";
    let path = "/api/v1/ingest/events";
    let bodies: [(&str, &[u8]); 3] = [
        (
            "small",
            br#"{"items":[{"name":"app_startup","attributes":{"channel":"stable"}}]}"#,
        ),
        ("1kib", &BODY_1KIB),
        ("64kib", &BODY_64KIB),
    ];

    let mut group = criterion.benchmark_group("ingest_signature");
    for (label, body) in bodies {
        group.throughput(Throughput::Bytes(body.len() as u64));
        let signature = sign(key, timestamp, nonce, "POST", path, body);
        group.bench_function(BenchmarkId::new("verify", label), |bencher| {
            bencher.iter(|| {
                ingest_signature::verify_at(
                    ingest_signature::VerifyRequest {
                        signing_key: key,
                        timestamp_ms: timestamp,
                        nonce,
                        method: "POST",
                        path,
                        body: black_box(body),
                        signature_hex: &signature,
                    },
                    timestamp,
                )
                .expect("signature must verify")
            });
        });
    }
    group.finish();
}

fn scoped_identity(criterion: &mut Criterion) {
    criterion.bench_function("device_identity_scoped_hash", |bencher| {
        bencher.iter(|| {
            device_identity::scoped_hash_parts(
                black_box("app-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
                black_box("production"),
                black_box("device-stable-identifier-01"),
            )
        });
    });
}

fn telemetry_validation(criterion: &mut Criterion) {
    let event = EventInput {
        name: "app_startup".into(),
        timestamp: None,
        anonymous_id: None,
        session_id: None,
        app_version: Some("1.2.3".into()),
        launcher_version: None,
        os: Some("windows".into()),
        system_language: Some("zh-CN".into()),
        architecture: Some("x86_64".into()),
        idempotency_key: Some("req-1".into()),
        attributes: Default::default(),
    };
    let histogram = MetricInput {
        name: "http.request.duration".into(),
        metric_type: MetricType::Histogram,
        value: None,
        histogram: Some(HistogramInput {
            count: 4,
            sum: Some(40.0),
            min: Some(2.0),
            max: Some(20.0),
            explicit_bounds: vec![5.0, 10.0],
            bucket_counts: vec![1, 2, 1],
        }),
        unit: Some("ms".into()),
        timestamp: None,
        attributes: Default::default(),
    };

    let mut group = criterion.benchmark_group("telemetry_validate");
    group.bench_function("event", |bencher| {
        bencher.iter(|| event.validate().expect("event valid"));
    });
    group.bench_function("histogram", |bencher| {
        bencher.iter(|| histogram.validate().expect("histogram valid"));
    });
    group.bench_function(BenchmarkId::new("histogram_normalize", 1), |bencher| {
        bencher.iter(|| histogram.normalized_histogram().expect("histogram present"));
    });
    group.finish();
}

fn event_batch_json(count: usize, with_attributes: bool) -> Vec<u8> {
    let batch = Batch {
        items: (0..count)
            .map(|index| EventInput {
                name: "app_startup".into(),
                timestamp: None,
                anonymous_id: None,
                session_id: None,
                app_version: Some("1.2.3".into()),
                launcher_version: None,
                os: Some("windows".into()),
                system_language: Some("zh-CN".into()),
                architecture: Some("x86_64".into()),
                idempotency_key: Some(format!("req-{index}")),
                attributes: if with_attributes {
                    Attributes::from_map(AttributeMap::from_iter([
                        ("channel".into(), serde_json::Value::from("stable")),
                        ("build".into(), serde_json::Value::from(index as i64)),
                    ]))
                    .expect("attribute json")
                } else {
                    Attributes::new()
                },
            })
            .collect(),
    };
    serde_json::to_vec(&batch).expect("batch json")
}

fn ingest_json_parse(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("ingest_json_parse");
    for (label, with_attributes) in [("empty_attrs", false), ("two_attrs", true)] {
        for count in [1_usize, 100, 1_000] {
            let payload = event_batch_json(count, with_attributes);
            group.throughput(Throughput::Elements(count as u64));
            group.bench_function(BenchmarkId::new(label, count), |bencher| {
                bencher.iter(|| {
                    let parsed: Batch<EventInput> =
                        sonde::json::from_slice(black_box(&payload)).expect("parse");
                    black_box(parsed.items.len())
                });
            });
            group.bench_function(
                BenchmarkId::new(format!("{label}_validate"), count),
                |bencher| {
                    bencher.iter(|| {
                        let parsed: Batch<EventInput> =
                            sonde::json::from_slice(black_box(&payload)).expect("parse");
                        for item in &parsed.items {
                            item.validate().expect("event valid");
                        }
                        black_box(parsed.items.len())
                    });
                },
            );
        }
    }
    group.finish();
}

fn ingest_json_serialize(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("ingest_json_serialize");
    let receipt = sonde::domain::telemetry::BatchReceipt {
        accepted: 997,
        rejected: vec![
            sonde::domain::telemetry::RejectedItem {
                index: 3,
                reason: "name must be 1..128 bytes",
            },
            sonde::domain::telemetry::RejectedItem {
                index: 41,
                reason: "timestamp must not be more than 7 days in the past for live ingestion",
            },
        ],
    };
    group.bench_function("batch_receipt", |bencher| {
        bencher
            .iter(|| black_box(serde_json::to_string(black_box(&receipt)).expect("receipt json")));
    });

    let bounds: Vec<f64> = (0..32_u32).map(|index| f64::from(index) * 5.0).collect();
    let buckets: Vec<u64> = (0..33_u64).collect();
    group.bench_function("histogram_bounds", |bencher| {
        bencher.iter(|| {
            black_box(sonde::json::encode_f64_array(black_box(&bounds)).expect("bounds json"))
        });
    });
    group.bench_function("histogram_buckets", |bencher| {
        bencher.iter(|| {
            black_box(sonde::json::encode_u64_array(black_box(&buckets)).expect("buckets json"))
        });
    });

    let stored = String::from(r#"{"channel":"stable","build":12}"#);
    group.bench_function("raw_attributes_replay", |bencher| {
        bencher.iter(|| {
            let raw = serde_json::value::RawValue::from_string(black_box(stored.clone()))
                .expect("stored json");
            black_box(serde_json::to_string(&raw).expect("replay"))
        });
    });
    group.bench_function("value_attributes_replay", |bencher| {
        bencher.iter(|| {
            let value: serde_json::Value =
                serde_json::from_str(black_box(&stored)).expect("parse value");
            black_box(serde_json::to_string(&value).expect("replay"))
        });
    });
    group.finish();
}

fn query_helpers(criterion: &mut Criterion) {
    criterion.bench_function("contains_like_pattern", |bencher| {
        bencher.iter(|| query::contains_like_pattern(black_box(r"win%_\\dows")));
    });
}

criterion_group!(
    hot_path,
    ingest_signature,
    scoped_identity,
    telemetry_validation,
    ingest_json_parse,
    ingest_json_serialize,
    query_helpers
);
criterion_main!(hot_path);

static BODY_1KIB: [u8; 1024] = {
    let mut body = [b'a'; 1024];
    body[0] = b'{';
    body[1] = b'}';
    body
};
static BODY_64KIB: [u8; 65_536] = {
    let mut body = [b'a'; 65_536];
    body[0] = b'{';
    body[1] = b'}';
    body
};
