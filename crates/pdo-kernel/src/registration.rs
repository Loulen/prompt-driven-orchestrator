//! The checks an extension's manifest goes through when the daemon starts: a
//! valid and unique id, and a version of the extension API this build of PDO
//! implements. A daemon never starts with an extension it cannot honour.

use std::collections::BTreeSet;
use std::fmt;

use pdo_extension_api::{Manifest, API_VERSION, OLDEST_COMPATIBLE_API_VERSION};

/// Why the daemon refused to start with an extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistrationError {
    /// The id is empty or holds something else than lowercase ASCII letters,
    /// digits and `-` (it names a URL segment and a folder).
    InvalidId { id: String },
    /// Two extensions claim the same id, hence the same space.
    DuplicateId { id: String },
    /// The extension requires a version of the API this build does not
    /// implement.
    IncompatibleApi {
        id: String,
        required: String,
        implemented: String,
    },
    /// The required version is not `MAJOR.MINOR[.PATCH]`.
    UnreadableApiVersion { id: String, required: String },
}

impl fmt::Display for RegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistrationError::InvalidId { id } => write!(
                f,
                "extension id `{id}` is invalid: use lowercase ASCII letters, digits and `-`, \
                 starting with a letter or a digit"
            ),
            RegistrationError::DuplicateId { id } => {
                write!(f, "two extensions are registered with the id `{id}`")
            }
            RegistrationError::IncompatibleApi {
                id,
                required,
                implemented,
            } => write!(
                f,
                "extension `{id}` requires extension API {required}, but this PDO implements \
                 extension API {implemented}: rebuild the extension against a compatible \
                 pdo-extension-api"
            ),
            RegistrationError::UnreadableApiVersion { id, required } => write!(
                f,
                "extension `{id}` declares an unreadable extension API version `{required}` \
                 (expected MAJOR.MINOR.PATCH)"
            ),
        }
    }
}

impl std::error::Error for RegistrationError {}

/// Check every manifest, in registration order: the first problem wins.
pub fn check_manifests<'m>(
    manifests: impl IntoIterator<Item = &'m Manifest>,
) -> Result<(), RegistrationError> {
    let mut seen = BTreeSet::new();
    for manifest in manifests {
        check_id(&manifest.id)?;
        if !seen.insert(manifest.id.as_str()) {
            return Err(RegistrationError::DuplicateId {
                id: manifest.id.clone(),
            });
        }
        check_api_version(manifest, API_VERSION, OLDEST_COMPATIBLE_API_VERSION)?;
    }
    Ok(())
}

fn check_id(id: &str) -> Result<(), RegistrationError> {
    let mut chars = id.chars();
    let valid = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if valid {
        Ok(())
    } else {
        Err(RegistrationError::InvalidId { id: id.to_string() })
    }
}

/// Whether `implemented` honours what `manifest` requires: the same major, and
/// at least as recent. From 1.0, any older minor of that major. In 0.x, where a
/// minor may break, nothing older than `oldest`, the oldest version the API
/// declares it still honours (`OLDEST_COMPATIBLE_API_VERSION`).
fn check_api_version(
    manifest: &Manifest,
    implemented: &str,
    oldest: &str,
) -> Result<(), RegistrationError> {
    let unreadable = || RegistrationError::UnreadableApiVersion {
        id: manifest.id.clone(),
        required: manifest.api_version.clone(),
    };
    let required = parse(&manifest.api_version).ok_or_else(unreadable)?;
    let host = parse(implemented).expect("the API's own version is MAJOR.MINOR.PATCH");
    let oldest = parse(oldest).expect("the oldest compatible version is MAJOR.MINOR.PATCH");
    let compatible = if required.0 == 0 {
        host.0 == 0 && oldest <= required && required <= host
    } else {
        host.0 == required.0 && (host.1, host.2) >= (required.1, required.2)
    };
    if compatible {
        Ok(())
    } else {
        Err(RegistrationError::IncompatibleApi {
            id: manifest.id.clone(),
            required: manifest.api_version.clone(),
            implemented: implemented.to_string(),
        })
    }
}

/// `MAJOR.MINOR[.PATCH]`, a missing patch reading as `0`.
fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.trim().split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = match parts.next() {
        Some(patch) => patch.parse().ok()?,
        None => 0,
    };
    parts.next().is_none().then_some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requiring(api: &str) -> Manifest {
        Manifest::new("demo", "Demo", "1.0.0").requiring_api(api)
    }

    fn verdict(required: &str, implemented: &str) -> Result<(), RegistrationError> {
        check_api_version(&requiring(required), implemented, "0.1.0")
    }

    #[test]
    fn the_version_an_extension_was_compiled_against_is_accepted() {
        assert_eq!(
            check_manifests([&Manifest::new("demo", "Demo", "1.0.0")]),
            Ok(())
        );
    }

    #[test]
    fn in_0_x_the_version_must_be_reached_and_not_older_than_the_oldest_honoured() {
        assert!(verdict("0.1.0", "0.1.0").is_ok());
        assert!(verdict("0.1", "0.1.3").is_ok());
        assert!(verdict("0.1.2", "0.1.3").is_ok());
        assert!(verdict("0.1.4", "0.1.3").is_err());
        assert!(verdict("0.2.0", "0.1.3").is_err());
        assert!(verdict("1.0.0", "0.1.0").is_err());
        // 0.2 honours 0.1 (it only added to it), never what 0.1.0 broke before.
        assert!(verdict("0.1.1", "0.2.0").is_ok());
        assert!(verdict("0.2.0", "0.2.0").is_ok());
        assert!(check_api_version(&requiring("0.1.1"), "0.2.0", "0.2.0").is_err());
        assert!(verdict("0.0.9", "0.2.0").is_err());
    }

    #[test]
    fn this_build_honours_an_extension_written_for_0_1() {
        assert_eq!(check_manifests([&requiring("0.1.1")]), Ok(()));
        assert_eq!(check_manifests([&requiring(API_VERSION)]), Ok(()));
    }

    #[test]
    fn from_1_0_the_major_must_match_and_the_minor_be_reached() {
        assert!(verdict("1.0.0", "1.4.0").is_ok());
        assert!(verdict("1.5.0", "1.4.0").is_err());
        assert!(verdict("2.0.0", "1.4.0").is_err());
    }

    #[test]
    fn an_incompatible_api_is_refused_with_a_clear_message() {
        let err = check_manifests([&requiring("9.0.0")]).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "extension `demo` requires extension API 9.0.0, but this PDO implements \
                 extension API {API_VERSION}: rebuild the extension against a compatible \
                 pdo-extension-api"
            )
        );
        assert!(matches!(
            check_manifests([&requiring("latest")]),
            Err(RegistrationError::UnreadableApiVersion { .. })
        ));
    }

    #[test]
    fn ids_are_lowercase_slugs_and_unique() {
        for bad in ["", "Demo", "-demo", "my ext", "demo/x", "démo"] {
            let manifest = Manifest::new(bad, "x", "1.0.0");
            assert!(
                matches!(
                    check_manifests([&manifest]),
                    Err(RegistrationError::InvalidId { .. })
                ),
                "{bad:?}"
            );
        }
        assert!(check_manifests([&Manifest::new("a1-b", "x", "1.0.0")]).is_ok());
        let one = Manifest::new("demo", "One", "1.0.0");
        let two = Manifest::new("demo", "Two", "1.0.0");
        assert_eq!(
            check_manifests([&one, &two]),
            Err(RegistrationError::DuplicateId { id: "demo".into() })
        );
    }
}
