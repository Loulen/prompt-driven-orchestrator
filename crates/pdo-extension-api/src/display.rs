use serde::{Deserialize, Serialize};

/// How an Actor shows to a person: a name and an avatar, rendered by
/// [`Extension::display_name`](crate::Extension::display_name). The core shows
/// it wherever it shows an Actor (the audit journal, the Runs), and hands it
/// to every extension ([`Host::display_name`](crate::Host::display_name), and
/// `useActorDisplay` of `@pdo/extension-ui`): « Alice », without knowing the
/// extension that identified her.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[non_exhaustive]
pub struct ActorDisplay {
    pub name: String,
    /// An image URL (often one of the extension's routes); `null` draws the
    /// initials of `name`.
    #[serde(default)]
    pub avatar_url: Option<String>,
}

impl ActorDisplay {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            avatar_url: None,
        }
    }

    pub fn with_avatar(mut self, url: impl Into<String>) -> Self {
        self.avatar_url = Some(url.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_its_name_and_avatar() {
        assert_eq!(
            serde_json::to_value(ActorDisplay::new("Alice").with_avatar("/ext/iam/a.png")).unwrap(),
            serde_json::json!({ "name": "Alice", "avatar_url": "/ext/iam/a.png" })
        );
        assert_eq!(
            serde_json::to_value(ActorDisplay::new("Bob")).unwrap(),
            serde_json::json!({ "name": "Bob", "avatar_url": null })
        );
    }
}
