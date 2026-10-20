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

#[cfg(test)]
mod tests {
    use super::shown_remote;

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
