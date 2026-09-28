//! Puts the SPA that `src/spa.rs` embeds into `$OUT_DIR/frontend-dist` (#21).
//!
//! Three sources, first match wins:
//!
//! 1. `PDO_FRONTEND_DIST=<absolute path>`: an already built UI (a Vite `dist/`) is
//!    copied as is. No Node, no pnpm. This is the way for a binary that depends on
//!    this crate by git tag (ADR-0081): cargo's checkout has no `node_modules`.
//! 2. `PDO_SKIP_FRONTEND_BUILD` (any value): nothing is built, whatever
//!    `$OUT_DIR/frontend-dist` already holds is embedded (created empty if absent,
//!    which serves the dev placeholder in debug). For loops that only compile the
//!    Rust (`make types`, the e2e run after a full build).
//! 3. Otherwise `pnpm run build` in `frontend/` (`tsc -b && vite build`), Vite
//!    writing to `$OUT_DIR/frontend-dist`. It needs `frontend/node_modules`: when
//!    it is missing, the build fails here and names the fix, instead of `tsc`
//!    failing on TS2318.
//!
//! The script never writes into the source tree itself; the only thing the pnpm
//! path leaves there is tsc's incremental cache, inside the `node_modules` the
//! developer installed (`tsBuildInfoFile` in the frontend's tsconfigs).

