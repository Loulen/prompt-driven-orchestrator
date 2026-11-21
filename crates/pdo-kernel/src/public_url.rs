//! The instance's « Public URL »: the address at which humans open PDO (a VPN
//! name, a LAN address, a reverse proxy). Agents receive it in their
//! environment and preamble, and `pdo page mount` prints its links with it.
//!
//! It always has a value. The precedence is the usual `stored → env → default`
//! (ADR-0015); the default is the machine's address, [`default_public_url`],
//! built once from where the daemon listens. The stored tier is kept by the
//! settings context, the run and platform contexts read it through their ports
//! and resolve it here.

use std::net::IpAddr;

/// The environment tier of the setting, and the variable a node session
/// receives it in.
pub const PUBLIC_URL_ENV: &str = "PDO_PUBLIC_URL";

/// The canonical form of a Public URL: an origin, `http(s)://host[:port]`,
/// without a trailing `/`. No path: the interface and the pages are served from
/// the root of the address. `Ok(None)` for a blank value (the clear); `Err`
/// with the reason otherwise.
pub fn normalize(raw: &str) -> Result<Option<String>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let invalid = |why: &str| Err(format!("public_url `{trimmed}` {why}"));
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return invalid("must start with http:// or https://");
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return invalid("must start with http:// or https://");
    }
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return invalid("must not contain spaces");
    }
    if trimmed.contains(['?', '#']) {
        return invalid("must not carry a query or a fragment");
    }
    let host = rest.split('/').next().unwrap_or_default();
    if host.is_empty() || host.starts_with(':') || host.contains('@') {
        return invalid("must name a host");
    }
    if !rest[host.len()..].trim_end_matches('/').is_empty() {
        return invalid("must be an origin, without a path: the app is served from the root");
    }
    Ok(Some(format!("{}://{host}", scheme.to_ascii_lowercase())))
}

/// The environment tier: [`PUBLIC_URL_ENV`], normalized; an invalid value is
/// ignored, with one warning per process, rather than handed to the agents.
pub fn env_public_url() -> Option<String> {
    static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let raw = std::env::var(PUBLIC_URL_ENV).ok()?;
    match normalize(&raw) {
        Ok(url) => url,
        Err(why) => {
            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                tracing::warn!("{PUBLIC_URL_ENV} ignored: {why}");
            }
            None
        }
    }
}

/// The default tier: the machine's address, from the address the daemon
/// listens on (`listen`, `port`) and the machine's `host_name`. A loopback
/// address gives `http://localhost:<port>`: only this machine reaches the
/// daemon. An unspecified one (`0.0.0.0`, `::`, every interface) gives
/// `http://<host name>:<port>`, `localhost` when the system names no host. A
/// precise address gives itself.
pub fn default_public_url(listen: IpAddr, port: u16, host_name: Option<&str>) -> String {
    // `::ffff:127.0.0.1` is the IPv4 loopback, written as IPv6.
    let listen = listen.to_canonical();
    let host = if listen.is_loopback() {
        "localhost".to_string()
    } else if listen.is_unspecified() {
        host_name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or("localhost")
            .to_string()
    } else {
        match listen {
            IpAddr::V4(ip) => ip.to_string(),
            IpAddr::V6(ip) => format!("[{ip}]"),
        }
    };
    format!("http://{host}:{port}")
}

/// `stored → env → default`. A stored value that no longer normalizes is
/// skipped.
pub fn resolve(stored: Option<&str>, env: Option<String>, default: &str) -> String {
    stored
        .and_then(|raw| normalize(raw).ok().flatten())
        .or(env)
        .unwrap_or_else(|| default.to_string())
}

/// The address of the page mounted as `name`, alone, under `base`.
pub fn page_url(base: &str, name: &str) -> String {
    format!("{}/pages/{name}/", base.trim_end_matches('/'))
}

