fn main() {
    tauri_build::build();
    embed_windows_test_manifest();
}

/// The application binary gets an embedded manifest from `tauri-build`
/// (which includes the Common-Controls v6 assembly binding). Integration
/// test binaries do not — and GUI objects linked into them import v6-only
/// APIs such as `TaskDialogIndirect`, making the test binary fail to start
/// with `STATUS_ENTRYPOINT_NOT_FOUND` (0xC0000139) unless the v6 binding is
/// declared. Embed a matching manifest into test targets here.
fn embed_windows_test_manifest() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" || target_env != "msvc" {
        return;
    }

    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("windows-test.manifest");
    if manifest.is_file() {
        println!("cargo::rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo::rustc-link-arg-tests=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
    println!("cargo:rerun-if-changed=tests/windows-test.manifest");
}
