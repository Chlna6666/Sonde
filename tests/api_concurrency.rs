#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::sync::Arc;

use actix_web::{App, web::Data};
use futures_util::future::join_all;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use sonde::{api, state::AppState};

const EVENT_PATH: &str = "/api/v1/ingest/events";
const EVENT_BODY: &[u8] = br#"{"items":[{"name":"app_startup"}]}"#;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenBody {
    token: String,
    signing_key: String,
}

#[derive(Debug, Deserialize)]
struct BatchReceiptBody {
    accepted: usize,
}

async fn start_server(state: Arc<AppState>) -> String {
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
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("server did not become ready");
}

async fn issue_token(client: &Client, base: &str, raw_key: &str, device_id: &str) -> TokenBody {
    let response = client
        .post(format!("{base}/api/v1/ingest/token"))
        .header("user-agent", common::TEST_USER_AGENT)
        .header("authorization", format!("Bearer {raw_key}"))
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
) -> reqwest::header::HeaderMap {
    let signature = common::hmac_hex(
        signing_key,
        timestamp,
        nonce,
        "POST",
        EVENT_PATH,
        EVENT_BODY,
    );
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("user-agent", common::TEST_USER_AGENT.parse().unwrap());
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    headers.insert("x-sonde-timestamp", timestamp.to_string().parse().unwrap());
    headers.insert("x-sonde-nonce", nonce.parse().unwrap());
    headers.insert("x-sonde-signature", signature.parse().unwrap());
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        "application/json".parse().unwrap(),
    );
    headers
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_health_and_setup_status_succeed() {
    let fixture = common::installed_http().await;
    let base = start_server(fixture.state.clone()).await;
    let client = Client::new();
    wait_ready(&client, &base).await;

    let responses = join_all((0..32).map(|index| {
        let client = client.clone();
        let url = if index % 2 == 0 {
            format!("{base}/health/live")
        } else {
            format!("{base}/health/ready")
        };
        async move { client.get(url).send().await.expect("health") }
    }))
    .await;
    for response in responses {
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), "ok");
    }

    let setup = join_all((0..16).map(|_| {
        let client = client.clone();
        let url = format!("{base}/api/v1/setup/status");
        async move { client.get(url).send().await.expect("setup") }
    }))
    .await;
    for response in setup {
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["installed"], true);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_session_and_admin_reads_succeed() {
    let fixture = common::installed_http().await;
    let base = start_server(fixture.state.clone()).await;
    let client = Client::new();
    wait_ready(&client, &base).await;
    let cookie = format!("sonde-session={}", fixture.owner_token);
    let app_id = fixture.app_id.clone();

    let me = join_all((0..8).map(|_| {
        let client = client.clone();
        let cookie = cookie.clone();
        let url = format!("{base}/api/v1/auth/me");
        async move {
            client
                .get(url)
                .header("cookie", cookie)
                .send()
                .await
                .expect("me")
        }
    }));
    let apps = join_all((0..8).map(|_| {
        let client = client.clone();
        let cookie = cookie.clone();
        let url = format!("{base}/api/v1/admin/applications");
        async move {
            client
                .get(url)
                .header("cookie", cookie)
                .send()
                .await
                .expect("apps")
        }
    }));
    let explorer = join_all((0..8).map(|_| {
        let client = client.clone();
        let cookie = cookie.clone();
        let url = format!(
            "{base}/api/v1/admin/explorer/events?applicationId={app_id}&page=1&pageSize=20"
        );
        async move {
            client
                .get(url)
                .header("cookie", cookie)
                .send()
                .await
                .expect("explorer")
        }
    }));
    let (me, apps, explorer) = tokio::join!(me, apps, explorer);
    for response in me.into_iter().chain(apps).chain(explorer) {
        assert_eq!(response.status(), StatusCode::OK);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_token_issue_for_distinct_devices_succeeds() {
    let fixture = common::installed_http().await;
    let base = start_server(fixture.state.clone()).await;
    let client = Client::new();
    wait_ready(&client, &base).await;
    let raw_key = fixture.raw_key.clone();

    let responses = join_all((0..8).map(|index| {
        let client = client.clone();
        let raw_key = raw_key.clone();
        let url = format!("{base}/api/v1/ingest/token");
        let device_id = format!("device-concurrent-{index:02}");
        async move {
            client
                .post(url)
                .header("user-agent", common::TEST_USER_AGENT)
                .header("authorization", format!("Bearer {raw_key}"))
                .json(&serde_json::json!({ "deviceId": device_id }))
                .send()
                .await
                .expect("token")
        }
    }))
    .await;
    for response in responses {
        assert_eq!(response.status(), StatusCode::OK);
        let body: TokenBody = response.json().await.unwrap();
        assert!(body.token.starts_with("sndt_"));
        assert!(!body.signing_key.is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_ingest_events_accept_distinct_nonces() {
    let fixture = common::installed_http().await;
    let base = start_server(fixture.state.clone()).await;
    let client = Client::new();
    wait_ready(&client, &base).await;
    let issued = issue_token(&client, &base, &fixture.raw_key, "device-events-01").await;
    let timestamp = chrono::Utc::now().timestamp_millis();

    let responses = join_all((0..8).map(|index| {
        let client = client.clone();
        let url = format!("{base}{EVENT_PATH}");
        let headers = signed_event_headers(
            &issued.token,
            &issued.signing_key,
            timestamp,
            &format!("nonce-ok-{index:02}-xxxx"),
        );
        async move {
            client
                .post(url)
                .headers(headers)
                .body(EVENT_BODY)
                .send()
                .await
                .expect("ingest")
        }
    }))
    .await;

    let mut accepted = 0;
    for response in responses {
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let receipt: BatchReceiptBody = response.json().await.unwrap();
        accepted += receipt.accepted;
    }
    assert_eq!(accepted, 8);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_ingest_same_nonce_is_accepted_once() {
    let fixture = common::installed_http().await;
    let base = start_server(fixture.state.clone()).await;
    let client = Client::new();
    wait_ready(&client, &base).await;
    let issued = issue_token(&client, &base, &fixture.raw_key, "device-replay-01").await;
    let timestamp = chrono::Utc::now().timestamp_millis();
    let nonce = "nonce-shared-replay-01";

    let responses = join_all((0..16).map(|_| {
        let client = client.clone();
        let url = format!("{base}{EVENT_PATH}");
        let headers = signed_event_headers(&issued.token, &issued.signing_key, timestamp, nonce);
        async move {
            client
                .post(url)
                .headers(headers)
                .body(EVENT_BODY)
                .send()
                .await
                .expect("replay ingest")
        }
    }))
    .await;

    let mut accepted = 0;
    let mut forbidden = 0;
    for response in responses {
        match response.status() {
            StatusCode::ACCEPTED => {
                let receipt: BatchReceiptBody = response.json().await.unwrap();
                accepted += receipt.accepted;
            }
            StatusCode::FORBIDDEN => forbidden += 1,
            other => panic!("unexpected ingest status {other}"),
        }
    }
    assert_eq!(accepted, 1, "exactly one replay window must commit");
    assert_eq!(forbidden, 15, "the remaining replicas must be rejected");
}
