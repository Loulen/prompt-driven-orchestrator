//! The remote of a repository, as Settings › Projects shows it under its path.

/// `url` as it may be shown: a token or a password written in its authority
/// (`https://user:token@host/org/app.git`) never reaches the screen. An scp-like
/// remote (`git@host:org/app.git`) names a user, never a secret: kept as is.
pub fn shown_remote(url: &str) -> String {
    let url = url.trim();
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, Some(path)),
        None => (rest, None),
    };
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    match path {
        Some(path) => format!("{scheme}://{host}/{path}"),
        None => format!("{scheme}://{host}"),
    }
}

/// The host `url` reaches, as a failure message names it: `git.example.com`
/// (with its port when the URL gives one) for `https://user:token@git.example.com:8443/acme/app.git`
/// or `git@git.example.com:acme/app.git`; the URL itself, shown, for a local
/// one (`file://`, a path).
pub fn remote_host(url: &str) -> String {
    let url = url.trim();
    if let Some((scheme, rest)) = url.split_once("://") {
        let authority = rest.split('/').next().unwrap_or("");
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        if scheme.eq_ignore_ascii_case("file") || host.is_empty() {
            return shown_remote(url);
        }
        return host.to_string();
    }
    // scp-like: `user@host:path`.
    match url.split_once(':') {
        Some((authority, _)) if authority.contains('@') && !authority.contains('/') => authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host)
            .to_string(),
        _ => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{remote_host, shown_remote};

    #[test]
    fn the_host_is_read_from_every_shape() {
        assert_eq!(
            remote_host("https://me:tok@git.example.com:8443/acme/app.git"),
            "git.example.com:8443"
        );
        assert_eq!(remote_host("ssh://git@host/acme/app.git"), "host");
        assert_eq!(remote_host("git@github.com:acme/app.git"), "github.com");
        assert_eq!(remote_host("file:///srv/app.git"), "file:///srv/app.git");
        assert_eq!(remote_host("/srv/app.git"), "/srv/app.git");
    }

    #[test]
    fn credentials_in_the_authority_are_dropped() {
        assert_eq!(
            shown_remote("https://me:s3cret@github.com/acme/app.git"),
            "https://github.com/acme/app.git"
        );
        assert_eq!(
            shown_remote("https://token@example.com"),
            "https://example.com"
        );
    }

    #[test]
    fn other_remotes_are_shown_as_they_are() {
        assert_eq!(
            shown_remote("git@github.com:acme/app.git"),
            "git@github.com:acme/app.git"
        );
        assert_eq!(
            shown_remote("https://github.com/acme/app.git"),
            "https://github.com/acme/app.git"
        );
        assert_eq!(shown_remote("/srv/git/app.git"), "/srv/git/app.git");
        assert_eq!(
            shown_remote("file:///srv/git/app.git"),
            "file:///srv/git/app.git"
        );
    }
}
