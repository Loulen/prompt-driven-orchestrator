//! The instance's « Public URL »: the address at which humans open PDO (a VPN
//! name, a LAN address, a reverse proxy). Agents receive it in their
//! environment and preamble, and `pdo page mount` prints its links with it.
//! Unset, nothing changes: links stay on the daemon's own address.
//!
//! The precedence is the usual `stored → env → none` (ADR-0015); the stored
//! tier is kept by the settings context, the run and platform contexts read it
//! through their ports and resolve it here.

/// The environment tier of the setting, and the variable a node session
/// receives it in.
pub const PUBLIC_URL_ENV: &str = "PDO_PUBLIC_URL";

/// The canonical form of a Public URL: `http(s)://host[:port][/path]`, without
/// a trailing `/`, a query or a fragment. `Ok(None)` for a blank value (the
/// clear); `Err` with the reason otherwise.
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
    let path = rest[host.len()..].trim_end_matches('/');
    Ok(Some(format!(
        "{}://{host}{path}",
        scheme.to_ascii_lowercase()
    )))
}

/// The environment tier: [`PUBLIC_URL_ENV`], normalized; an invalid value is
/// ignored with a warning rather than handed to the agents.
pub fn env_public_url() -> Option<String> {
    let raw = std::env::var(PUBLIC_URL_ENV).ok()?;
    match normalize(&raw) {
        Ok(url) => url,
        Err(why) => {
            tracing::warn!("{PUBLIC_URL_ENV} ignored: {why}");
            None
        }
    }
}

/// `stored → env → none`. A stored value that no longer normalizes is skipped.
pub fn resolve(stored: Option<&str>, env: Option<String>) -> Option<String> {
    stored
        .and_then(|raw| normalize(raw).ok().flatten())
        .or(env)
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
            normalize("HTTP://10.0.0.4:5272/pdo//").unwrap(),
            Some("http://10.0.0.4:5272/pdo".into())
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
        ] {
            let error = normalize(raw).expect_err(raw);
            assert!(error.contains("public_url"), "{error}");
        }
    }

    #[test]
    fn the_stored_tier_wins_over_the_environment() {
        let env = Some("http://env:1".to_string());
        assert_eq!(
            resolve(Some("https://stored/"), env.clone()).as_deref(),
            Some("https://stored")
        );
        assert_eq!(resolve(None, env.clone()), env);
        assert_eq!(resolve(Some("not a url"), env.clone()), env);
        assert_eq!(resolve(None, None), None);
    }

    #[test]
    fn links_are_built_under_the_base() {
        assert_eq!(page_url("https://h/", "demo"), "https://h/pages/demo/");
        assert_eq!(guided_page_url("https://h", "demo"), "https://h/#/pages/demo");
    }
}
