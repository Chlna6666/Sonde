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
    // `null` and other opaque origins carry no authority, so they can never match this server.
    let Some((_, authority)) = origin.trim().split_once("://") else {
        return Err(AppError::Forbidden);
    };
    let authority = authority.trim_end_matches('/');
    (!authority.is_empty() && origin_authority_matches(request, authority))
        .then_some(())
        .ok_or(AppError::Forbidden)
}

/// True when `authority` — the `host[:port]` part of an `Origin` header — names this server as
/// the client addressed it.
///
/// Only the authority is compared, never the scheme. The security property comes from the host:
/// a cross-site page cannot make a browser send an `Origin` for a host it is not on. The scheme
/// is deliberately ignored because a TLS-terminating proxy presents `http` internally while the
/// browser sees `https`, and rejecting that mismatch would only break correctly deployed setups.
pub(crate) fn origin_authority_matches(request: &HttpRequest, authority: &str) -> bool {
    // `ConnectionInfo::host()` already prefers `Forwarded` and `X-Forwarded-Host` over `Host`,
    // so behind a proxy this is the public name rather than the internal one.
    let connection = request.connection_info();
    let host_header = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());
    // Only the leading hop is compared: it is the host the client originally addressed, while
    // anything after the first comma is a proxy's own view of it.
    [host_header, Some(connection.host())]
        .into_iter()
        .flatten()
        .filter_map(|candidate| candidate.split(',').next())
        .any(|candidate| same_authority(candidate, authority))
}

/// Compares two authorities, tolerating the default port a proxy may keep and a browser omits.
fn same_authority(candidate: &str, authority: &str) -> bool {
    let candidate = strip_default_port(candidate.trim().trim_end_matches('/'));
    let authority = strip_default_port(authority.trim());
    candidate.eq_ignore_ascii_case(authority)
}

fn strip_default_port(value: &str) -> &str {
    [":443", ":80"]
        .iter()
        .find_map(|port| value.strip_suffix(port))
        .unwrap_or(value)
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
    fn forwarded_host_chain_and_default_port_do_not_break_a_valid_origin() {
        // Proxies append every hop to `X-Forwarded-Host`; the browser only ever names the first.
        let chained = TestRequest::default()
            .insert_header(("host", "sonde-upstream:8080"))
            .insert_header(("x-forwarded-host", "sonde.example.com, proxy.internal"))
            .insert_header(("origin", "https://sonde.example.com"))
            .to_http_request();
        assert!(reject_cross_site_origin(&chained).is_ok());

        // A proxy that keeps the default port must not fight the browser, which omits it.
        let explicit_port = TestRequest::default()
            .insert_header(("host", "sonde-upstream:8080"))
            .insert_header(("x-forwarded-host", "sonde.example.com:443"))
            .insert_header(("origin", "https://sonde.example.com"))
            .to_http_request();
        assert!(reject_cross_site_origin(&explicit_port).is_ok());

        // The scheme a proxy presents internally differs from what the browser sees; that alone
        // is not a cross-site request.
        let terminated_tls = TestRequest::default()
            .insert_header(("host", "sonde.example.com"))
            .insert_header(("origin", "https://sonde.example.com"))
            .to_http_request();
        assert!(reject_cross_site_origin(&terminated_tls).is_ok());

        // Later hops belong to the proxies, not to the client that addressed this server, so a
        // host that only appears after the first comma is still a different site.
        let later_hop = TestRequest::default()
            .insert_header(("host", "sonde-upstream:8080"))
            .insert_header(("x-forwarded-host", "sonde.example.com, attacker.example"))
            .insert_header(("origin", "https://attacker.example"))
            .to_http_request();
        assert!(reject_cross_site_origin(&later_hop).is_err());

        let unrelated_host = TestRequest::default()
            .insert_header(("host", "sonde.example.com"))
            .insert_header(("origin", "https://sonde.example.evil"))
            .to_http_request();
        assert!(reject_cross_site_origin(&unrelated_host).is_err());
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
