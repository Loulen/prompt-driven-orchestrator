//! Validation of what a user may store in a staging profile: its name, its extra
//! entries, its env and its image source. Pure: every refusal is a message that
//! names the offending value, turned into a 400 by the HTTP adapter.

use crate::domain::container::RUN_CONSTANT_ENV_KEYS;
use crate::domain::image::ProfileImage;
use crate::domain::mode::SandboxMode;

use super::set_owned_paths;

/// Longest accepted profile name. The name is a URL segment, a NOCASE index key, a
/// `<select>` value and an equality token compared across three stores and one
/// immutable event — so it stays short, ASCII and boring.
pub const MAX_PROFILE_NAME_LEN: usize = 32;

/// Grammar: `^[a-z0-9][a-z0-9-]{0,31}$`, trimmed first then checked **strictly**.
///
/// - `off` is **reserved**: it short-circuits resolution before any lookup, so a
///   profile named `off` would be unreachable for life.
/// - `""` is **reserved**: it is the *clear* sentinel of a `PUT /settings`, trigger create
///   and `default_sandbox_with`.
/// - `full` / `minimal` are **allowed** — creating them *is* materialising them.
/// - An uppercase name is a **400, never folded**: accepting `Foo` and storing `foo`
///   would have the UI hunt through a list that displays `foo`. See the deliberate
///   asymmetry documented on [`SandboxMode::parse`].
pub fn validate_profile_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("a profile name cannot be blank".to_string());
    }
    if name.eq_ignore_ascii_case(SandboxMode::OFF_WIRE) {
        return Err("`off` is reserved: it means \"no sandbox\", not a profile".to_string());
    }
    if name.len() > MAX_PROFILE_NAME_LEN {
        return Err(format!(
            "profile name `{name}` is longer than {MAX_PROFILE_NAME_LEN} characters"
        ));
    }
    let mut chars = name.chars();
    let first = chars.next().expect("non-empty checked above");
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err(format!(
            "profile name `{name}` must start with a lowercase letter or a digit"
        ));
    }
    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err(format!(
            "profile name `{name}` may only contain lowercase letters, digits and `-`"
        ));
    }
    Ok(name.to_string())
}

/// Validate and **normalise** one user-authored entry (an *extra*).
///
/// Returns the normalised form — `./foo` → `foo`, `foo/` → `foo`, `foo//bar` →
/// `foo/bar` — which is what makes dedup and the ancestor collapse well defined.
///
/// The insight that lets this be pure, with no `$HOME` in scope: once an absolute path
/// and `..` are refused, a relative path **cannot** leave its root lexically, so
/// knowing `$HOME` would add nothing. Escaping by **symlink** is not decidable here at
/// all (the path may not exist yet; the tree mutates between this write and `prepare`),
/// and the answer already lives where it belongs — the `copy_root` deref of
/// `copy_tree_preserving`.
pub fn validate_entry(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("an entry cannot be blank".to_string());
    }
    if trimmed.contains('\0') {
        return Err("an entry cannot contain a NUL byte".to_string());
    }
    // A backslash is a LEGAL filename character under Linux, so `..\..\x` would be ONE
    // component and would sail straight through the `..` test below.
    if trimmed.contains('\\') {
        return Err(format!("`{trimmed}`: an entry cannot contain a backslash"));
    }
    if trimmed.contains(['*', '?', '[']) {
        return Err(format!(
            "`{trimmed}`: patterns are not accepted — pick the file or folder itself"
        ));
    }
    if trimmed.starts_with('/') {
        return Err(format!(
            "`{trimmed}`: an entry is relative to $HOME, not an absolute path"
        ));
    }
    let mut parts: Vec<&str> = Vec::new();
    for seg in trimmed.split('/') {
        match seg {
            "" | "." => continue,
            ".." => return Err(format!("`{trimmed}`: `..` would escape $HOME")),
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        return Err(format!("`{trimmed}`: that is $HOME itself, not an entry"));
    }
    let norm = parts.join("/");

    // Three hard refusals, each of which would otherwise kill EVERY Run of the profile
    // with an opaque `Error response from daemon: Duplicate mount point`, or leave an
    // undeletable staging behind.
    if norm == ".claude" {
        return Err(
            "`.claude`: the staged Claude home is already mounted whole — list the \
             entries you want under it instead"
                .to_string(),
        );
    }
    if norm == ".pdo" || norm.starts_with(".pdo/") {
        return Err(format!(
            "`{norm}`: `.pdo` holds the staging root itself — staging it would copy \
             every other Run's staging into this one"
        ));
    }
    let owned = set_owned_paths();
    if owned.contains(&norm.as_str()) || owned.iter().any(|p| norm.starts_with(&format!("{p}/"))) {
        return Err(format!(
            "`{norm}`: the harness staging set owns that path in every profile — it is \
             guaranteed, not selectable"
        ));
    }
    Ok(norm)
}

