#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::fs;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use actix_web::{App, web::Data};
use futures_util::future::join_all;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use sha2::Digest;
use sonde::{
    api, auth,
    config::{InstallationConfig, MasterKey, PasswordPepper, RuntimeConfig},
    database::{self, applications as application_store, auth as auth_store, auth_state},
    state::AppState,
};

const EVENT_PATH: &str = "/api/v1/ingest/events";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenBody {
    token: String,
    signing_key: String,
}

#[derive(Debug, Deserialize)]
struct BatchReceiptBody {
    accepted: usize,
}

#[derive(Debug, Clone, Default)]
struct LatencyStats {
    count: usize,
    total_duration: Duration,
    min_us: u64,
    max_us: u64,
    mean_us: f64,
    std_dev_us: f64,
    p50_us: u64,
    p75_us: u64,
    p90_us: u64,
    p95_us: u64,
    p99_us: u64,
    p999_us: u64,
    rps: f64,
    items_per_sec: f64,
}

impl LatencyStats {
    fn calculate(mut samples_us: Vec<u64>, elapsed: Duration, total_items: usize) -> Self {
        if samples_us.is_empty() {
            return Self::default();
        }
        samples_us.sort_unstable();
        let count = samples_us.len();
        let total_us: u64 = samples_us.iter().sum();
        let mean_us = total_us as f64 / count as f64;

        let variance = samples_us
            .iter()
            .map(|&s| {
                let diff = s as f64 - mean_us;
                diff * diff
            })
            .sum::<f64>()
            / count as f64;
        let std_dev_us = variance.sqrt();

        let percentile = |pct: f64| -> u64 {
            let idx = ((count as f64 * pct / 100.0).round() as usize).saturating_sub(1);
            samples_us[idx.min(count - 1)]
        };

        let elapsed_secs = elapsed.as_secs_f64().max(0.0001);
        let rps = count as f64 / elapsed_secs;
        let items_per_sec = total_items as f64 / elapsed_secs;

        Self {
            count,
            total_duration: elapsed,
            min_us: samples_us[0],
            max_us: samples_us[count - 1],
            mean_us,
            std_dev_us,
            p50_us: percentile(50.0),
            p75_us: percentile(75.0),
            p90_us: percentile(90.0),
            p95_us: percentile(95.0),
            p99_us: percentile(99.0),
            p999_us: percentile(99.9),
            rps,
            items_per_sec,
        }
    }

    fn print_summary(&self, title: &str) {
        println!("============================================================");
        println!("  {title}");
        println!("============================================================");
        println!("  Total Requests:       {}", self.count);
        println!(
            "  Elapsed Time:         {:.3} s",
            self.total_duration.as_secs_f64()
        );
        println!("  Throughput (RPS):     {:.2} req/s", self.rps);
        if self.items_per_sec > 0.0 {
            println!("  Throughput (Items):   {:.2} items/s", self.items_per_sec);
        }
        println!(
            "  Latency Min:          {:.3} ms",
            self.min_us as f64 / 1000.0
        );
        println!(
            "  Latency P50 (Median): {:.3} ms",
            self.p50_us as f64 / 1000.0
        );
        println!(
            "  Latency P75:          {:.3} ms",
            self.p75_us as f64 / 1000.0
        );
        println!(
            "  Latency P90:          {:.3} ms",
            self.p90_us as f64 / 1000.0
        );
        println!(
            "  Latency P95:          {:.3} ms",
            self.p95_us as f64 / 1000.0
        );
        println!(
            "  Latency P99:          {:.3} ms",
            self.p99_us as f64 / 1000.0
        );
        println!(
            "  Latency P99.9:        {:.3} ms",
            self.p999_us as f64 / 1000.0
        );
        println!(
            "  Latency Max:          {:.3} ms",
            self.max_us as f64 / 1000.0
        );
        println!(
            "  Latency Mean:         {:.3} ms (+/- {:.3} ms)",
            self.mean_us / 1000.0,
            self.std_dev_us / 1000.0
        );
        println!("------------------------------------------------------------");
    }
}

