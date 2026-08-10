use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PDO_SKIP_FRONTEND_BUILD");

    for path in [
        "../../frontend/src",
        "../../frontend/index.html",
        "../../frontend/package.json",
        "../../frontend/package-lock.json",
        "../../frontend/vite.config.ts",
        "../../frontend/tsconfig.json",
        "../../frontend/tsconfig.app.json",
        "../../frontend/tsconfig.node.json",
        "../../frontend/components.json",
    ] {
        // A missing `rerun-if-changed` target produces neither a warning nor an
        // error: cargo silently re-runs this script — and therefore
        // `npm run build` — on EVERY cargo build, forever. We fail loudly
        // instead of paying that cost. Paths are relative to crates/pdo-daemon/.
        assert!(
            std::path::Path::new(path).exists(),
            "build.rs: rerun-if-changed target `{path}` is missing (paths are \
             relative to crates/pdo-daemon/). A missing target silently re-runs \
             this script and `npm run build` on EVERY cargo build. If the file \
             was moved or renamed, update the path list in build.rs."
        );
        println!("cargo:rerun-if-changed={path}");
    }

    // `../../frontend/dist` is generated (gitignored), created by this script,
    // and absent on a fresh clone — so it stays out of the assert above.
    // rust_embed already fails the compile downstream if dist is missing.
    println!("cargo:rerun-if-changed=../../frontend/dist");

    if std::env::var_os("PDO_SKIP_FRONTEND_BUILD").is_some() {
        println!("cargo:warning=PDO_SKIP_FRONTEND_BUILD set; assuming frontend/dist is current");
        return;
    }

    let frontend = std::path::Path::new("../../frontend");
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };

    let status = Command::new(npm)
        .args(["run", "build"])
        .current_dir(frontend)
        .status()
        .unwrap_or_else(|e| {
            panic!(
                "failed to invoke `{npm} run build` in {}: {e}. \
                 Install Node.js + run `npm ci` in frontend/, \
                 or set PDO_SKIP_FRONTEND_BUILD=1 if dist is already prepared.",
                frontend.display()
            )
        });

    if !status.success() {
        panic!(
            "`{npm} run build` failed with exit code {}",
            status.code().unwrap_or(-1)
        );
    }
}
