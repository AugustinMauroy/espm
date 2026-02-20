mod common;

use tempfile::tempdir;

use common::{assert_success, run_espm};

#[test]
fn e2e_init_creates_espm_json() {
    let temp = tempdir().expect("create temp dir");
    let output = run_espm(temp.path(), &["init"]);
    assert_success(&output, "espm init");
    assert!(temp.path().join("espm.json").exists());
}