/// Query real process WorkingSet64 and PrivateMemorySize64 in MiB on Windows.
fn get_process_memory_mib() -> (f64, f64) {
    let pid = std::process::id();
    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "$p = Get-Process -Id {pid}; \"$($p.WorkingSet64) $($p.PrivateMemorySize64)\""
                ),
            ])
            .output();
        if let Ok(out) = output
            && out.status.success()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() == 2 {
                let ws: f64 = parts[0].parse().unwrap_or(0.0) / 1_048_576.0;
                let pm: f64 = parts[1].parse().unwrap_or(0.0) / 1_048_576.0;
                return (ws, pm);
            }
        }
    }
    (0.0, 0.0)
}

fn get_dir_size_bytes(path: &std::path::Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata()
                && meta.is_file()
            {
                total += meta.len();
            }
        }
    }
    total
}

struct StressFixture {
    pub _temp_dir: tempfile::TempDir,
    pub state: Arc<AppState>,
    pub owner_token: String,
    pub app_id: String,
    pub raw_key: String,
}

async fn create_stress_fixture() -> StressFixture {
    let temp_dir = tempfile::tempdir().unwrap();
    let data_dir = temp_dir.path().to_path_buf();
    let config_path = data_dir.join("sonde.json");
    let db_path = data_dir.join("sonde.sqlite");
    let db_url = format!(
        "sqlite://{}?mode=rwc",
        db_path.to_string_lossy().replace('\\', "/")
    );

    let install_config = InstallationConfig {
        database_url: db_url.clone(),
        locale: "en".into(),
        timezone: "UTC".into(),
        secure_cookie: false,
    };
    std::fs::write(
        &config_path,
        serde_json::to_string(&install_config).unwrap(),
    )
    .unwrap();

    let log_dir = data_dir.join("logs");
    let runtime = RuntimeConfig {
        bind: "127.0.0.1:8080".into(),
        domain: None,
        allowed_hosts: vec!["localhost".into(), "127.0.0.1".into(), "::1".into()],
        data_dir,
        config_path,
        log_dir,
        log_retention_days: 14,
        audit_log_retention_days: 180,
        database_url_override: None,
        password_pepper: PasswordPepper::new(common::TEST_PEPPER),
        trusted_proxies: vec!["127.0.0.1".parse::<IpAddr>().unwrap()],
        allow_insecure_cookies: true,
        master_key: MasterKey::from_bytes([9_u8; 32]),
    };

    let database = database::connect(&db_url).await.unwrap();
    database::migrate(&database).await.unwrap();
    let password_hash = auth::hash_password(common::TEST_PASSWORD, &common::TEST_PEPPER).unwrap();
    auth_store::create_super_admin(
        &database,
        "admin@example.com",
        "admin",
        &password_hash,
        "en",
    )
    .await
    .unwrap();
    let owner = auth_store::user_by_email(&database, "admin@example.com")
        .await
        .unwrap()
        .unwrap();
    let (app_id, env_id) = application_store::create_application(
        &database,
        "Stress App",
        "stress-app",
        Some(&owner.id),
    )
    .await
    .unwrap();
    let key_hash = hex::encode(sha2::Sha256::digest(common::TEST_INGEST_KEY.as_bytes()));
    application_store::create_api_key(
        &database,
        &app_id,
        &env_id,
        "Stress Key",
        &key_hash,
        "sonde_stress",
        &["ingest".to_string()],
    )
    .await
    .unwrap();
    let (owner_token, _) = auth_state::create_session(&database, &owner.id)
        .await
        .unwrap();

    StressFixture {
        _temp_dir: temp_dir,
        state: Arc::new(AppState::load(runtime).await.unwrap()),
        owner_token,
        app_id,
        raw_key: common::TEST_INGEST_KEY.to_owned(),
    }
}

async fn start_test_server(state: Arc<AppState>) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let addr = listener.local_addr().expect("listener address");
    let server = actix_web::HttpServer::new(move || {
        App::new()
            .app_data(Data::new(state.clone()))
            .configure(api::configure)
    })
    .listen(listener)
    .expect("listen")
    .workers(4)
    .max_connections(25_000)
    .client_request_timeout(Duration::from_secs(15))
    .disable_signals()
    .run();
    tokio::spawn(server);
    format!("http://{addr}")
}

