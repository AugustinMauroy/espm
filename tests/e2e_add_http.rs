mod common;

use std::fs;
use std::net::TcpStream;
use std::sync::atomic::Ordering;
use tempfile::tempdir;

use common::{assert_success, create_tgz_package_bytes, run_espm, start_tgz_http_server};

#[test]
fn e2e_add_http_installs_and_writes_lockfile() {
    let temp = tempdir().expect("create temp dir");
    let body = create_tgz_package_bytes("remote-pkg", "3.1.4");
    let (addr, running, handle) = start_tgz_http_server(body);

    let url = format!("http://{}/remote-pkg.tgz", addr);
    let output = run_espm(temp.path(), &["add", &url]);

    running.store(false, Ordering::Relaxed);
    let _ = TcpStream::connect(addr);
    handle.join().expect("join server thread");

    assert_success(&output, "espm add http");

    let espm: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(temp.path().join("espm.json")).expect("read espm.json"),
    )
    .expect("parse espm.json");

    assert_eq!(
        espm.get("import_map")
            .and_then(|value| value.get("imports"))
            .and_then(|value| value.get("remote-pkg"))
            .and_then(|value| value.as_str()),
        Some(url.as_str())
    );

    let lock: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(temp.path().join("espm-lock.json")).expect("read lockfile"),
    )
    .expect("parse lockfile");

    let has_remote = lock
        .get("packages")
        .and_then(|value| value.as_array())
        .expect("packages array")
        .iter()
        .any(|pkg| {
            pkg.get("id").and_then(|value| value.as_str()) == Some("http:remote-pkg")
                && pkg.get("tarball").and_then(|value| value.as_str()) == Some(url.as_str())
        });
    assert!(has_remote, "http package missing from lockfile");
    assert!(temp.path().join("node_modules/remote-pkg/package.json").exists());
}
