//! The fingerprint of a pipeline's content: its YAML and every prompt beside
//! it. A Run records the one of the pipeline it was launched from, so the
//! overwrite warning can say whether the shared pipeline changed since
//! (ADR-0085).

use std::collections::HashMap;

use sha2::{Digest, Sha256};

/// The fingerprint of `yaml` and its `prompts`, in node-id order. Two reads of
/// an unchanged pipeline agree; any edit of the YAML or of one prompt (added,
/// changed, removed) changes it.
pub fn fingerprint(yaml: &str, prompts: &HashMap<String, String>) -> String {
    let mut ids: Vec<&String> = prompts.keys().collect();
    ids.sort();
    let mut hasher = Sha256::new();
    // Length-prefixed fields: no two different contents hash the same bytes.
    let mut field = |bytes: &[u8]| {
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    };
    field(yaml.as_bytes());
    for id in ids {
        field(id.as_bytes());
        field(prompts[id].as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompts(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(id, content)| (id.to_string(), content.to_string()))
            .collect()
    }

    #[test]
    fn an_unchanged_pipeline_reads_the_same_fingerprint() {
        let a = fingerprint("name: p\n", &prompts(&[("w", "v1"), ("r", "v1")]));
        let b = fingerprint("name: p\n", &prompts(&[("r", "v1"), ("w", "v1")]));
        assert_eq!(a, b);
    }

    #[test]
    fn the_yaml_and_each_prompt_move_it() {
        let base = fingerprint("name: p\n", &prompts(&[("w", "v1")]));
        assert_ne!(base, fingerprint("name: q\n", &prompts(&[("w", "v1")])));
        assert_ne!(base, fingerprint("name: p\n", &prompts(&[("w", "v2")])));
        assert_ne!(base, fingerprint("name: p\n", &prompts(&[])));
        assert_ne!(
            base,
            fingerprint("name: p\n", &prompts(&[("w", "v1"), ("x", "")]))
        );
    }

    #[test]
    fn moving_bytes_between_fields_moves_it() {
        assert_ne!(
            fingerprint("a", &prompts(&[("b", "c")])),
            fingerprint("ab", &prompts(&[("", "c")]))
        );
    }
}
