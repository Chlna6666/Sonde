use std::net::IpAddr;

use actix_web::{HttpRequest, http::header};

use crate::{auth, services::authentication::AuthRequest};

impl AuthRequest for HttpRequest {
    fn session_token(&self) -> Option<String> {
        self.cookie(auth::SESSION_COOKIE)
            .or_else(|| self.cookie(auth::DEVELOPMENT_SESSION_COOKIE))
            .map(|cookie| cookie.value().to_owned())
    }

    fn csrf_token(&self) -> Option<&str> {
        self.headers()
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok())
    }
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
    use super::parse_forwarded_ip;

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
