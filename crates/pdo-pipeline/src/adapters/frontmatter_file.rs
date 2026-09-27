//! A node output's frontmatter, read off its artifact on disk.

use std::collections::HashMap;
use std::path::Path;

use crate::domain::frontmatter::{parse_frontmatter, FrontmatterError};

/// The frontmatter of the file at `path`; an unreadable file has none.
pub fn parse_frontmatter_from_file(
    path: &Path,
) -> Result<HashMap<String, serde_yaml::Value>, FrontmatterError> {
    match std::fs::read_to_string(path) {
        Ok(content) => parse_frontmatter(&content),
        Err(_) => Ok(HashMap::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonexistent_file_returns_empty() {
        let fields =
            parse_frontmatter_from_file(Path::new("/nonexistent/path/to/file.md")).unwrap();
        assert!(fields.is_empty());
    }

    #[test]
    fn reads_the_frontmatter_of_a_real_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("output.md");
        std::fs::write(&path, "---\nverdict: PASS\n---\n\nBody").unwrap();
        let fields = parse_frontmatter_from_file(&path).unwrap();
        assert_eq!(fields["verdict"], serde_yaml::Value::String("PASS".into()));
    }
}