async fn wait_ready(client: &Client, base: &str) {
    for _ in 0..50 {
        if let Ok(response) = client.get(format!("{base}/health/live")).send().await
            && response.status() == StatusCode::OK
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("server did not become ready");
}

async fn issue_token_with_ip(
    client: &Client,
    base: &str,
    raw_key: &str,
    device_id: &str,
    simulated_ip: &str,
) -> TokenBody {
    let response = client
        .post(format!("{base}/api/v1/ingest/token"))
        .header("user-agent", common::TEST_USER_AGENT)
        .header("authorization", format!("Bearer {raw_key}"))
        .header("x-forwarded-for", simulated_ip)
        .json(&serde_json::json!({ "deviceId": device_id }))
        .send()
        .await
        .expect("token request");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "token issue must succeed"
    );
    response.json().await.expect("token json")
}

fn signed_event_headers(
    token: &str,
    signing_key: &str,
    timestamp: i64,
    nonce: &str,
    body: &[u8],
    simulated_ip: &str,
) -> reqwest::header::HeaderMap {
    let signature = common::hmac_hex(signing_key, timestamp, nonce, "POST", EVENT_PATH, body);
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("user-agent", common::TEST_USER_AGENT.parse().unwrap());
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    headers.insert("x-sonde-timestamp", timestamp.to_string().parse().unwrap());
    headers.insert("x-sonde-nonce", nonce.parse().unwrap());
    headers.insert("x-sonde-signature", signature.parse().unwrap());
    headers.insert("x-forwarded-for", simulated_ip.parse().unwrap());
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        "application/json".parse().unwrap(),
    );
    headers
}

// ---------------------------------------------------------------------------
// TEST 1: 并发连接数阶梯压力与长连接维持能力测试 (Keep-Alive Connection Scaling)
// ---------------------------------------------------------------------------
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_connection_scaling_and_concurrency() {
    let fixture = create_stress_fixture().await;
    let base = start_test_server(fixture.state.clone()).await;
    let bootstrap_client = Client::new();
    wait_ready(&bootstrap_client, &base).await;

    let (base_ws, base_pm) = get_process_memory_mib();
    println!("\n=== TEST 1: Connection Scaling & Concurrency Test ===");
    println!(
        "Baseline Process Memory: WorkingSet = {base_ws:.2} MiB, PrivateBytes = {base_pm:.2} MiB"
    );

    let concurrency_levels = [10, 50, 100, 200];
    let requests_per_client = 20;

    for &concurrency in &concurrency_levels {
        let clients: Vec<Client> = (0..concurrency)
            .map(|_| {
                Client::builder()
                    .tcp_keepalive(Some(Duration::from_secs(60)))
                    .pool_max_idle_per_host(10)
                    .build()
                    .unwrap()
            })
            .collect();

        let start = Instant::now();
        let tasks = clients.into_iter().enumerate().map(|(client_idx, client)| {
            let base = base.clone();
            async move {
                let mut latencies = Vec::with_capacity(requests_per_client);
                for req_idx in 0..requests_per_client {
                    let url = if (client_idx + req_idx) % 2 == 0 {
                        format!("{base}/health/live")
                    } else {
                        format!("{base}/health/ready")
                    };
                    let t0 = Instant::now();
                    let res = client.get(&url).send().await.expect("connection request");
                    assert_eq!(res.status(), StatusCode::OK);
                    latencies.push(t0.elapsed().as_micros() as u64);
                }
                latencies
            }
        });

        let results: Vec<Vec<u64>> = join_all(tasks).await;
        let elapsed = start.elapsed();
        let all_latencies: Vec<u64> = results.into_iter().flatten().collect();
        let total_requests = all_latencies.len();

        let stats = LatencyStats::calculate(all_latencies, elapsed, total_requests);
        let (ws, _pm) = get_process_memory_mib();

        println!(
            "Concurrency {:>3} clients | Total {:>4} reqs | Elapsed {:>6.2} ms | RPS: {:>7.1} | P50: {:>5.2} ms | P99: {:>5.2} ms | WS: {:>5.1} MiB",
            concurrency,
            total_requests,
            elapsed.as_secs_f64() * 1000.0,
            stats.rps,
            stats.p50_us as f64 / 1000.0,
            stats.p99_us as f64 / 1000.0,
            ws
        );
    }
}

