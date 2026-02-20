mod common;

use std::fs;

use tempfile::tempdir;

use common::{assert_success, run_espm, write_local_package_dir};

#[test]
fn e2e_add_file_dependency_refreshes_lockfile() {
    let temp = tempdir().expect("create temp dir");

    let existing_pkg_dir = temp.path().join("existing-pkg");
    write_local_package_dir(&existing_pkg_dir, "existing-pkg", "1.0.0");

    let new_pkg_dir = temp.path().join("new-pkg");
    write_local_package_dir(&new_pkg_dir, "new-pkg", "2.0.0");

    let espm = serde_json::json!({
        "name": "demo",
        "import_map": {
            "imports": {
                "existing-pkg": "file:./existing-pkg"
            }
        }
    });
    fs::write(
        temp.path().join("espm.json"),
        serde_json::to_string_pretty(&espm).expect("serialize espm"),
    )
    .expect("write espm.json");

    let stale_lock = serde_json::json!({
        "version": 1,
        "packages": [
            {
                "id": "file:existing-pkg",
                "source": "file:./existing-pkg",
                "resolved_version": "1.0.0",
                "tarball": existing_pkg_dir.to_string_lossy().to_string(),
                "requested": null,
                "dev": false
            }
        ]
    });
    fs::write(
        temp.path().join("espm-lock.json"),
        serde_json::to_string_pretty(&stale_lock).expect("serialize lock"),
    )
    .expect("write lockfile");

    let output = run_espm(temp.path(), &["add", "file:./new-pkg"]);
    assert_success(&output, "espm add");

    let lock_content = fs::read_to_string(temp.path().join("espm-lock.json")).expect("read lock");
    let lock: serde_json::Value = serde_json::from_str(&lock_content).expect("parse lock");
    let packages = lock
        .get("packages")
        .and_then(|value| value.as_array())
        .expect("lock packages array");
    assert_eq!(packages.len(), 2);

    let has_existing = packages.iter().any(|pkg| {
        pkg.get("id").and_then(|value| value.as_str()) == Some("file:existing-pkg")
            && pkg.get("source").and_then(|value| value.as_str()) == Some("file:./existing-pkg")
    });
    let has_new = packages.iter().any(|pkg| {
        pkg.get("id").and_then(|value| value.as_str()) == Some("file:new-pkg")
            && pkg.get("source").and_then(|value| value.as_str()) == Some("file:./new-pkg")
    });

    assert!(has_existing, "existing package missing in lockfile");
    assert!(has_new, "new package missing in lockfile");

    let espm_content = fs::read_to_string(temp.path().join("espm.json")).expect("read espm");
    let espm_json: serde_json::Value = serde_json::from_str(&espm_content).expect("parse espm");
    assert_eq!(
        espm_json
            .get("import_map")
            .and_then(|value| value.get("imports"))
            .and_then(|value| value.get("new-pkg"))
            .and_then(|value| value.as_str()),
        Some("file:./new-pkg")
    );

    assert!(temp
        .path()
        .join("node_modules/new-pkg/package.json")
        .exists());
}