/// Longest accepted env key. Not a POSIX limit (there is none) — a guard against a paste
/// accident turning a whole `.env` file into one key.
pub const MAX_ENV_KEY_LEN: usize = 128;

/// Validate and normalise one env **key**.
///
/// Grammar `^[A-Za-z_][A-Za-z0-9_]*$` — the portable-shell subset. Not because `-e K=V`
/// goes through a shell (it does not: it is argv), but because a key outside that subset is
/// unreachable from inside the container by any normal means (`$FOO` in a script, `os.environ`
/// lookups in generated code) — accepting it would store something that silently does nothing.
///
/// The **reserved** keys come from [`RUN_CONSTANT_ENV_KEYS`], not
/// from a second literal here: that module poses them at `docker create`, so it owns the
/// list (the #447 lesson — one fact, one owner). Rejecting them is a **400 that names the
/// key**, never a silent skip: the precedent is the `PDO_DAEMON_URL` guard in the `docker
/// exec` `extra_env` loop, whose comment says why ("a re-passed `-e` would clobber the
/// gateway"). A silent skip would leave the user staring at an editor that shows `HOME` set
/// and a container where it is not.
pub fn validate_env_key(raw: &str) -> Result<String, String> {
    let key = raw.trim();
    if key.is_empty() {
        return Err("an environment variable name cannot be blank".to_string());
    }
    if key.len() > MAX_ENV_KEY_LEN {
        return Err(format!(
            "environment variable name `{key}` is longer than {MAX_ENV_KEY_LEN} characters"
        ));
    }
    let mut chars = key.chars();
    let first = chars.next().expect("non-empty checked above");
    if !first.is_ascii_alphabetic() && first != '_' {
        return Err(format!(
            "`{key}`: an environment variable name must start with a letter or `_`"
        ));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!(
            "`{key}`: an environment variable name may only contain letters, digits and `_`"
        ));
    }
    if let Some(reserved) = RUN_CONSTANT_ENV_KEYS.iter().find(|r| **r == key) {
        return Err(format!(
            "`{reserved}` is set by PDO for every sandboxed Run and cannot be overridden — \
             the container's home, its daemon URL and its Run id all depend on it"
        ));
    }
    Ok(key.to_string())
}

/// Validate one env **value**. Returns it **verbatim**, deliberately un-trimmed.
///
/// Two refusals, both about legibility rather than injection (`-e K=V` is argv, never
/// parsed by a shell):
/// - a **NUL byte** cannot survive `execve`'s environment block at all;
/// - a **newline** would make `docker inspect --format '{{.Config.Env}}'` — the one place
///   an operator can check what the container actually got — unreadable, and would hide a
///   paste accident (a whole `.env` file dropped into one field) behind something that
///   looks like it worked.
///
/// No trim: whitespace can be meaningful in a value (a separator, an indent in a PEM-ish
/// blob), and silently changing what the user typed is worse than carrying their typo. The
/// key is trimmed because a key with spaces is unusable, not merely surprising.
pub fn validate_env_value(key: &str, raw: &str) -> Result<String, String> {
    if raw.contains('\0') {
        return Err(format!(
            "`{key}`: an environment value cannot contain a NUL byte"
        ));
    }
    if raw.contains('\n') || raw.contains('\r') {
        return Err(format!(
            "`{key}`: an environment value cannot span multiple lines — it would make \
             `docker inspect` unreadable and hide a paste accident"
        ));
    }
    Ok(raw.to_string())
}

