use actix_web::{HttpRequest, HttpResponse, http::header};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web/dist"]
struct WebAssets;

pub async fn serve(request: HttpRequest) -> HttpResponse {
    let raw_path = request.path();
    let path = raw_path.trim_start_matches('/');

    if crate::security::validate_safe_relative_path(raw_path).is_err()
        || crate::security::validate_safe_relative_path(path).is_err()
    {
        return HttpResponse::BadRequest().finish();
    }

    #[cfg(debug_assertions)]
    if let Ok(proxy_target) = std::env::var("SONDE_DEV_PROXY") {
        let proxy_target = proxy_target.trim().trim_end_matches('/');
        if !proxy_target.is_empty() {
            if is_loopback_proxy_target(proxy_target) {
                return proxy_dev_request(proxy_target, &request).await;
            }
            tracing::warn!(
                target = %proxy_target,
                "ignoring SONDE_DEV_PROXY because the target is not a loopback address"
            );
        }
    }

    if is_source_map(path) {
        return HttpResponse::NotFound().finish();
    }

    let asset_path = if path.is_empty() { "index.html" } else { path };
    if let Some(asset) = WebAssets::get(asset_path) {
        return response(asset_path, asset.data.as_ref());
    }
    if raw_path.starts_with("/api/") || raw_path.starts_with("/v1/") {
        return HttpResponse::NotFound()
            .json(serde_json::json!({ "code": "not_found", "message": "API route not found" }));
    }
    match WebAssets::get("index.html") {
        Some(asset) => response("index.html", asset.data.as_ref()),
        None => HttpResponse::ServiceUnavailable().body("Sonde web assets are not available"),
    }
}

#[cfg(debug_assertions)]
async fn proxy_dev_request(proxy_target: &str, request: &HttpRequest) -> HttpResponse {
    if !matches!(request.method().as_str(), "GET" | "HEAD") {
        return HttpResponse::MethodNotAllowed().finish();
    }

    let query = request.query_string();
    let target_url = if query.is_empty() {
        format!("{proxy_target}{}", request.path())
    } else {
        format!("{proxy_target}{}?{query}", request.path())
    };
    let client = match reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => return HttpResponse::BadGateway().body("Could not initialize Vite dev proxy"),
    };
    let mut client_req = if request.method().as_str() == "HEAD" {
        client.head(&target_url)
    } else {
        client.get(&target_url)
    };
    for (name, value) in request.headers() {
        if !is_safe_dev_request_header(name.as_str()) {
            continue;
        }
        let Ok(name) = reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes()) else {
            continue;
        };
        let Ok(value) = reqwest::header::HeaderValue::from_bytes(value.as_bytes()) else {
            continue;
        };
        client_req = client_req.header(name, value);
    }

    match client_req.send().await {
        Ok(res) => {
            let status = actix_web::http::StatusCode::from_u16(res.status().as_u16())
                .unwrap_or(actix_web::http::StatusCode::BAD_GATEWAY);
            let mut builder = HttpResponse::build(status);
            for (name, value) in res.headers() {
                if !is_safe_dev_response_header(name.as_str()) {
                    continue;
                }
                let Ok(name) = name.as_str().parse::<header::HeaderName>() else {
                    continue;
                };
                let Ok(value) = header::HeaderValue::from_bytes(value.as_bytes()) else {
                    continue;
                };
                builder.insert_header((name, value));
            }
            match res.bytes().await {
                Ok(body) => builder.body(body),
                Err(_) => HttpResponse::BadGateway()
                    .body("Failed to read response body from Vite dev server"),
            }
        }
        Err(_) => HttpResponse::BadGateway()
            .body("Could not connect to Vite dev server (is 'pnpm dev' running?)"),
    }
}

#[cfg(debug_assertions)]
fn is_safe_dev_request_header(name: &str) -> bool {
    matches!(
        name,
        "accept"
            | "accept-encoding"
            | "accept-language"
            | "cache-control"
            | "if-modified-since"
            | "if-none-match"
            | "range"
            | "user-agent"
    )
}

#[cfg(debug_assertions)]
fn is_safe_dev_response_header(name: &str) -> bool {
    matches!(
        name,
        "accept-ranges"
            | "cache-control"
            | "content-encoding"
            | "content-range"
            | "content-type"
            | "etag"
            | "last-modified"
            | "location"
            | "vary"
    )
}

/// Rejects source maps even if a stale `web/dist` still contains them.
fn is_source_map(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".map")
}

/// The development proxy may only ever forward to the local machine, so a stray
/// `SONDE_DEV_PROXY` value cannot turn the server into an open forward proxy.
#[cfg(debug_assertions)]
fn is_loopback_proxy_target(target: &str) -> bool {
    let Ok(parsed) = url::Url::parse(target) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    match parsed.host() {
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

fn response(path: &str, bytes: &[u8]) -> HttpResponse {
    let cache = if path == "index.html" {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    };
    HttpResponse::Ok()
        .insert_header((
            header::CONTENT_TYPE,
            mime_guess::from_path(path).first_or_octet_stream().as_ref(),
        ))
        .insert_header((header::CACHE_CONTROL, cache))
        .body(bytes.to_vec())
}
