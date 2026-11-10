//! `tidy` depends on the standard library alone: a dependency would put its
//! compilation in front of `make fast`, which must answer in under a minute on
//! a cold `target/`.

/// The non-empty lines of every dependency table of a `Cargo.toml`.
fn dependency_lines(manifest: &str) -> Vec<String> {
    let mut in_dependencies = false;
    let mut found = Vec::new();
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            let table = line.trim_matches(['[', ']']);
            in_dependencies = table.ends_with("dependencies") || table.contains("dependencies.");
            if in_dependencies && table.contains("dependencies.") {
                found.push(line.to_string());
            }
            continue;
        }
        if in_dependencies && !line.is_empty() && !line.starts_with('#') {
            found.push(line.to_string());
        }
    }
    found
}

#[test]
fn tidy_depends_on_the_standard_library_only() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("read crates/tidy/Cargo.toml");
    assert_eq!(
        dependency_lines(&manifest),
        Vec::<String>::new(),
        "crates/tidy must stay on the standard library alone"
    );
}

#[test]
fn every_kind_of_dependency_is_seen() {
    let manifest = "[package]\nname = \"tidy\"\n\n[dependencies]\n# none\n\n\
                    [dev-dependencies]\ntempfile = \"3\"\n\
                    [target.'cfg(unix)'.build-dependencies]\ncc = \"1\"\n\
                    [dependencies.serde]\nversion = \"1\"\n";
    assert_eq!(
        dependency_lines(manifest),
        [
            "tempfile = \"3\"",
            "cc = \"1\"",
            "[dependencies.serde]",
            "version = \"1\""
        ]
    );
}