/// Longest accepted registry ref. Not a Docker limit (a ref is bounded by its components, and
/// 255 per name component at that) — a guard against a paste accident dropping a whole
/// `docker inspect` output into the field.
pub const MAX_IMAGE_REF_LEN: usize = 256;

/// Validate and **normalise** a profile's image source (#467).
///
/// Returns the normalised value — both arms are trimmed — so `  ghcr.io/x:1  ` and `ghcr.io/x:1`
/// cannot become two rows that render identically.
///
/// The refusals, and why each one is a 400 rather than something handled downstream:
///
/// - **`dockerfile`: the path must be absolute.** Same rule, same reason as the `dockerfile_path`
///   setting #471 removed (#431): the daemon's cwd is not the user's, so a relative path resolves
///   against something nobody chose. Existence is checked by the *handler*, not here — this stays
///   pure, and the check has to live where `$HOME` and the filesystem are in scope
///   (necessary-but-not-sufficient: the authoritative `is_file()` bail is `ensure_image`'s, at
///   prep time).
/// - **`registry`: the ref may not start with `-`.** `docker pull -x` parses as a flag. This is
///   the one refusal that is about the *shell-less argv* rather than legibility, and it is why the
///   check cannot be left to Docker: `docker pull` would fail with an unrelated usage error.
/// - **`registry`: printable ASCII only, no whitespace.** A Docker reference is ASCII by
///   specification; a value with a space or a newline is a paste accident that would otherwise
///   fail minutes later, inside a `spawn_blocking`, as an opaque pull error.
///
/// PDO deliberately does **not** try to validate that the ref exists, resolves, or contains
/// `claude`: the first is a network round-trip inside a PUT handler, and the last is the
/// responsibility of whoever supplies the ref (ADR-0030 pt 7, amended by #467).
pub fn validate_profile_image(image: &ProfileImage) -> Result<ProfileImage, String> {
    match image {
        ProfileImage::Dockerfile { path } => {
            let path = path.trim();
            if path.is_empty() {
                return Err(
                    "a Dockerfile path cannot be blank — pick a file, or clear the profile's \
                     image source to fall back to the instance-wide setting"
                        .to_string(),
                );
            }
            if path.contains('\0') {
                return Err("a Dockerfile path cannot contain a NUL byte".to_string());
            }
            if !path.starts_with('/') {
                return Err(format!(
                    "`{path}`: the profile's Dockerfile must be an absolute path"
                ));
            }
            Ok(ProfileImage::Dockerfile {
                path: path.to_string(),
            })
        }
        ProfileImage::Registry { image_ref } => {
            let image_ref = image_ref.trim();
            if image_ref.is_empty() {
                return Err(
                    "an image reference cannot be blank — type one, or clear the profile's \
                     image source to fall back to the instance-wide setting"
                        .to_string(),
                );
            }
            if image_ref.len() > MAX_IMAGE_REF_LEN {
                return Err(format!(
                    "image reference `{image_ref}` is longer than {MAX_IMAGE_REF_LEN} characters"
                ));
            }
            if image_ref.starts_with('-') {
                return Err(format!(
                    "`{image_ref}`: an image reference cannot start with `-` — `docker pull` \
                     would read it as a flag"
                ));
            }
            if !image_ref.chars().all(|c| c.is_ascii_graphic()) {
                return Err(format!(
                    "`{image_ref}`: an image reference may only contain printable ASCII, with no \
                     spaces — a Docker reference is ASCII by specification"
                ));
            }
            Ok(ProfileImage::Registry {
                image_ref: image_ref.to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::profile::{base_entries, FULL_PROFILE};

    #[test]
    fn profile_names_accept_the_grammar() {
        for ok in ["full", "minimal", "full-no-mcp", "a", "0", "x9-y"] {
            assert_eq!(
                validate_profile_name(ok).unwrap(),
                ok,
                "{ok} should be valid"
            );
        }
        assert_eq!(validate_profile_name("  full  ").unwrap(), "full");
    }

    #[test]
    fn profile_names_reject_the_reserved_and_the_malformed() {
        for bad in [
            "",
            "   ",
            "off",
            "OFF",
            "Foo",
            "-lead",
            "under_score",
            "spa ce",
            "é",
            "with.dot",
        ] {
            assert!(
                validate_profile_name(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
        let too_long = "a".repeat(MAX_PROFILE_NAME_LEN + 1);
        assert!(validate_profile_name(&too_long).is_err());
        assert!(validate_profile_name(&"a".repeat(MAX_PROFILE_NAME_LEN)).is_ok());
    }

    #[test]
    fn entries_normalise_to_a_canonical_relative_path() {
        assert_eq!(validate_entry("./foo").unwrap(), "foo");
        assert_eq!(validate_entry("foo/").unwrap(), "foo");
        assert_eq!(validate_entry("foo//bar").unwrap(), "foo/bar");
        assert_eq!(validate_entry("  .gitconfig  ").unwrap(), ".gitconfig");
        assert_eq!(validate_entry(".config/gh").unwrap(), ".config/gh");
        // Sensitive but ALLOWED (ADR-0031 §3) — warned in the UI, not refused.
        assert_eq!(validate_entry(".ssh").unwrap(), ".ssh");
    }

    #[test]
    fn entries_reject_escapes_and_reserved_paths() {
        for bad in [
            "",
            "   ",
            ".",
            "./",
            "/etc/passwd",
            "../x",
            "foo/../../x",
            // A backslash is a legal Linux filename char, so this would otherwise be
            // ONE component and pass the `..` test.
            "..\\..\\x",
            ".claude/*.md",
            "foo?",
            ".claude",
            ".pdo",
            ".pdo/sandbox",
            ".claude/projects",
            ".claude/projects/-enc/x.jsonl",
            ".claude/.credentials.json",
            ".claude/remote-settings.json",
        ] {
            assert!(validate_entry(bad).is_err(), "{bad:?} should be rejected");
        }
        assert!(validate_entry("a\0b").is_err());
    }

    /// The default's glob is authored HERE, never by a user — which is why
    /// `validate_entry` may refuse `*` while the resolved list still carries `*.md`.
    #[test]
    fn the_default_glob_is_not_user_authorable() {
        assert!(base_entries(FULL_PROFILE).contains(&".claude/*.md"));
        assert!(validate_entry(".claude/*.md").is_err());
    }

    #[test]
    fn env_keys_accept_the_portable_shell_subset() {
        for ok in [
            "FOO",
            "_FOO",
            "PUPPETEER_EXECUTABLE_PATH",
            "http_proxy",
            "X9",
            "_",
        ] {
            assert_eq!(validate_env_key(ok).unwrap(), ok, "{ok} should be valid");
        }
        assert_eq!(validate_env_key("  FOO  ").unwrap(), "FOO");
    }

    #[test]
    fn env_keys_reject_the_malformed() {
        for bad in [
            "", "   ", "9FOO", "FOO-BAR", "FOO BAR", "FOO=BAR", "FOÔ", "a.b",
        ] {
            assert!(validate_env_key(bad).is_err(), "{bad:?} should be rejected");
        }
        assert!(validate_env_key(&"A".repeat(MAX_ENV_KEY_LEN + 1)).is_err());
        assert!(validate_env_key(&"A".repeat(MAX_ENV_KEY_LEN)).is_ok());
    }

    /// AC2 of #468. The refusal is a *named* one: the message must carry the offending key,
    /// because the whole point of preferring a 400 to a silent skip is that the user learns
    /// WHICH variable PDO owns. A message that merely says "reserved" would send them back
    /// to the source.
    #[test]
    fn env_keys_reject_the_run_constants_by_name() {
        for reserved in RUN_CONSTANT_ENV_KEYS {
            let err = validate_env_key(reserved)
                .expect_err("a run-constant key must be refused, never skipped");
            assert!(
                err.contains(reserved),
                "the 400 must name the offending key, got: {err}"
            );
        }
        // Whitespace and the trim must not sneak one past the check.
        assert!(validate_env_key("  HOME  ").is_err());
        // Case matters: env vars are case-sensitive, so `home` is a different (allowed) var.
        assert!(validate_env_key("home").is_ok());
    }

    #[test]
    fn env_values_reject_nul_and_newlines_and_keep_everything_else_verbatim() {
        // Verbatim, including surrounding whitespace: silently trimming would change a
        // value the user may have meant, and a value is not a path.
        assert_eq!(validate_env_value("K", "  spaced  ").unwrap(), "  spaced  ");
        assert_eq!(validate_env_value("K", "").unwrap(), "");
        assert_eq!(
            validate_env_value("K", "a=b:c/d spaces & $dollars").unwrap(),
            "a=b:c/d spaces & $dollars"
        );
        for bad in ["a\0b", "a\nb", "a\r\nb", "trailing\n"] {
            assert!(
                validate_env_value("K", bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
        // The refusal names the key, like every other one here.
        assert!(validate_env_value("TOKEN", "a\nb")
            .unwrap_err()
            .contains("TOKEN"));
    }

    #[test]
    fn a_profile_dockerfile_must_be_an_absolute_path_and_is_trimmed() {
        let df = |p: &str| ProfileImage::Dockerfile {
            path: p.to_string(),
        };
        assert_eq!(
            validate_profile_image(&df("  /repo/Dockerfile.chrome-dev  ")).unwrap(),
            df("/repo/Dockerfile.chrome-dev"),
            "trimmed, so two rows cannot render identically"
        );
        for bad in [
            "",
            "   ",
            "docker/Dockerfile",
            "./Dockerfile",
            "~/Dockerfile",
        ] {
            assert!(
                validate_profile_image(&df(bad)).is_err(),
                "{bad:?} should be rejected"
            );
        }
        assert!(validate_profile_image(&df("/a\0b")).is_err());
        // The refusal NAMES the offending value, like every other one in this module.
        assert!(validate_profile_image(&df("relative/Dockerfile"))
            .unwrap_err()
            .contains("relative/Dockerfile"));
    }

    #[test]
    fn a_profile_registry_ref_rejects_the_unpullable() {
        let reg = |r: &str| ProfileImage::Registry {
            image_ref: r.to_string(),
        };
        for ok in [
            "ghcr.io/acme/agent:1.4",
            "acme/agent:latest",
            "agent",
            "registry.example.com:5000/team/agent@sha256:abc",
            "ghcr.io/loulen/pdo-sandbox-chrome-dev:h-0123456789ab",
        ] {
            assert_eq!(
                validate_profile_image(&reg(ok)).unwrap(),
                reg(ok),
                "{ok} should be valid"
            );
        }
        assert_eq!(
            validate_profile_image(&reg("  acme/agent:1  ")).unwrap(),
            reg("acme/agent:1")
        );
        // A leading `-` is the one refusal that is about argv rather than legibility: Docker
        // would read it as a flag and fail with an unrelated usage error.
        let err = validate_profile_image(&reg("-rm")).unwrap_err();
        assert!(err.contains("-rm") && err.contains("flag"), "{err}");
        for bad in [
            "",
            "   ",
            "acme/agent :1",
            "acme/agent\n:1",
            "agent\u{e9}",
            "a\0b",
        ] {
            assert!(
                validate_profile_image(&reg(bad)).is_err(),
                "{bad:?} should be rejected"
            );
        }
        assert!(validate_profile_image(&reg(&"a".repeat(MAX_IMAGE_REF_LEN + 1))).is_err());
        assert!(validate_profile_image(&reg(&"a".repeat(MAX_IMAGE_REF_LEN))).is_ok());
    }
}
