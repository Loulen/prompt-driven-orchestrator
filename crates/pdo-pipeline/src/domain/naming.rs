//! A pipeline's identity (#774): its visible name and its file stem are 1:1.
//! The stem is the slug of the name; a copy, an import or a duplicate takes
//! the first free pair; a rename splices the `name:` line and nothing else.

use std::collections::HashSet;

/// #774 — the registry file stem a visible pipeline name maps to. One
/// definition shared by identity allocation (create/duplicate/import) and by
/// the rename paths (dedicated endpoint + save-with-changed-name), so "the
/// visible name and the file stem are 1:1" can never drift between surfaces.
pub fn pipeline_slug(requested_name: &str) -> String {
    let slug = requested_name
        .trim()
        .chars()
        .fold(String::new(), |mut slug, c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                slug.push(c.to_ascii_lowercase());
            } else if !slug.ends_with('-') {
                slug.push('-');
            }
            slug
        });
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "pipeline".to_string()
    } else {
        slug.to_string()
    }
}

/// #774 — rewrite ONLY the top-level `name:` scalar of a pipeline YAML, leaving
/// every other byte (comments, unknown keys, formatting) untouched. A rename
/// must never round-trip the document through parse/serialize (#682: that loses
/// comments and unknown keys), so the new name is spliced into the existing
/// text. `None` when the document has no top-level `name:` key — a malformed
/// pipeline the caller must refuse rather than silently rename.
pub fn rewrite_pipeline_name_field(yaml: &str, new_name: &str) -> Option<String> {
    // `serde_yaml` scalar serialization quotes exactly when the value needs it
    // (`my: name` → `'my: name'`), so the spliced line always re-parses.
    let scalar = serde_yaml::to_string(new_name).ok()?.trim_end().to_string();
    let mut replaced = false;
    let mut out = String::with_capacity(yaml.len() + scalar.len() + 8);
    for line in yaml.split_inclusive('\n') {
        if !replaced && line.starts_with("name:") {
            out.push_str("name: ");
            out.push_str(&scalar);
            if line.ends_with('\n') {
                out.push('\n');
            }
            replaced = true;
        } else {
            out.push_str(line);
        }
    }
    if replaced {
        Some(out)
    } else {
        None
    }
}

/// The first free identity for `requested_name`: its slug and the name itself,
/// or `<slug>-N` and `<name> (N)` from 2 on, skipping every stem `stem_taken`
/// reports and every visible name already in `names`. A blank name is
/// "Untitled".
pub fn available_identity(
    requested_name: &str,
    names: &HashSet<String>,
    stem_taken: impl Fn(&str) -> bool,
) -> (String, String) {
    let base_name = requested_name.trim().to_string();
    let base_name = if base_name.is_empty() {
        "Untitled".to_string()
    } else {
        base_name
    };
    let slug = pipeline_slug(&base_name);
    let mut id = slug.to_string();
    let mut name = base_name.clone();
    let mut suffix = 2u32;
    while stem_taken(&id) || names.contains(&name) {
        id = format!("{slug}-{suffix}");
        name = format!("{base_name} ({suffix})");
        suffix += 1;
    }
    (id, name)
}

/// The name of a duplicate of `source_name`: `<base> (copy)`, then
/// `<base> (copy N)`, where `<base>` drops one trailing ` (copy)` /
/// ` (copy N)` so the copy of a copy does not stack them.
pub fn copy_name(source_name: &str, existing_names: &HashSet<String>) -> String {
    let source_name = source_name.trim();
    let base_name = source_name
        .rfind(" (copy")
        .filter(|&index| {
            let tail = &source_name[index..];
            tail == " (copy)"
                || tail
                    .strip_prefix(" (copy ")
                    .and_then(|value| value.strip_suffix(')'))
                    .is_some_and(|value| {
                        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
                    })
        })
        .map(|index| &source_name[..index])
        .unwrap_or(source_name);
    let mut copy_number = 1u32;
    loop {
        let candidate = if copy_number == 1 {
            format!("{base_name} (copy)")
        } else {
            format!("{base_name} (copy {copy_number})")
        };
        if !existing_names.contains(&candidate) {
            return candidate;
        }
        copy_number += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> HashSet<String> {
        list.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn available_identity_avoids_an_existing_name_with_a_different_id() {
        assert_eq!(
            available_identity("Release train", &names(&["Release train"]), |stem| {
                stem == "legacy-id"
            }),
            ("release-train-2".into(), "Release train (2)".into())
        );
    }

    #[test]
    fn available_identity_collapses_name_punctuation_in_the_file_id() {
        assert_eq!(
            available_identity("Release train (2) (copy)", &names(&[]), |_| false),
            (
                "release-train-2-copy".into(),
                "Release train (2) (copy)".into()
            )
        );
    }

    #[test]
    fn available_identity_skips_a_taken_stem_and_names_a_blank_request_untitled() {
        assert_eq!(
            available_identity("  ", &names(&[]), |stem| stem == "untitled"),
            ("untitled-2".into(), "Untitled (2)".into())
        );
    }

    #[test]
    fn the_slug_is_lowercase_ascii_with_single_dashes() {
        assert_eq!(pipeline_slug("  Mixed Case: v2!  "), "mixed-case-v2");
        assert_eq!(pipeline_slug("snake_case"), "snake_case");
        assert_eq!(pipeline_slug("!!!"), "pipeline");
    }

    #[test]
    fn a_copy_does_not_stack_copy_tails() {
        assert_eq!(copy_name("foo", &names(&[])), "foo (copy)");
        assert_eq!(
            copy_name("foo (copy)", &names(&["foo (copy)"])),
            "foo (copy 2)"
        );
        assert_eq!(copy_name("foo (copy 3)", &names(&[])), "foo (copy)");
        assert_eq!(
            copy_name("foo (copyright)", &names(&[])),
            "foo (copyright) (copy)"
        );
    }

    #[test]
    fn a_rename_splices_only_the_top_level_name_line() {
        let yaml = "# keep me\nname: old\nnodes:\n  - id: a\n    name: A\nextra: 1";
        assert_eq!(
            rewrite_pipeline_name_field(yaml, "my: name").as_deref(),
            Some("# keep me\nname: 'my: name'\nnodes:\n  - id: a\n    name: A\nextra: 1")
        );
        assert_eq!(rewrite_pipeline_name_field("nodes: []\n", "x"), None);
    }
}
