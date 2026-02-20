mod common;

use std::fs;
use tempfile::tempdir;

use common::{assert_success, run_espm, write_local_package_dir};

#[test]
fn e2e_update_file_dependency_is_intentional_noop() {
    let temp = tempdir().expect("create temp dir");
    write_local_package_dir(&temp.path().join("frozen-pkg"), "frozen-pkg", "1.0.0");

    let add_output = run_espm(temp.path(), &["add", "file:./frozen-pkg"]);
    assert_success(&add_output, "espm add file for update test");

    let before = fs::read_to_string(temp.path().join("espm.json")).expect("read espm before");
    let update_output = run_espm(temp.path(), &["update", "frozen-pkg"]);
    assert_success(&update_output, "espm update file dependency");
    let after = fs::read_to_string(temp.path().join("espm.json")).expect("read espm after");

    assert_eq!(
        before, after,
        "file dependency update should keep config unchanged"
    );

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&update_output.stdout),
        String::from_utf8_lossy(&update_output.stderr)
    );
    assert!(
        combined.contains("not updateable yet") || combined.contains("Skipping"),
        "expected explicit update warning, got:\n{}",
        combined
    );
}
