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
        println!("cargo:rerun-if-changed={path}");
    }

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