use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// The frontend inputs whose change re-runs `pnpm run build`. Relative to crates/pdo/.
const FRONTEND_INPUTS: [&str; 9] = [
    "../../frontend/src",
    "../../frontend/index.html",
    "../../frontend/package.json",
    "../../frontend/pnpm-lock.yaml",
    "../../frontend/vite.config.ts",
    "../../frontend/tsconfig.json",
    "../../frontend/tsconfig.app.json",
    "../../frontend/tsconfig.node.json",
    "../../frontend/components.json",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PDO_FRONTEND_DIST");
    println!("cargo:rerun-if-env-changed=PDO_SKIP_FRONTEND_BUILD");

    let manifest_dir = PathBuf::from(env("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(env("OUT_DIR")).join("frontend-dist");

    match std::env::var_os("PDO_FRONTEND_DIST").filter(|v| !v.is_empty()) {
        Some(dist) => copy_prebuilt(Path::new(&dist), &out),
        None if std::env::var_os("PDO_SKIP_FRONTEND_BUILD").is_some() => {
            std::fs::create_dir_all(&out)
                .unwrap_or_else(|e| panic!("build.rs: cannot create {}: {e}", out.display()));
            println!(
                "cargo:warning=PDO_SKIP_FRONTEND_BUILD set; embedding the frontend already in {} \
                 (empty on a fresh build: the UI is then missing)",
                out.display()
            );
        }
        None => {
            let frontend = manifest_dir.join("../../frontend");
            build_with_pnpm(&frontend.canonicalize().unwrap_or(frontend), &out)
        }
    }

    export_embed_dir(&manifest_dir, &out);
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("build.rs: cargo did not set {name}"))
}

/// Source 1: copy a prebuilt `dist/` into `out`, replacing what was there.
fn copy_prebuilt(dist: &Path, out: &Path) {
    // A build script runs in crates/pdo/ of wherever cargo put the crate (a git
    // checkout for a dependency): a relative path would not mean what the caller
    // typed in their shell.
    assert!(
        dist.is_absolute(),
        "PDO_FRONTEND_DIST must be an absolute path, got `{}`. In .cargo/config.toml, \
         `[env] PDO_FRONTEND_DIST = {{ value = \"<path>\", relative = true }}` resolves \
         the path from the directory holding `.cargo/`.",
        dist.display()
    );
    assert!(
        dist.join("index.html").is_file(),
        "PDO_FRONTEND_DIST=`{}` is not a built PDO frontend: no index.html in it. Point it at \
         the `dist/` folder that `pnpm run build` writes in the core's frontend/.",
        dist.display()
    );
    println!("cargo:rerun-if-changed={}", dist.display());

    if out.exists() {
        std::fs::remove_dir_all(out)
            .unwrap_or_else(|e| panic!("build.rs: cannot clear {}: {e}", out.display()));
    }
    copy_dir(dist, out)
        .unwrap_or_else(|e| panic!("build.rs: cannot copy PDO_FRONTEND_DIST into OUT_DIR: {e}"));
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        // `metadata` follows symlinks: a linked file or folder is copied by content.
        if std::fs::metadata(entry.path())?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Source 3: `pnpm run build`, Vite writing to `out` (`PDO_FRONTEND_OUT_DIR`,
/// read by frontend/vite.config.ts).
fn build_with_pnpm(frontend: &Path, out: &Path) {
    for path in FRONTEND_INPUTS {
        // A missing `rerun-if-changed` target produces neither a warning nor an
        // error: cargo silently re-runs this script — and therefore
        // `pnpm run build` — on EVERY cargo build, forever. We fail loudly
        // instead of paying that cost. Paths are relative to crates/pdo/.
        assert!(
            Path::new(path).exists(),
            "build.rs: rerun-if-changed target `{path}` is missing (paths are \
             relative to crates/pdo/). A missing target silently re-runs \
             this script and `pnpm run build` on EVERY cargo build. If the file \
             was moved or renamed, update FRONTEND_INPUTS in build.rs."
        );
        println!("cargo:rerun-if-changed={path}");
    }

    // pnpm's auto-install (`verify-deps-before-run`) would otherwise fill a
    // missing node_modules on some versions and configs, writing ~800 packages
    // into cargo's git checkout; without it, tsc fails on TS2318. Neither is an
    // answer: say what is missing.
    if !frontend.join("node_modules").is_dir() {
        panic!(
            "\n\npdo-daemon embeds its web UI and has no way to get it:\n\
             `{}` is missing and PDO_FRONTEND_DIST is not set.\n\n\
             Either:\n\
             \x20 - point PDO_FRONTEND_DIST at an already built UI (absolute path to a \
             `dist/` folder holding index.html). The way to go when pdo-daemon is a git \
             dependency: see \"Building a binary that embeds PDO\" in the core's README;\n\
             \x20 - or build the UI from source: install Node.js 22+ and pnpm, then run \
             `pnpm install --frozen-lockfile` in {}.\n",
            frontend.join("node_modules").display(),
            frontend.display()
        );
    }

    // pnpm, not npm: every node iteration runs in its own git worktree, and an
    // npm `node_modules` is copied into each one whole (~580 MB each). pnpm
    // keeps a single content-addressed store and hardlinks into each worktree,
    // so N worktrees cost one copy plus links. See frontend/.npmrc for why the
    // hoisted linker.
    let pnpm = if cfg!(windows) { "pnpm.cmd" } else { "pnpm" };

    let status = Command::new(pnpm)
        .args(["run", "build"])
        .env("PDO_FRONTEND_OUT_DIR", out)
        .current_dir(frontend)
        .status()
        .unwrap_or_else(|e| {
            panic!(
                "failed to invoke `{pnpm} run build` in {}: {e}. Install Node.js 22+ and \
                 pnpm, or set PDO_FRONTEND_DIST to an already built UI.",
                frontend.display()
            )
        });

    if !status.success() {
        panic!(
            "`{pnpm} run build` failed with exit code {}",
            status.code().unwrap_or(-1)
        );
    }
}

/// Hands `out` to rust-embed as `PDO_FRONTEND_EMBED_DIR`, relative to the
/// manifest dir: with its `compression` feature, rust-embed (and include-flate
/// under it) only takes a `folder` relative to `CARGO_MANIFEST_DIR`, so
/// `$OUT_DIR/frontend-dist` itself cannot go in `#[folder]`. Both paths are
/// canonicalized first so a `..` never crosses a symlink the wrong way.
fn export_embed_dir(manifest_dir: &Path, out: &Path) {
    let canonical = |p: &Path| {
        p.canonicalize()
            .unwrap_or_else(|e| panic!("build.rs: cannot resolve {}: {e}", p.display()))
    };
    let (base, target) = (canonical(manifest_dir), canonical(out));
    let relative = relative_path(&base, &target).unwrap_or_else(|| {
        panic!(
            "build.rs: no relative path from {} to {} (different roots): rust-embed needs one",
            base.display(),
            target.display()
        )
    });
    let relative = relative
        .to_str()
        .unwrap_or_else(|| panic!("build.rs: {} is not valid UTF-8", relative.display()));
    println!("cargo:rustc-env=PDO_FRONTEND_EMBED_DIR={relative}");
}

/// `target` expressed from `base`, both absolute; `None` when they share no root.
fn relative_path(base: &Path, target: &Path) -> Option<PathBuf> {
    let (base, target): (Vec<_>, Vec<_>) =
        (base.components().collect(), target.components().collect());
    // The root (`/`, or a Windows drive prefix) comes first: no `..` climbs across two.
    if base.first() != target.first() {
        return None;
    }
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    let mut relative: PathBuf =
        std::iter::repeat_n(Component::ParentDir, base.len() - common).collect();
    relative.extend(&target[common..]);
    if relative.as_os_str().is_empty() {
        relative.push(Component::CurDir);
    }
    Some(relative)
}