// ---------------------------------------------------------------------------
// TEST 2: 端到端高吞吐遥测入库全指标压测 (HMAC + Nonce + Batch + SQLite WAL)
// ---------------------------------------------------------------------------
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_e2e_ingest_pipeline_throughput_and_latencies() {
    let fixture = create_stress_fixture().await;
    let base = start_test_server(fixture.state.clone()).await;
    let client = Client::builder()
        .pool_max_idle_per_host(100)
        .build()
        .unwrap();
    wait_ready(&client, &base).await;

    let db_dir = fixture._temp_dir.path();
    let initial_disk_bytes = get_dir_size_bytes(db_dir);
    let (initial_ws, initial_pm) = get_process_memory_mib();

    println!("\n=== TEST 2: High-Throughput Telemetry Ingest Benchmark ===");
    println!(
        "Initial Storage Size: {:.2} KiB",
        initial_disk_bytes as f64 / 1024.0
    );
    println!(
        "Initial Process Memory: WorkingSet = {initial_ws:.2} MiB, PrivateBytes = {initial_pm:.2} MiB"
    );

    // 预热并创建 50 个模拟设备的令牌（分散在不同模拟 IP 下）
    const SIMULATED_DEVICES: usize = 50;
    let mut tokens = Vec::with_capacity(SIMULATED_DEVICES);
    for i in 0..SIMULATED_DEVICES {
        let device_id = format!("device-stress-{i:03}");
        let simulated_ip = format!("10.1.{}.{}", (i / 250) + 1, (i % 250) + 1);
        let issued =
            issue_token_with_ip(&client, &base, &fixture.raw_key, &device_id, &simulated_ip).await;
        tokens.push((issued, simulated_ip));
    }

    // 构造每批 10 个具有代表性的复杂遥测事件（含属性、渠道、版本、操作系统、用户ID）
    let batch_items: Vec<serde_json::Value> = (0..10)
        .map(|i| {
            serde_json::json!({
                "name": format!("event_checkout_step_{i}"),
                "attributes": {
                    "channel": "production",
                    "build": "2026.09.28.release",
                    "os": "windows-x86_64",
                    "step": i,
                    "session_id": format!("sess-stress-{}", i % 5)
                }
            })
        })
        .collect();
    let body_bytes = serde_json::to_vec(&serde_json::json!({ "items": batch_items })).unwrap();
    let items_per_batch = 10;

    // 运行 1,000 个批次入库，总计 10,000 条事件，在 25 个并发工作协程中调度
    const TOTAL_BATCHES: usize = 1000;
    const CONCURRENCY: usize = 25;
    let batches_per_worker = TOTAL_BATCHES / CONCURRENCY;

    let (peak_ws, peak_pm) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));

    let start = Instant::now();
    let worker_tasks = (0..CONCURRENCY).map(|worker_idx| {
        let client = client.clone();
        let base = base.clone();
        let body_bytes = body_bytes.clone();
        let tokens = tokens.clone();
        let peak_ws = peak_ws.clone();
        let peak_pm = peak_pm.clone();

        async move {
            let mut latencies = Vec::with_capacity(batches_per_worker);
            let mut accepted_items = 0;

            for b in 0..batches_per_worker {
                let token_idx = (worker_idx * batches_per_worker + b) % tokens.len();
                let (token_info, simulated_ip) = &tokens[token_idx];
                let timestamp = chrono::Utc::now().timestamp_millis();
                let nonce = format!("nonce-w{worker_idx}-b{b}-{}", uuid::Uuid::now_v7());

                let headers = signed_event_headers(
                    &token_info.token,
                    &token_info.signing_key,
                    timestamp,
                    &nonce,
                    &body_bytes,
                    simulated_ip,
                );

                let t0 = Instant::now();
                let response = client
                    .post(format!("{base}{EVENT_PATH}"))
                    .headers(headers)
                    .body(body_bytes.clone())
                    .send()
                    .await
                    .expect("ingest request failed");

                assert_eq!(response.status(), StatusCode::ACCEPTED);
                let receipt: BatchReceiptBody = response.json().await.expect("receipt json");
                accepted_items += receipt.accepted;
                latencies.push(t0.elapsed().as_micros() as u64);

                if b % 20 == 0 {
                    let (ws, pm) = get_process_memory_mib();
                    peak_ws.fetch_max((ws * 1000.0) as u64, Ordering::Relaxed);
                    peak_pm.fetch_max((pm * 1000.0) as u64, Ordering::Relaxed);
                }
            }

            (latencies, accepted_items)
        }
    });

    let results = join_all(worker_tasks).await;
    let elapsed = start.elapsed();

    let mut all_latencies = Vec::with_capacity(TOTAL_BATCHES);
    let mut total_accepted = 0;
    for (lats, count) in results {
        all_latencies.extend(lats);
        total_accepted += count;
    }

    assert_eq!(total_accepted, TOTAL_BATCHES * items_per_batch);

    let stats = LatencyStats::calculate(all_latencies, elapsed, total_accepted);
    stats.print_summary("E2E Ingest Pipeline Stress Results (10,000 Events)");

    tokio::time::sleep(Duration::from_millis(150)).await;
    let final_disk_bytes = get_dir_size_bytes(db_dir);
    let disk_delta_bytes = final_disk_bytes.saturating_sub(initial_disk_bytes);
    let bytes_per_event = disk_delta_bytes as f64 / total_accepted as f64;

    let (final_ws, final_pm) = get_process_memory_mib();
    let max_ws = (peak_ws.load(Ordering::Relaxed) as f64 / 1000.0).max(final_ws);
    let max_pm = (peak_pm.load(Ordering::Relaxed) as f64 / 1000.0).max(final_pm);

    println!("------------------------------------------------------------");
    println!("  Resource & Footprint Analysis");
    println!("------------------------------------------------------------");
    println!(
        "  Total Telemetry Ingested: {} items across {} batches",
        total_accepted, TOTAL_BATCHES
    );
    println!(
        "  Initial Storage Size:     {:.2} KiB",
        initial_disk_bytes as f64 / 1024.0
    );
    println!(
        "  Final Storage Size:       {:.2} KiB (Delta: +{:.2} KiB)",
        final_disk_bytes as f64 / 1024.0,
        disk_delta_bytes as f64 / 1024.0
    );
    println!(
        "  Storage Cost / Event:     {:.1} bytes/event",
        bytes_per_event
    );
    println!("  Memory Baseline (WS):     {:.2} MiB", initial_ws);
    println!(
        "  Memory Peak under Load:   {:.2} MiB (Delta: +{:.2} MiB)",
        max_ws,
        max_ws - initial_ws
    );
    println!(
        "  Memory Final Settled:     {:.2} MiB (Delta: +{:.2} MiB)",
        final_ws,
        final_ws - initial_ws
    );
    println!("  Private Bytes Peak:       {:.2} MiB", max_pm);
    println!("============================================================\n");
}

