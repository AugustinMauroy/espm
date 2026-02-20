mod common;

use std::fs;
use std::net::TcpStream;
use std::sync::atomic::Ordering;
use tempfile::tempdir;

use common::{
    assert_success, create_tgz_package_bytes, run_espm, start_tgz_http_server,
    write_local_package_dir,
};

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
    assert!(temp
        .path()
        .join("node_modules/valid-pkg/package.json")
        .exists());
}

// ensure that the lockfile is respected even when the import map has a different URL
#[test]
fn e2e_install_uses_lockfile_for_http_dependency() {
    let temp = tempdir().expect("create temp dir");
    let body = create_tgz_package_bytes("remote-pkg", "1.2.3");
    let (addr, running, handle) = start_tgz_http_server(body);

    let espm = serde_json::json!({
        "name": "demo",
        "import_map": { "imports": { "remote-pkg": "http://example.com/bad.tgz" } }
    });
    fs::write(
        temp.path().join("espm.json"),
        serde_json::to_string_pretty(&espm).expect("serialize espm"),
    )
    .expect("write espm.json");

    let url = format!("http://{}/remote-pkg.tgz", addr);
    let lock = serde_json::json!({
        "version": 1,
        "packages": [
            {
                "id": "http:remote-pkg",
                "source": "http://example.com/bad.tgz",
                "resolved_version": "1.2.3",
                "tarball": url,
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

    running.store(false, Ordering::Relaxed);
    let _ = TcpStream::connect(addr);
    handle.join().expect("join server thread");

    assert_success(&output, "espm install http");
    assert!(temp
        .path()
        .join("node_modules/remote-pkg/package.json")
        .exists());
}

#[test]
fn e2e_install_uses_lockfile_for_npm_dependency() {
    let temp = tempdir().expect("create temp dir");
    let body = create_tgz_package_bytes("npm-pkg", "2.3.4");
    let (addr, running, handle) = start_tgz_http_server(body);

    let espm = serde_json::json!({
        "name": "demo",
        "import_map": { "imports": { "npm-pkg": "npm:npm-pkg@2.3.4" } }
    });
    fs::write(
        temp.path().join("espm.json"),
        serde_json::to_string_pretty(&espm).expect("serialize espm"),
    )
    .expect("write espm.json");

    let url = format!("http://{}/npm-pkg.tgz", addr);
    let lock = serde_json::json!({
        "version": 1,
        "packages": [
            {
                "id": "npm:npm-pkg",
                "source": "npm:npm-pkg@2.3.4",
                "resolved_version": "2.3.4",
                "tarball": url,
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

    running.store(false, Ordering::Relaxed);
    let _ = TcpStream::connect(addr);
    handle.join().expect("join server thread");

    assert_success(&output, "espm install npm");
    assert!(temp
        .path()
        .join("node_modules/npm-pkg/package.json")
        .exists());
}

#[test]
fn e2e_install_uses_lockfile_for_jsr_scoped_dependency() {
    let temp = tempdir().expect("create temp dir");
    let body = create_tgz_package_bytes("pkg", "0.5.0");
    let (addr, running, handle) = start_tgz_http_server(body);

    let espm = serde_json::json!({
        "name": "demo",
        "import_map": { "imports": { "@scope/pkg": "jsr:@scope/pkg@0.5.0" } }
    });
    fs::write(
        temp.path().join("espm.json"),
        serde_json::to_string_pretty(&espm).expect("serialize espm"),
    )
    .expect("write espm.json");

    let url = format!("http://{}/pkg.tgz", addr);
    let lock = serde_json::json!({
        "version": 1,
        "packages": [
            {
                "id": "jsr:@scope/pkg",
                "source": "jsr:@scope/pkg@0.5.0",
                "resolved_version": "0.5.0",
                "tarball": url,
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

    running.store(false, Ordering::Relaxed);
    let _ = TcpStream::connect(addr);
    handle.join().expect("join server thread");

    assert_success(&output, "espm install jsr");
    assert!(temp
        .path()
        .join("node_modules/@scope/pkg/package.json")
        .exists());
}

// perform a real network installation against the JSR registry; network failures are tolerated
#[test]
fn e2e_install_real_jsr_dependency() {
    let temp = tempdir().expect("create temp dir");
    let espm = serde_json::json!({
        "name": "demo",
        "import_map": { "imports": { "@am/decisiontree": "jsr:@am/decisiontree@1.0.1" } }
    });
    fs::write(
        temp.path().join("espm.json"),
        serde_json::to_string_pretty(&espm).expect("serialize espm"),
    )
    .expect("write espm.json");

    let output = run_espm(temp.path(), &["install"]);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("Failed") || stderr.contains("network") || stderr.contains("reqwest") {
            eprintln!("network issue during real jsr install; skipping assertion");
            return;
        }
    }

    assert_success(&output, "espm install real jsr package");
    assert!(temp
        .path()
        .join("node_modules/@am/decisiontree/package.json")
        .exists());
}
