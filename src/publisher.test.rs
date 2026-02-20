use super::*;
use crate::logger::Logger;
use flate2::read::GzDecoder;
use std::fs;
use std::io::Cursor;
use tar::Archive;
use tempfile::tempdir;

#[test]
fn pack_current_package_includes_package_contents() {
    let temp = tempdir().expect("create temp dir");
    let dir = temp.path();

    fs::write(
        dir.join("package.json"),
        r#"{"name":"@scope/demo","version":"1.0.0"}"#,
    )
    .expect("write package.json");
    fs::write(dir.join("index.js"), "export default 1;").expect("write index.js");

    let bytes = pack_current_package(dir).expect("pack directory");
    let tar = GzDecoder::new(Cursor::new(bytes));
    let mut archive = Archive::new(tar);

    let mut paths = Vec::new();
    for entry in archive.entries().expect("read entries") {
        let entry = entry.expect("archive entry");
        let path = entry.path().expect("entry path").into_owned();
        paths.push(path.to_string_lossy().to_string());
    }

    assert!(paths.contains(&"package/package.json".to_string()));
    assert!(paths.contains(&"package/index.js".to_string()));
}

#[tokio::test]
async fn debug_logging_does_not_crash() {
    // enable verbose mode and run through publisher helpers; we don't assert on output
    Logger::set_verbose(true);

    let temp = tempdir().expect("create temp dir");
    let dir = temp.path();
    fs::write(dir.join("package.json"), r#"{"name":"pkg"}"#).expect("write package.json");
    fs::write(dir.join("index.js"), "export default 1;").expect("write index.js");

    // exercise pack_current_package and package_name_for_registry
    let _ = pack_current_package(dir).expect("pack");
    let _ = package_name_for_registry(Some("scope"), "name", true).expect("pkg name");

    // dry run publish_from_dir should also hit debug branches
    publish_from_dir(dir, true, true).await.expect("dry run");
}
