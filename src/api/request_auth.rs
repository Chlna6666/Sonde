use std::net::IpAddr;

use actix_web::{HttpRequest, http::header};
use url::Url;

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

/// Rejects a cross-site request when the browser supplied an `Origin` header.
///
/// Origin matching is schemeful. A TLS-terminating proxy may describe the browser-facing scheme
/// and host through `Forwarded` / `X-Forwarded-*`, but those headers are considered only when
/// the socket peer is explicitly listed in `SONDE_TRUSTED_PROXIES`. Otherwise a direct client
/// cannot redefine the server origin by injecting proxy headers.
pub(crate) fn reject_cross_site_origin(
    request: &HttpRequest,
    trusted_proxies: &[IpAddr],
    fallback_scheme: &str,
) -> Result<(), AppError> {
    let Some(origin) = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return Ok(());
    };

    origin_matches_request(request, origin, trusted_proxies, fallback_scheme)
        .then_some(())
        .ok_or(AppError::Forbidden)
}

/// Matches a serialized browser origin against the public origin of this request.
///
/// `fallback_scheme` comes from server-side deployment/session policy. A trusted reverse proxy
/// may override it with `Forwarded: proto=` or `X-Forwarded-Proto`. Host overrides are accepted
/// under the same trusted-peer rule.
pub(crate) fn origin_matches_request(
    request: &HttpRequest,
    origin: &str,
    trusted_proxies: &[IpAddr],
    fallback_scheme: &str,
) -> bool {
    let Ok(origin) = Url::parse(origin.trim()) else {
        return false;
    };
    if !matches!(origin.scheme(), "http" | "https")
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
    {
        return false;
    }

    let peer_is_trusted = request
        .peer_addr()
        .is_some_and(|address| trusted_proxies.contains(&address.ip()));
    let scheme = if peer_is_trusted {
        forwarded_parameter(request, "proto")
            .or_else(|| first_header_value(request, "x-forwarded-proto"))
            .filter(|value| matches!(value.as_str(), "http" | "https"))
            .unwrap_or_else(|| fallback_scheme.to_ascii_lowercase())
    } else {
        fallback_scheme.to_ascii_lowercase()
    };
    if !matches!(scheme.as_str(), "http" | "https") || !origin.scheme().eq_ignore_ascii_case(&scheme)
    {
        return false;
    }

    let mut authorities = Vec::with_capacity(3);
    if let Some(host) = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    {
        authorities.push(host.trim().to_owned());
    }
    if peer_is_trusted {
        if let Some(host) = forwarded_parameter(request, "host") {
            authorities.push(host);
        }
        if let Some(host) = first_header_value(request, "x-forwarded-host") {
            authorities.push(host);
        }
    }

    authorities
        .iter()
        .any(|authority| same_origin(&origin, &scheme, authority))
}

fn forwarded_parameter(request: &HttpRequest, name: &str) -> Option<String> {
    let raw = request.headers().get(header::FORWARDED)?.to_str().ok()?;
    raw.split(',')
        .next()?
        .split(';')
        .find_map(|parameter| {
            let (key, value) = parameter.trim().split_once('=')?;
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim().trim_matches('"').to_owned())
        })
}

fn first_header_value(request: &HttpRequest, name: &str) -> Option<String> {
    request
        .headers()
        .get(name)?
        .to_str()
        .ok()?
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn same_origin(origin: &Url, scheme: &str, authority: &str) -> bool {
    let authority = authority.trim().trim_end_matches('/');
    if authority.is_empty() {
        return false;
    }
    let Ok(candidate) = Url::parse(&format!("{scheme}://{authority}")) else {
        return false;
    };
    if !candidate.username().is_empty()
        || candidate.password().is_some()
        || candidate.path() != "/"
        || candidate.query().is_some()
        || candidate.fragment().is_some()
    {
        return false;
    }

    origin
        .host_str()
        .zip(candidate.host_str())
        .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))
        && origin.port_or_known_default() == candidate.port_or_known_default()
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
        && let Some(forwarded) = forwarded_client_ip(request, trusted_proxies)
    {
        return forwarded.to_string();
    }
    peer.map_or_else(|| "unknown".into(), |address| address.to_string())
}

