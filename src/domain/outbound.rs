use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

const BLOCKED_PUBLIC_HOSTS: &[&str] = &[
    "localhost",
    "metadata.google.internal",
    "metadata.google.com",
    "metadata",
];

pub fn validate_https_http_url(raw: &str) -> Result<(), &'static str> {
    validate_url(raw, false)
}

pub fn validate_public_link(raw: &str) -> Result<(), &'static str> {
    validate_url(raw, true)
}

pub fn validate_outbound_url(raw: &str) -> Result<(), String> {
    validate_https_http_url(raw).map_err(str::to_owned)
}

fn validate_url(raw: &str, https_only: bool) -> Result<(), &'static str> {
    if raw.len() > 2_048 {
        return Err("URL is too long");
    }
    let parsed = Url::parse(raw).map_err(|_| "URL is invalid")?;
    if https_only {
        if parsed.scheme() != "https" {
            return Err("URL must use https");
        }
    } else if !matches!(parsed.scheme(), "http" | "https") {
        return Err("URL must use http or https");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("URL must not contain userinfo credentials");
    }
    match parsed.host().ok_or("URL must contain a host")? {
        Host::Domain(domain) => validate_domain(domain),
        Host::Ipv4(address) => {
            if is_public_ipv4(address) {
                Ok(())
            } else {
                Err("URL must target a public IPv4 address")
            }
        }
        Host::Ipv6(address) => {
            if is_public_ipv6(address) {
                Ok(())
            } else {
                Err("URL must target a public IPv6 address")
            }
        }
    }
}

fn validate_domain(domain: &str) -> Result<(), &'static str> {
    let domain = domain.trim_end_matches('.').to_ascii_lowercase();
    if domain.is_empty() {
        return Err("URL must contain a host");
    }
    if BLOCKED_PUBLIC_HOSTS.contains(&domain.as_str())
        || domain.ends_with(".localhost")
        || domain.ends_with(".local")
        || domain.ends_with(".internal")
        || domain.ends_with(".lan")
        || domain.ends_with(".home")
        || domain.ends_with(".corp")
        || domain.ends_with(".invalid")
    {
        return Err("URL must not target a local host");
    }
    if domain.parse::<IpAddr>().is_ok() {
        return Err("URL host must not be a literal IP in domain form");
    }
    Ok(())
}

#[must_use]
pub fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_ipv4(address),
        IpAddr::V6(address) => is_public_ipv6(address),
    }
}

#[must_use]
pub fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let [a, b, _, _] = address.octets();
    !(address.is_unspecified()
        || address.is_loopback()
        || address.is_broadcast()
        || address.is_link_local()
        || address.is_multicast()
        || a == 10
        || (a == 100 && (64..=127).contains(&b))
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 198 && (18..=19).contains(&b))
        || a >= 224)
}

#[must_use]
pub fn is_public_ipv6(address: Ipv6Addr) -> bool {
    if let Some(mapped) = address.to_ipv4_mapped() {
        return is_public_ipv4(mapped);
    }
    !(address.is_loopback()
        || address.is_unspecified()
        || address.is_unique_local()
        || address.is_unicast_link_local()
        || address.is_multicast()
        || is_ipv6_documentation(address)
        || is_ipv6_discard(address)
        || is_ipv6_nat64(address)
        || is_ipv6_teredo_or_6to4(address))
}

fn is_ipv6_documentation(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    segments[0] == 0x2001 && segments[1] == 0x0db8
}

fn is_ipv6_discard(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    segments[0] == 0x0100 && segments[1] == 0 && segments[2] == 0 && segments[3] == 0
}

fn is_ipv6_nat64(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    segments[0] == 0x64 && segments[1] == 0xff9b
}

fn is_ipv6_teredo_or_6to4(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    (segments[0] == 0x2001 && segments[1] == 0) || segments[0] == 0x2002
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::{is_public_ipv4, is_public_ipv6, validate_outbound_url, validate_public_link};
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn private_notification_targets_are_rejected() {
        assert!(!is_public_ipv4(Ipv4Addr::new(127, 0, 0, 1)));
        assert!(!is_public_ipv4(Ipv4Addr::new(10, 0, 0, 1)));
        assert!(!is_public_ipv4(Ipv4Addr::new(169, 254, 0, 1)));
        assert!(!is_public_ipv6(Ipv6Addr::LOCALHOST));
        assert!(!is_public_ipv6("::ffff:127.0.0.1".parse().expect("mapped")));
        assert!(!is_public_ipv6("64:ff9b::7f00:1".parse().expect("nat64")));
        assert!(validate_outbound_url("http://127.0.0.1/hook").is_err());
        assert!(validate_outbound_url("http://localhost/hook").is_err());
        assert!(validate_outbound_url("http://metadata.google.internal/").is_err());
        assert!(validate_outbound_url("https://example.com/hook").is_ok());
    }

    #[test]
    fn public_links_require_https() {
        assert!(validate_public_link("javascript:alert(1)").is_err());
        assert!(validate_public_link("http://example.com").is_err());
        assert!(validate_public_link("https://github.com/example").is_ok());
    }
}