// ---------------------------------------------------------------------------
// TEST 3: 复杂分析与检索查询多指标压测 (Query-Heavy Analytics & Explorer)
// ---------------------------------------------------------------------------
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_query_heavy_analytics_and_explorer() {
    let fixture = create_stress_fixture().await;
    let base = start_test_server(fixture.state.clone()).await;
    let client = Client::new();
    wait_ready(&client, &base).await;

    // 先插入测试数据
    let issued = issue_token_with_ip(
        &client,
        &base,
        &fixture.raw_key,
        "device-query-seed",
        "10.0.1.1",
    )
    .await;
    let items = serde_json::json!({
        "items": (0..100).map(|i| {
            serde_json::json!({
                "name": if i % 2 == 0 { "app_launch" } else { "page_view" },
                "attributes": { "version": "1.0.0", "env": "prod" }
            })
        }).collect::<Vec<_>>()
    });
    let body_bytes = serde_json::to_vec(&items).unwrap();
    let headers = signed_event_headers(
        &issued.token,
        &issued.signing_key,
        chrono::Utc::now().timestamp_millis(),
        "seed-nonce-01",
        &body_bytes,
        "10.0.1.1",
    );
    client
        .post(format!("{base}{EVENT_PATH}"))
        .headers(headers)
        .body(body_bytes)
        .send()
        .await
        .unwrap();

    let cookie = format!("sonde-session={}", fixture.owner_token);
    let app_id = fixture.app_id.clone();

    println!("\n=== TEST 3: Query-Heavy Analytics & Explorer Benchmark ===");
    let (ws_before, _) = get_process_memory_mib();

    let query_urls = [
        format!("{base}/api/v1/admin/explorer/events?applicationId={app_id}&page=1&pageSize=50"),
        format!("{base}/api/v1/admin/overview?days=30"),
        format!("{base}/api/v1/admin/applications"),
        format!("{base}/api/v1/admin/errors/groups?applicationId={app_id}&page=1&pageSize=20"),
    ];

    let start = Instant::now();
    const CONCURRENT_QUERY_CLIENTS: usize = 4;
    const QUERIES_PER_CLIENT: usize = 50;
    let total_queries = CONCURRENT_QUERY_CLIENTS * QUERIES_PER_CLIENT;

    let tasks = (0..CONCURRENT_QUERY_CLIENTS).map(|client_idx| {
        let client = client.clone();
        let cookie = cookie.clone();
        let query_urls = query_urls.clone();

        async move {
            let mut latencies = Vec::with_capacity(QUERIES_PER_CLIENT);
            for q_idx in 0..QUERIES_PER_CLIENT {
                let url = &query_urls[(client_idx + q_idx) % query_urls.len()];
                let t0 = Instant::now();
                let res = client
                    .get(url)
                    .header("cookie", &cookie)
                    .send()
                    .await
                    .expect("query request");
                assert_eq!(res.status(), StatusCode::OK);
                latencies.push(t0.elapsed().as_micros() as u64);
            }
            latencies
        }
    });

    let results = join_all(tasks).await;
    let elapsed = start.elapsed();
    let all_latencies: Vec<u64> = results.into_iter().flatten().collect();

    let stats = LatencyStats::calculate(all_latencies, elapsed, total_queries);
    let (ws_after, _) = get_process_memory_mib();

    stats.print_summary("Analytics & Explorer Query Benchmark (400 Complex Queries)");
    println!(
        "  Query Memory Footprint: Before = {:.2} MiB, After = {:.2} MiB",
        ws_before, ws_after
    );
}

