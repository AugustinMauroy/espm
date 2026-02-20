mod common;

use std::process::Command;

use common::{assert_success, run_espm, write_local_package_dir};
use tempfile::tempdir;

#[test]
fn e2e_publish_dry_run_creates_tarball_for_jsr() {
    let temp = tempdir().expect("create temp dir");
    write_local_package_dir(temp.path(), "@scope/demo", "1.0.0");

    let output = run_espm(temp.path(), &["publish", "--dry-run"]);
    assert_success(&output, "espm publish --dry-run jsr");

    assert!(temp.path().join("espm-publish.tgz").exists());
}

#[test]
fn e2e_publish_dry_run_creates_tarball_for_npm() {
    let temp = tempdir().expect("create temp dir");
    write_local_package_dir(temp.path(), "demo-npm", "1.0.0");

    let output = run_espm(temp.path(), &["publish", "--npm", "--dry-run"]);
    assert_success(&output, "espm publish --npm --dry-run");

    assert!(temp.path().join("espm-publish.tgz").exists());
}

#[test]
fn e2e_publish_without_token_fails() {
    let temp = tempdir().expect("create temp dir");
    write_local_package_dir(temp.path(), "@scope/demo", "1.0.0");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_espm"));
    cmd.current_dir(temp.path())
        .arg("publish")
        .env_remove("JSR_TOKEN")
        .env_remove("NPM_TOKEN");
    let output = cmd.output().expect("run espm publish without token");

    assert!(
        !output.status.success(),
        "publish should fail without tokens"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("JSR_TOKEN"), "stderr was: {}", stderr);
}
