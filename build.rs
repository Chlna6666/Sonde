use std::{env, fs, path::Path, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=web/src");
    println!("cargo:rerun-if-changed=web/index.html");
    println!("cargo:rerun-if-changed=web/package.json");
    println!("cargo:rerun-if-changed=web/pnpm-lock.yaml");
    println!("cargo:rerun-if-env-changed=SONDE_BUILD_WEB");
    println!("cargo:rerun-if-env-changed=SONDE_SKIP_WEB_BUILD");
    println!("cargo:rerun-if-env-changed=SONDE_DEV_PROXY");

    let profile = env::var("PROFILE").unwrap_or_default();
    let force_build = env::var_os("SONDE_BUILD_WEB").is_some();
    let skip_build = env::var_os("SONDE_SKIP_WEB_BUILD").is_some();
    let dev_proxy = env::var_os("SONDE_DEV_PROXY").is_some();

    if skip_build || dev_proxy || (!force_build && profile != "release") {
        let dist = Path::new("web/dist");
        if !dist.join("index.html").exists() {
            let _ = fs::create_dir_all(dist);
            let _ = fs::write(
                dist.join("index.html"),
                "<!DOCTYPE html><html><head><title>Sonde</title></head><body><div id=\"root\"></div></body></html>",
            );
        }
        return;
    }

    let web = Path::new("web");
    if !web.join("package.json").exists() {
        return;
    }

    if !web.join("node_modules").exists() {
        exit_on_error(run_pnpm(web, &["install", "--frozen-lockfile"]));
    }
    exit_on_error(run_pnpm(web, &["build"]));
}

fn run_pnpm(directory: &Path, arguments: &[&str]) -> Result<(), String> {
    let program = if cfg!(windows) { "pnpm.cmd" } else { "pnpm" };
    let status = Command::new(program)
        .args(arguments)
        .current_dir(directory)
        .status()
        .map_err(|error| format!("failed to start pnpm: {error}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("pnpm {} failed", arguments.join(" ")))
}

fn exit_on_error(result: Result<(), String>) {
    if let Err(error) = result {
        println!("cargo:error={error}");
        std::process::exit(1);
    }
}
