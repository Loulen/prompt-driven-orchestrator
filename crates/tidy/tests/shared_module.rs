//! The version of the front's shared module `@pdo/extension-ui` (ADR-0081,
//! amendment #32) is the version of `pdo-extension-api`, whose major it is: a
//! bundle built against another major is not loaded. Read from the text of the
//! API's `Cargo.toml`, so the check never compiles the API.

fn read(relative: &str) -> String {
    let path = tidy::workspace_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The `version = "…"` of the `[package]` table of a `Cargo.toml`.
fn package_version(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some(value) = line.strip_prefix("version") else {
            continue;
        };
        let Some(value) = value.trim_start().strip_prefix('=') else {
            continue;
        };
        return value
            .trim()
            .strip_prefix('"')?
            .split('"')
            .next()
            .map(str::to_string);
    }
    None
}

#[test]
fn the_front_s_shared_module_is_versioned_with_the_api() {
    let version = package_version(&read("crates/pdo-extension-api/Cargo.toml"))
        .expect("crates/pdo-extension-api/Cargo.toml declares its own version");
    let major = version.split('.').next().unwrap();
    let front = read("frontend/src/shared/extensions/uiModuleVersion.ts");
    for line in [
        format!("export const UI_MODULE_VERSION = \"{version}\";"),
        format!("export const UI_MODULE_MAJOR = {major};"),
    ] {
        assert!(
            front.lines().any(|l| l == line),
            "frontend/src/shared/extensions/uiModuleVersion.ts must declare `{line}`"
        );
    }
}

#[test]
fn the_package_version_is_read_from_the_package_table_only() {
    let manifest =
        "[package]\nname = \"x\"\nversion = \"0.20.0\"\n\n[dependencies]\nversion = \"9\"\n";
    assert_eq!(package_version(manifest).as_deref(), Some("0.20.0"));
    let workspace =
        "[package]\nversion.workspace = true\n[workspace.package]\nversion = \"2.0.0\"\n";
    assert_eq!(package_version(workspace), None);
}
