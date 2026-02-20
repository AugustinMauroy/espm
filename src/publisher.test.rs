use super::*;
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