// ---------------------------------------------------------------------------
// TEST 4: 混合负载与连续压力测试 (Mixed Real-World Workload & Stability)
// ---------------------------------------------------------------------------
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_mixed_real_world_workload_and_stability() {
    let fixture = create_stress_fixture().await;
    let base = start_test_server(fixture.state.clone()).await;
    let client = Client::builder()
        .pool_max_idle_per_host(30)
        .build()
        .unwrap();
    wait_ready(&client, &base).await;

    let cookie = format!("sonde-session={}", fixture.owner_token);
    let app_id = fixture.app_id.clone();

    println!("\n=== TEST 4: Mixed Real-World Workload Test ===");
    let (initial_ws, _) = get_process_memory_mib();

    // 运行 3 个连续压力周期，检测循环负载下内存是否能稳定平复（无内存泄漏）
    for cycle in 1..=3 {
        let cycle_start = Instant::now();
        let mut sample_latencies = Vec::new();

        let device_id = format!("device-mixed-cycle-{cycle}");
        let simulated_ip = format!("10.2.{cycle}.1");
        let issued =
            issue_token_with_ip(&client, &base, &fixture.raw_key, &device_id, &simulated_ip).await;

        // 每个周期执行：70% 遥测入库 + 20% 查询 + 10% 会话/探针
        for i in 0..40 {
            if i % 10 < 7 {
                // Ingest (70%)
                let body = serde_json::to_vec(&serde_json::json!({
                    "items": [{ "name": "mixed_event", "attributes": { "cycle": cycle, "i": i } }]
                }))
                .unwrap();
                let headers = signed_event_headers(
                    &issued.token,
                    &issued.signing_key,
                    chrono::Utc::now().timestamp_millis(),
                    &format!("nonce-c{cycle}-i{i}-{}", uuid::Uuid::now_v7()),
                    &body,
                    &simulated_ip,
                );
                let t0 = Instant::now();
                let res = client
                    .post(format!("{base}{EVENT_PATH}"))
                    .headers(headers)
                    .body(body)
                    .send()
                    .await
                    .unwrap();
                assert_eq!(res.status(), StatusCode::ACCEPTED);
                sample_latencies.push(t0.elapsed().as_micros() as u64);
            } else if i % 10 < 9 {
                // Query (20%)
                let t0 = Instant::now();
                let res = client
                    .get(format!("{base}/api/v1/admin/explorer/events?applicationId={app_id}&page=1&pageSize=10"))
                    .header("cookie", &cookie)
                    .send()
                    .await
                    .unwrap();
                assert_eq!(res.status(), StatusCode::OK);
                sample_latencies.push(t0.elapsed().as_micros() as u64);
            } else {
                // Health/Auth (10%)
                let t0 = Instant::now();
                let res = client
                    .get(format!("{base}/health/live"))
                    .send()
                    .await
                    .unwrap();
                assert_eq!(res.status(), StatusCode::OK);
                sample_latencies.push(t0.elapsed().as_micros() as u64);
            }
        }

        let (ws, pm) = get_process_memory_mib();
        let stats = LatencyStats::calculate(sample_latencies, cycle_start.elapsed(), 40);
        println!(
            "Cycle {}/3 | 40 Mixed Ops | Elapsed: {:>6.2} ms | RPS: {:>6.1} | P50: {:>5.2} ms | WorkingSet: {:>5.1} MiB | PrivateBytes: {:>5.1} MiB",
            cycle,
            cycle_start.elapsed().as_secs_f64() * 1000.0,
            stats.rps,
            stats.p50_us as f64 / 1000.0,
            ws,
            pm
        );
    }

    let (final_ws, _) = get_process_memory_mib();
    let net_change = final_ws - initial_ws;
    println!(
        "Memory Stability: Initial = {initial_ws:.2} MiB, Final = {final_ws:.2} MiB (Net Change: {net_change:+.2} MiB)"
    );
    println!("============================================================\n");
}

