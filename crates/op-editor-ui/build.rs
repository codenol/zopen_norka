//! Stamps the version and build time into the chrome.
//!
//! The build label exists because "am I looking at an old build?" is a real
//! question during a session: the kit manifest is `include_str!`-embedded, so
//! a config change is invisible until the binary is rebuilt. Showing the
//! stamp in the top bar answers it at a glance instead of by guessing.

use std::process::Command;

fn main() {
    // Re-run on every build: the stamp is only useful if it is the truth.
    // A `rerun-if-changed` on a path that does not exist keeps cargo from
    // caching this script's output between builds.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.build-stamp-always-fresh");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let time = Command::new("date")
        .args(["+%Y-%m-%d %H:%M"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| text.trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    let epoch = Command::new("date")
        .args(["+%s"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|text| text.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    let path = std::path::Path::new(&out_dir).join("build_info.rs");
    std::fs::write(
        path,
        format!(
            "/// Crate version + the moment this build was produced.\n\
             pub const VERSION: &str = {version:?};\n\
             pub const BUILD_TIME: &str = {time:?};\n\
             /// Unix seconds — what the freshness colour is computed from.\n\
             pub const BUILD_EPOCH: u64 = {epoch};\n"
        ),
    )
    .expect("writing build_info.rs");
}
