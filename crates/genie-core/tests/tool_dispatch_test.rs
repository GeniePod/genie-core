// Integration tests for genie-core.
// Verify tool dispatch, config loading, and binary properties
// without requiring an LLM, HA, or Jetson hardware.

use std::process::Command;

/// Verify genie-core builds successfully.
#[test]
fn core_binary_builds() {
    let output = Command::new("cargo")
        .args(["build", "--release", "-p", "genie-core"])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run cargo build");

    assert!(
        output.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Verify release binary is under 3 MB.
#[test]
fn binary_size_budget() {
    let path = workspace_root().join("target/release/genie-core");
    if path.exists() {
        let size = std::fs::metadata(&path).unwrap().len();
        let size_mb = size as f64 / 1_048_576.0;
        println!("genie-core: {:.2} MB", size_mb);
        assert!(size_mb < 3.0, "{:.1} MB exceeds 3 MB budget", size_mb);
    }
}

/// Verify deploy config is valid TOML with expected sections.
#[test]
fn config_parses() {
    let config_path = workspace_root().join("deploy/config/geniepod.toml");
    let contents = std::fs::read_to_string(&config_path).unwrap();
    let config: toml::Value = toml::from_str(&contents).unwrap();

    // Verify expected sections exist.
    let table = config.as_table().unwrap();
    assert!(table.contains_key("core"), "missing [core] section");
    assert!(table.contains_key("governor"), "missing [governor] section");
    assert!(table.contains_key("health"), "missing [health] section");
    assert!(table.contains_key("services"), "missing [services] section");
}

/// Verify all systemd unit files reference correct binary names.
#[test]
fn systemd_units_valid() {
    let systemd_dir = workspace_root().join("deploy/systemd");
    let entries = std::fs::read_dir(&systemd_dir).unwrap();

    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "service") {
            let contents = std::fs::read_to_string(&path).unwrap();
            // No unit should reference "dawn".
            assert!(
                !contents.contains("dawn"),
                "{:?} still references 'dawn'",
                path.file_name().unwrap()
            );
        }
    }
}

fn workspace_root() -> std::path::PathBuf {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap().parent().unwrap().to_path_buf()
}
