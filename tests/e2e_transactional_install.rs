mod common;

use std::fs;
use tempfile::tempdir;

use common::run_espm;

#[test]
fn e2e_install_restores_node_modules_on_failure() {
    let temp = tempdir().expect("create temp dir");

    // Create an existing node_modules state that should be preserved on failure
    let existing_pkg_dir = temp.path().join("node_modules/existing-pkg");
    fs::create_dir_all(&existing_pkg_dir).expect("create existing package dir");
    let package_json = serde_json::json!({ "name": "existing-pkg", "version": "0.1.0" });
    fs::write(
        existing_pkg_dir.join("package.json"),
        serde_json::to_string_pretty(&package_json).expect("serialize package json"),
    )
    .expect("write package.json");

    // Create espm.json with a dependency that will fail to download (unused port)
    let espm = serde_json::json!({
        "name": "txn-test",
        "import_map": { "imports": { "broken-pkg": "http://127.0.0.1:9/broken.tgz" } }
    });
    fs::write(
        temp.path().join("espm.json"),
        serde_json::to_string_pretty(&espm).unwrap(),
    )
    .expect("write espm.json");

    // Run `espm install` -- it should fail and restore the original node_modules
    let output = run_espm(temp.path(), &["install"]);

    assert!(
        !output.status.success(),
        "install should fail for broken remote"
    );

    // existing package should still be present (restored)
    assert!(
        temp.path()
            .join("node_modules/existing-pkg/package.json")
            .exists(),
        "existing package should be present after failed install"
    );

    // backup should be removed or not present
    assert!(
        !temp.path().join("node_modules.backup").exists(),
        "node_modules.backup should not remain"
    );
}
