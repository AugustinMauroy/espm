mod common;

use std::fs;
use tempfile::tempdir;

use common::{assert_success, run_espm, write_local_package_dir};

#[test]
fn e2e_remove_deletes_config_and_installed_artifacts() {
    let temp = tempdir().expect("create temp dir");
    write_local_package_dir(&temp.path().join("remove-me"), "remove-me", "1.0.0");

    let add_output = run_espm(temp.path(), &["add", "file:./remove-me"]);
    assert_success(&add_output, "espm add file for remove test");

    let remove_output = run_espm(temp.path(), &["remove", "remove-me"]);
    assert_success(&remove_output, "espm remove");

    let espm: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(temp.path().join("espm.json")).expect("read espm.json"),
    )
    .expect("parse espm.json");
    let has_dep = espm
        .get("import_map")
        .and_then(|value| value.get("imports"))
        .and_then(|value| value.get("remove-me"))
        .is_some();
    assert!(!has_dep, "dependency still present in espm.json after remove");
    assert!(!temp.path().join("node_modules/remove-me").exists());
}
