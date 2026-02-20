mod common;

use std::fs;
use tempfile::tempdir;

use common::{assert_success, run_espm, write_local_package_dir};

#[test]
fn e2e_install_uses_lockfile_for_file_dependency() {
    let temp = tempdir().expect("create temp dir");
    let valid_pkg_dir = temp.path().join("valid-pkg");
    write_local_package_dir(&valid_pkg_dir, "valid-pkg", "1.0.0");

    let espm = serde_json::json!({
        "name": "demo",
        "import_map": {
            "imports": {
                "valid-pkg": "file:./missing-path"
            }
        }
    });
    fs::write(
        temp.path().join("espm.json"),
        serde_json::to_string_pretty(&espm).expect("serialize espm"),
    )
    .expect("write espm.json");

    let lock = serde_json::json!({
        "version": 1,
        "packages": [
            {
                "id": "file:valid-pkg",
                "source": "file:./valid-pkg",
                "resolved_version": "1.0.0",
                "tarball": valid_pkg_dir.to_string_lossy().to_string(),
                "requested": null,
                "dev": false
            }
        ]
    });
    fs::write(
        temp.path().join("espm-lock.json"),
        serde_json::to_string_pretty(&lock).expect("serialize lock"),
    )
    .expect("write lockfile");

    let output = run_espm(temp.path(), &["install"]);
    assert_success(&output, "espm install");
    assert!(temp.path().join("node_modules/valid-pkg/package.json").exists());
}