// ---------------------------------------------------------------------------
// TEST 5: 限流与并发背压阈值准确性测试 (Rate Limiting & Backpressure Test)
// ---------------------------------------------------------------------------
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_rate_limiting_and_backpressure_thresholds() {
    let fixture = create_stress_fixture().await;
    let base = start_test_server(fixture.state.clone()).await;
    let client = Client::new();
    wait_ready(&client, &base).await;

    println!("\n=== TEST 5: Rate Limiting & Backpressure Thresholds Test ===");

    // 1. 验证设备级 60 req/min 限流
    let single_ip = "192.168.100.50";
    let single_device = "device-rate-limit-check";
    let issued =
        issue_token_with_ip(&client, &base, &fixture.raw_key, single_device, single_ip).await;

    let body = br#"{"items":[{"name":"rate_test"}]}"#;
    let mut accepted_count = 0;
    let mut rate_limited_count = 0;

    for i in 0..70 {
        let timestamp = chrono::Utc::now().timestamp_millis();
        let nonce = format!("nonce-limit-{i}-{}", uuid::Uuid::now_v7());
        let headers = signed_event_headers(
            &issued.token,
            &issued.signing_key,
            timestamp,
            &nonce,
            body,
            single_ip,
        );
        let res = client
            .post(format!("{base}{EVENT_PATH}"))
            .headers(headers)
            .body(body.to_vec())
            .send()
            .await
            .unwrap();

        if res.status() == StatusCode::ACCEPTED {
            accepted_count += 1;
        } else if res.status() == StatusCode::TOO_MANY_REQUESTS {
            rate_limited_count += 1;
        }
    }

    println!(
        "  Device Ingest Rate Limit: {} accepted, {} rate-limited (429)",
        accepted_count, rate_limited_count
    );
    assert_eq!(
        accepted_count, 60,
        "Device limit must allow exactly 60 requests per minute"
    );
    assert_eq!(
        rate_limited_count, 10,
        "Subsequent 10 requests must be rejected with 429"
    );
    println!("  -> PASS: Device rate limiting exactly throttles at 60 req/min");

    // 2. 验证分析查询信号量并发背压保护 (Permit budget = 8)
    let cookie = format!("sonde-session={}", fixture.owner_token);
    let app_id = fixture.app_id.clone();
    let query_url =
        format!("{base}/api/v1/admin/explorer/events?applicationId={app_id}&page=1&pageSize=50");
    let parallel_tasks = (0..20).map(|_| {
        let client = client.clone();
        let cookie = cookie.clone();
        let query_url = query_url.clone();
        async move {
            client
                .get(&query_url)
                .header("cookie", cookie)
                .send()
                .await
                .unwrap()
                .status()
        }
    });
    let statuses = join_all(parallel_tasks).await;
    let ok_count = statuses.iter().filter(|&&s| s == StatusCode::OK).count();
    let throttled_count = statuses
        .iter()
        .filter(|&&s| s == StatusCode::TOO_MANY_REQUESTS)
        .count();
    println!(
        "  Analytics Semaphore Backpressure: {} OK, {} Throttled (429)",
        ok_count, throttled_count
    );
    assert!(
        throttled_count > 0,
        "Excess simultaneous queries must receive 429 backpressure"
    );
    println!(
        "  -> PASS: Analytics semaphore backpressure safely protects database from starvation"
    );
    println!("============================================================\n");
}
