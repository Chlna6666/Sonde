use std::net::IpAddr;

use actix_web::{HttpRequest, http::header};

use crate::{auth, error::AppError, services::authentication::AuthRequest};

impl AuthRequest for HttpRequest {
    fn session_token(&self, secure_cookie: bool) -> Option<String> {
        let name = if secure_cookie {
            auth::SESSION_COOKIE
        } else {
            auth::DEVELOPMENT_SESSION_COOKIE
        };
        self.cookie(name).map(|cookie| cookie.value().to_owned())
    }

    fn csrf_token(&self) -> Option<&str> {
        self.headers()
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok())
    }
}

/// Rejects a cross-site request when the browser attached an `Origin` header that does not
/// match the request host.
///
/// Browsers always send `Origin` on cross-origin `POST` requests, so this blocks login CSRF
/// (forcing a victim into an attacker-controlled account) without breaking same-origin
/// clients that omit the header. Both the raw `Host` header and the connection info are
/// accepted so that a terminating proxy which rewrites `Host` but sets
/// `Forwarded`/`X-Forwarded-Host` still works. A cross-site page can forge neither: custom
/// headers require a CORS preflight, which this server never approves.
pub(crate) fn reject_cross_site_origin(request: &HttpRequest) -> Result<(), AppError> {
    let Some(origin) = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return Ok(());
    };
    let Some((_, authority)) = origin.trim().split_once("://") else {
        return Err(AppError::Forbidden);
    };
    let authority = authority.trim_end_matches('/');
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::trim);
    let forwarded_host = request.connection_info().host().to_owned();
    if authority.is_empty() || (host.is_none() && forwarded_host.is_empty()) {
        return Err(AppError::Forbidden);
    }
    let matches = host.is_some_and(|host| authority.eq_ignore_ascii_case(host))
        || (!forwarded_host.is_empty() && authority.eq_ignore_ascii_case(&forwarded_host));
    matches.then_some(()).ok_or(AppError::Forbidden)
}

pub(crate) fn bearer_token(request: &HttpRequest) -> Option<&str> {
    request
        .headers()
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

pub(crate) fn client_ip(request: &HttpRequest, trusted_proxies: &[IpAddr]) -> String {
    let peer = request.peer_addr().map(|address| address.ip());
    if let Some(peer) = peer
        && trusted_proxies.contains(&peer)
        && let Some(forwarded) = forwarded_client_ip(request)
    {
        return forwarded;
    }
    peer.map_or_else(|| "unknown".into(), |address| address.to_string())
}

fn forwarded_client_ip(request: &HttpRequest) -> Option<String> {
    if let Some(value) = request.headers().get(header::FORWARDED)
        && let Ok(raw) = value.to_str()
        && let Some(part) = raw.split([',', ';']).find_map(|part| {
            part.trim()
                .strip_prefix("for=")
                .or_else(|| part.trim().strip_prefix("For="))
        })
        && let Some(ip) = parse_forwarded_ip(part)
    {
        return Some(ip);
    }
    request
        .headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.split(',').next())
        .and_then(parse_forwarded_ip)
}

fn parse_forwarded_ip(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('"');
    if trimmed.is_empty() {
        return None;
    }
    if let Some(inner) = trimmed.strip_prefix('[') {
        let host = inner.split(']').next()?;
        return host.parse::<IpAddr>().ok().map(|ip| ip.to_string());
    }
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return Some(ip.to_string());
    }
    let host = trimmed.rsplit_once(':').map_or(trimmed, |(host, _)| host);
    host.parse::<IpAddr>().ok().map(|ip| ip.to_string())
}

#[cfg(test)]
mod tests {
    use super::{parse_forwarded_ip, reject_cross_site_origin};
    use actix_web::test::TestRequest;

    #[test]
    fn same_origin_requests_pass_and_cross_site_origins_are_rejected() {
        let same_origin = TestRequest::default()
            .insert_header(("host", "sonde.example.com"))
            .insert_header(("origin", "https://sonde.example.com"))
            .to_http_request();
        assert!(reject_cross_site_origin(&same_origin).is_ok());

        let cross_site = TestRequest::default()
            .insert_header(("host", "sonde.example.com"))
            .insert_header(("origin", "https://attacker.example"))
            .to_http_request();
        assert!(reject_cross_site_origin(&cross_site).is_err());

        let opaque_origin = TestRequest::default()
            .insert_header(("host", "sonde.example.com"))
            .insert_header(("origin", "null"))
            .to_http_request();
        assert!(reject_cross_site_origin(&opaque_origin).is_err());

        let absent_origin = TestRequest::default()
            .insert_header(("host", "sonde.example.com"))
            .to_http_request();
        assert!(reject_cross_site_origin(&absent_origin).is_ok());
    }

    #[test]
    fn proxy_rewritten_host_still_accepts_the_browser_origin() {
        let rewritten = TestRequest::default()
            .insert_header(("host", "sonde-upstream:8080"))
            .insert_header(("x-forwarded-host", "sonde.example.com"))
            .insert_header(("origin", "https://sonde.example.com"))
            .to_http_request();
        assert!(reject_cross_site_origin(&rewritten).is_ok());
    }

    #[test]
    fn forwarded_literals_parse_ipv4_and_ipv6() {
        assert_eq!(
            parse_forwarded_ip("203.0.113.10"),
            Some("203.0.113.10".into())
        );
        assert_eq!(
            parse_forwarded_ip("203.0.113.10:443"),
            Some("203.0.113.10".into())
        );
        assert_eq!(
            parse_forwarded_ip("[2001:db8::1]"),
            Some("2001:db8::1".into())
        );
        assert_eq!(
            parse_forwarded_ip("[2001:db8::1]:8080"),
            Some("2001:db8::1".into())
        );
        assert_eq!(
            parse_forwarded_ip("2001:db8::1"),
            Some("2001:db8::1".into())
        );
        assert_eq!(parse_forwarded_ip("unknown"), None);
    }
}
