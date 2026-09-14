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

    if let Ok(proxy_target) = std::env::var("SONDE_DEV_PROXY") {
        let proxy_target = proxy_target.trim().trim_end_matches('/');
        if !proxy_target.is_empty() {
            return proxy_dev_request(proxy_target, &request).await;
        }
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

async fn proxy_dev_request(proxy_target: &str, request: &HttpRequest) -> HttpResponse {
    let client = awc::Client::default();
    let query = request.query_string();
    let target_url = if query.is_empty() {
        format!("{proxy_target}{}", request.path())
    } else {
        format!("{proxy_target}{}?{query}", request.path())
    };

    let mut client_req = client.request(request.method().clone(), target_url);
    for (name, value) in request.headers() {
        if name != "host" && name != "content-length" {
            client_req = client_req.insert_header((name.clone(), value.clone()));
        }
    }

    match client_req.send().await {
        Ok(mut res) => {
            let mut builder = HttpResponse::build(res.status());
            for (name, value) in res.headers() {
                if name != "content-length" {
                    builder.insert_header((name.clone(), value.clone()));
                }
            }
            match res.body().await {
                Ok(body) => builder.body(body),
                Err(_) => HttpResponse::BadGateway()
                    .body("Failed to read response body from Vite dev server"),
            }
        }
        Err(_) => HttpResponse::BadGateway()
            .body("Could not connect to Vite dev server (is 'pnpm dev' running?)"),
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