fn forwarded_client_ip(request: &HttpRequest, trusted_proxies: &[IpAddr]) -> Option<IpAddr> {
    let chain = forwarded_chain(request);
    // Standard proxies append their hop to Forwarded/X-Forwarded-For. Walk from the socket
    // peer outwards, discarding only explicitly trusted hops. Taking the first header entry
    // instead would let a client pre-seed X-Forwarded-For with an arbitrary rate-limit identity.
    chain
        .iter()
        .rev()
        .find(|address| !trusted_proxies.contains(address))
        .copied()
        .or_else(|| chain.first().copied())
}

fn forwarded_chain(request: &HttpRequest) -> Vec<IpAddr> {
    if let Some(value) = request.headers().get(header::FORWARDED)
        && let Ok(raw) = value.to_str()
    {
        let chain: Vec<IpAddr> = raw
            .split(',')
            .filter_map(|element| {
                element.split(';').find_map(|parameter| {
                    let (name, value) = parameter.trim().split_once('=')?;
                    name.eq_ignore_ascii_case("for")
                        .then(|| parse_forwarded_ip(value))
                        .flatten()
                })
            })
            .collect();
        if !chain.is_empty() {
            return chain;
        }
    }

    request
        .headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .map(|raw| raw.split(',').filter_map(parse_forwarded_ip).collect())
        .unwrap_or_default()
}