/// The address of PDO's view of the page mounted as `name`, with its guide.
pub fn guided_page_url(base: &str, name: &str) -> String {
    format!("{}/#/pages/{name}", base.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_public_url_is_an_http_origin_with_an_optional_path() {
        assert_eq!(
            normalize(" https://pdo.example.net/ ").unwrap(),
            Some("https://pdo.example.net".into())
        );
        assert_eq!(
            normalize("HTTP://10.0.0.4:5272//").unwrap(),
            Some("http://10.0.0.4:5272".into())
        );
        assert_eq!(normalize("").unwrap(), None);
        assert_eq!(normalize("   ").unwrap(), None);
    }

    #[test]
    fn anything_else_is_refused_with_its_reason() {
        for raw in [
            "pdo.example.net",
            "ftp://pdo.example.net",
            "https://",
            "https://:5272",
            "https://pdo.example.net/?a=1",
            "https://pdo.example.net/#/runs",
            "https://pdo example.net",
            "https://user@pdo.example.net",
            "https://pdo.example.net/pdo",
        ] {
            let error = normalize(raw).expect_err(raw);
            assert!(error.contains("public_url"), "{error}");
        }
    }

    #[test]
    fn the_stored_tier_wins_over_the_environment_then_the_default() {
        let env = Some("http://env:1".to_string());
        let default = "http://host:5272";
        assert_eq!(
            resolve(Some("https://stored/"), env.clone(), default),
            "https://stored"
        );
        assert_eq!(resolve(None, env.clone(), default), "http://env:1");
        assert_eq!(
            resolve(Some("not a url"), env.clone(), default),
            "http://env:1"
        );
        assert_eq!(resolve(Some("not a url"), None, default), default);
        assert_eq!(resolve(None, None, default), default);
    }

    #[test]
    fn the_default_is_the_machine_s_address_where_the_daemon_listens() {
        let every = IpAddr::from([0, 0, 0, 0]);
        let every_v6 = IpAddr::from(std::net::Ipv6Addr::UNSPECIFIED);
        assert_eq!(
            default_public_url(every, 5272, Some("atelier")),
            "http://atelier:5272"
        );
        assert_eq!(
            default_public_url(every_v6, 5272, Some("atelier")),
            "http://atelier:5272"
        );
        // No host name from the system: the only name left is this machine's.
        assert_eq!(
            default_public_url(every, 5272, None),
            "http://localhost:5272"
        );
        assert_eq!(
            default_public_url(every, 5272, Some(" ")),
            "http://localhost:5272"
        );
    }

    #[test]
    fn a_loopback_daemon_defaults_to_localhost() {
        for ip in [
            IpAddr::from([127, 0, 0, 1]),
            IpAddr::from(std::net::Ipv6Addr::LOCALHOST),
            // The IPv4 loopback written as IPv6.
            "::ffff:127.0.0.1".parse().unwrap(),
        ] {
            assert_eq!(
                default_public_url(ip, 4100, Some("atelier")),
                "http://localhost:4100"
            );
        }
    }

    #[test]
    fn a_precise_listen_address_is_its_own_default() {
        assert_eq!(
            default_public_url(IpAddr::from([10, 0, 0, 4]), 5272, Some("atelier")),
            "http://10.0.0.4:5272"
        );
        let v6: IpAddr = "fd00::4".parse().unwrap();
        assert_eq!(
            default_public_url(v6, 5272, Some("atelier")),
            "http://[fd00::4]:5272"
        );
        // Every default is a Public URL as the setting stores one.
        let url = default_public_url(v6, 5272, None);
        assert_eq!(normalize(&url).unwrap().as_deref(), Some(url.as_str()));
    }

    #[test]
    fn links_are_built_under_the_base() {
        assert_eq!(page_url("https://h/", "demo"), "https://h/pages/demo/");
        assert_eq!(
            guided_page_url("https://h", "demo"),
            "https://h/#/pages/demo"
        );
    }
}
