//! The proxy the system's environment names for a URL, as command-line
//! tools read it: `HTTPS_PROXY`/`HTTP_PROXY`/`ALL_PROXY`, with `NO_PROXY`
//! exceptions. Loopback addresses never go through a proxy.

use std::net::IpAddr;

/// The proxy to use for `url`, or `None` to connect directly.
pub fn for_url(url: &str) -> Option<String> {
    for_url_with(url, |name| {
        std::env::var(name)
            .or_else(|_| std::env::var(name.to_lowercase()))
            .ok()
            .filter(|value| !value.trim().is_empty())
    })
}

fn for_url_with(url: &str, env: impl Fn(&str) -> Option<String>) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        bracketed.split(']').next()?
    } else {
        authority.split(':').next()?
    }
    .to_ascii_lowercase();
    if is_loopback(&host) || bypassed(&host, env("NO_PROXY").as_deref().unwrap_or("")) {
        return None;
    }
    let specific = match scheme.to_ascii_lowercase().as_str() {
        "https" => env("HTTPS_PROXY"),
        "http" => env("HTTP_PROXY"),
        _ => None,
    };
    specific.or_else(|| env("ALL_PROXY"))
}

fn is_loopback(host: &str) -> bool {
    host == "localhost"
        || host.ends_with(".localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Whether a `NO_PROXY` list exempts `host`: `*`, a domain (with or
/// without a leading dot) and its subdomains, an IP address, or an IPv4 or
/// IPv6 range in CIDR notation.
fn bypassed(host: &str, list: &str) -> bool {
    let ip = host.parse::<IpAddr>().ok();
    list.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .any(|entry| {
            let entry = entry.to_ascii_lowercase();
            if entry == "*" {
                return true;
            }
            if let Some(ip) = ip {
                if let Some((network, bits)) = entry.split_once('/') {
                    return in_range(ip, network, bits);
                }
                return entry.trim_matches(['[', ']']).parse::<IpAddr>().ok() == Some(ip);
            }
            let domain = entry.trim_start_matches("*.").trim_start_matches('.');
            host == domain || host.ends_with(&format!(".{domain}"))
        })
}

fn in_range(ip: IpAddr, network: &str, bits: &str) -> bool {
    let (Ok(network), Ok(bits)) = (network.parse::<IpAddr>(), bits.parse::<u32>()) else {
        return false;
    };
    match (ip, network) {
        (IpAddr::V4(ip), IpAddr::V4(network)) if bits <= 32 => {
            let mask = u32::MAX.checked_shl(32 - bits).unwrap_or(0);
            u32::from(ip) & mask == u32::from(network) & mask
        }
        (IpAddr::V6(ip), IpAddr::V6(network)) if bits <= 128 => {
            let mask = u128::MAX.checked_shl(128 - bits).unwrap_or(0);
            u128::from(ip) & mask == u128::from(network) & mask
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(url: &str, vars: &[(&str, &str)]) -> Option<String> {
        for_url_with(url, |name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_string())
        })
    }

    #[test]
    fn picks_the_proxy_for_the_scheme() {
        let vars = [
            ("HTTPS_PROXY", "http://secure:8080"),
            ("HTTP_PROXY", "http://plain:8080"),
        ];
        assert_eq!(
            with("https://api.open-meteo.com/v1", &vars).as_deref(),
            Some("http://secure:8080")
        );
        assert_eq!(
            with("http://example.com", &vars).as_deref(),
            Some("http://plain:8080")
        );
        assert_eq!(
            with("https://example.com", &[("ALL_PROXY", "socks5://all:1080")]).as_deref(),
            Some("socks5://all:1080")
        );
        assert_eq!(with("https://example.com", &[]), None);
    }

    #[test]
    fn connects_directly_to_loopback_and_exceptions() {
        let vars = [
            ("HTTPS_PROXY", "http://proxy:8080"),
            ("HTTP_PROXY", "http://proxy:8080"),
            (
                "NO_PROXY",
                "example.com, .internal.test,10.0.0.0/8,192.168.1.5",
            ),
        ];
        for direct in [
            "http://127.0.0.1:4000/image.png",
            "http://localhost:3000",
            "http://[::1]:80/",
            "https://example.com/a",
            "https://cdn.example.com",
            "https://git.internal.test/repo",
            "http://10.2.3.4/x",
            "http://192.168.1.5",
        ] {
            assert_eq!(with(direct, &vars), None, "{direct}");
        }
        for proxied in [
            "https://notexample.com",
            "http://192.168.1.6",
            "https://github.com",
        ] {
            assert!(with(proxied, &vars).is_some(), "{proxied}");
        }
        assert_eq!(
            with(
                "https://github.com",
                &[("HTTPS_PROXY", "http://p:1"), ("NO_PROXY", "*")]
            ),
            None
        );
    }
}