fn parse_forwarded_ip(raw: &str) -> Option<IpAddr> {
    let trimmed = raw.trim().trim_matches('"');
    if trimmed.is_empty() {
        return None;
    }
    if let Some(inner) = trimmed.strip_prefix('[') {
        let host = inner.split(']').next()?;
        return host.parse::<IpAddr>().ok();
    }
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return Some(ip);
    }
    let host = trimmed.rsplit_once(':').map_or(trimmed, |(host, _)| host);
    host.parse::<IpAddr>().ok()
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    use super::{client_ip, origin_matches_request, parse_forwarded_ip, reject_cross_site_origin};
    use actix_web::test::TestRequest;

    #[test]
    fn same_origin_requests_pass_and_cross_site_origins_are_rejected() {
        let same_origin = TestRequest::default()
            .insert_header(("host", "sonde.example.com"))
            .insert_header(("origin", "http://sonde.example.com"))
            .to_http_request();
        assert!(reject_cross_site_origin(&same_origin, &[], "http").is_ok());

        let cross_site = TestRequest::default()
            .insert_header(("host", "sonde.example.com"))
            .insert_header(("origin", "https://attacker.example"))
            .to_http_request();
        assert!(reject_cross_site_origin(&cross_site, &[], "http").is_err());

        let opaque_origin = TestRequest::default()
            .insert_header(("host", "sonde.example.com"))
            .insert_header(("origin", "null"))
            .to_http_request();
        assert!(reject_cross_site_origin(&opaque_origin, &[], "http").is_err());

        let absent_origin = TestRequest::default()
            .insert_header(("host", "sonde.example.com"))
            .to_http_request();
        assert!(reject_cross_site_origin(&absent_origin, &[], "http").is_ok());
    }

    #[test]
    fn proxy_rewritten_host_still_accepts_the_browser_origin() {
        let edge = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        let rewritten = TestRequest::default()
            .peer_addr(SocketAddr::new(edge, 43123))
            .insert_header(("host", "sonde-upstream:8080"))
            .insert_header(("x-forwarded-host", "sonde.example.com"))
            .insert_header(("x-forwarded-proto", "https"))
            .insert_header(("origin", "https://sonde.example.com"))
            .to_http_request();
        assert!(reject_cross_site_origin(&rewritten, &[edge], "http").is_ok());
    }

    #[test]
    fn forwarded_host_chain_and_default_port_do_not_break_a_valid_origin() {
        let edge = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));

        let chained = TestRequest::default()
            .peer_addr(SocketAddr::new(edge, 43123))
            .insert_header(("host", "sonde-upstream:8080"))
            .insert_header(("x-forwarded-host", "sonde.example.com, proxy.internal"))
            .insert_header(("x-forwarded-proto", "https"))
            .insert_header(("origin", "https://sonde.example.com"))
            .to_http_request();
        assert!(reject_cross_site_origin(&chained, &[edge], "http").is_ok());

        let explicit_port = TestRequest::default()
            .peer_addr(SocketAddr::new(edge, 43123))
            .insert_header(("host", "sonde-upstream:8080"))
            .insert_header(("x-forwarded-host", "sonde.example.com:443"))
            .insert_header(("x-forwarded-proto", "https"))
            .insert_header(("origin", "https://sonde.example.com"))
            .to_http_request();
        assert!(reject_cross_site_origin(&explicit_port, &[edge], "http").is_ok());

        let later_hop = TestRequest::default()
            .peer_addr(SocketAddr::new(edge, 43123))
            .insert_header(("host", "sonde-upstream:8080"))
            .insert_header(("x-forwarded-host", "sonde.example.com, attacker.example"))
            .insert_header(("x-forwarded-proto", "https"))
            .insert_header(("origin", "https://attacker.example"))
            .to_http_request();
        assert!(reject_cross_site_origin(&later_hop, &[edge], "http").is_err());
    }

    #[test]
    fn same_authority_with_different_scheme_is_cross_origin() {
        let request = TestRequest::default()
            .insert_header(("host", "sonde.example.com"))
            .insert_header(("origin", "https://sonde.example.com"))
            .to_http_request();
        assert!(reject_cross_site_origin(&request, &[], "http").is_err());
    }

    #[test]
    fn untrusted_peer_cannot_redefine_origin_with_forwarded_headers() {
        let peer = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10));
        let request = TestRequest::default()
            .peer_addr(SocketAddr::new(peer, 43123))
            .insert_header(("host", "sonde.example.com"))
            .insert_header(("x-forwarded-host", "attacker.example"))
            .insert_header(("x-forwarded-proto", "https"))
            .insert_header(("origin", "https://attacker.example"))
            .to_http_request();
        assert!(!origin_matches_request(
            &request,
            "https://attacker.example",
            &[],
            "http"
        ));
    }

    #[test]
    fn forwarded_literals_parse_ipv4_and_ipv6() {
        let ipv4 = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10));
        let ipv6 = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        assert_eq!(parse_forwarded_ip("203.0.113.10"), Some(ipv4));
        assert_eq!(parse_forwarded_ip("203.0.113.10:443"), Some(ipv4));
        assert_eq!(parse_forwarded_ip("[2001:db8::1]"), Some(ipv6));
        assert_eq!(parse_forwarded_ip("[2001:db8::1]:8080"), Some(ipv6));
        assert_eq!(parse_forwarded_ip("2001:db8::1"), Some(ipv6));
        assert_eq!(parse_forwarded_ip("unknown"), None);
    }

    #[test]
    fn trusted_proxy_chain_ignores_client_supplied_leftmost_spoof() {
        let edge = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        let inner_proxy = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3));
        let request = TestRequest::default()
            .peer_addr(SocketAddr::new(edge, 43123))
            .insert_header(("x-forwarded-for", "192.0.2.123, 198.51.100.44, 10.0.0.3"))
            .to_http_request();

        assert_eq!(client_ip(&request, &[edge, inner_proxy]), "198.51.100.44");
    }

    #[test]
    fn forwarded_header_chain_uses_first_untrusted_hop_from_the_right() {
        let edge = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        let inner_proxy = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3));
        let request = TestRequest::default()
            .peer_addr(SocketAddr::new(edge, 43123))
            .insert_header((
                "forwarded",
                "for=192.0.2.123;proto=https, for=198.51.100.44, for=10.0.0.3",
            ))
            .to_http_request();

        assert_eq!(client_ip(&request, &[edge, inner_proxy]), "198.51.100.44");
    }
}
