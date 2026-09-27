//! The names the registry already uses, read two ways: as the list shows them
//! (a file that does not parse shows its stem), and leniently off the raw
//! `name:` line (what identity allocation compares).

use std::collections::HashSet;

use super::ports::RegistryFile;
use crate::domain::pipeline::parse_pipeline;

/// The visible name of a registry file, as the list shows it.
pub(crate) fn listed_name(file: &RegistryFile) -> String {
    file.yaml
        .as_deref()
        .and_then(|yaml| parse_pipeline(yaml).ok())
        .map(|parsed| parsed.pipeline.name)
        .unwrap_or_else(|| file.id.clone())
}

/// The trimmed top-level `name:` of a YAML document, read as plain YAML (not
/// the pipeline schema): `None` when there is no string `name:`.
pub(crate) fn lenient_name(yaml: &str) -> Option<String> {
    let value = serde_yaml::from_str::<serde_yaml::Value>(yaml).ok()?;
    value
        .as_mapping()?
        .get(serde_yaml::Value::String("name".into()))?
        .as_str()
        .map(|name| name.trim().to_string())
}

/// Every name the registry's files carry on their `name:` line.
pub(crate) fn lenient_names(files: &[RegistryFile]) -> HashSet<String> {
    files
        .iter()
        .filter_map(|file| file.yaml.as_deref())
        .filter_map(lenient_name)
        .collect()
}
