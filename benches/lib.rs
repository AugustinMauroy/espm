#![feature(test)]
extern crate test;
extern crate espm;

use test::Bencher;

use espm::specifier::Specifier;

#[bench]
fn bench_specifier_from_string(b: &mut Bencher) {
    let samples = vec![
        "jsr:@scope/name@1.2.3",
        "npm:package@0.1.0",
        "jsr:@otherscope/long-name__with__chars@12.0.0",
        "file:./some/local/path.tgz",
    ];

    b.iter(|| {
        for s in &samples {
            let _ = test::black_box(Specifier::from_string(s));
        }
    });
}

#[bench]
fn bench_package_value_is_esm(b: &mut Bencher) {
    use serde_json::json;
    let v_module = json!({"type": "module"});
    let v_module_field = json!({"module": "index.mjs"});
    let v_exports = json!({"exports": {"./": "./index.mjs"}});
    let v_main_mjs = json!({"main": "index.mjs"});
    let v_common = json!({"main": "index.js"});

    let cases = vec![v_module, v_module_field, v_exports, v_main_mjs, v_common];

    b.iter(|| {
        for c in &cases {
            let _ = test::black_box(espm::installer::package_value_is_esm(c));
        }
    });
}

#[bench]
fn bench_inspect_tgz_bytes_for_esm(b: &mut Bencher) {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use tar::Builder;

    // prepare a small tgz in-memory containing a package.json with "type":"module"
    let tar_buf = {
        let gz = GzEncoder::new(Vec::new(), Compression::default());
        let mut tar = Builder::new(gz);
        let pkg = r#"{"name":"benchpkg","version":"0.0.1","type":"module"}"#;
        let mut header = tar::Header::new_gnu();
        header.set_path("package/package.json").unwrap();
        header.set_size(pkg.as_bytes().len() as u64);
        header.set_cksum();
        tar.append(&header, pkg.as_bytes()).unwrap();
        let gz = tar.into_inner().unwrap();
        gz.finish().unwrap()
    };

    b.iter(|| {
        let _ = test::black_box(
            espm::installer::inspect_tgz_bytes_for_esm(&tar_buf, "bench-tgz").unwrap(),
        );
    });
}

#[bench]
fn bench_pack_current_package(b: &mut Bencher) {
    use std::fs::{create_dir_all, write};
    use std::path::PathBuf;
    // create a temporary directory under system temp
    let mut dir = std::env::temp_dir();
    dir.push("espm_bench_temp");
    let _ = std::fs::remove_dir_all(&dir);
    create_dir_all(&dir).unwrap();

    // write a package.json and some files to include in the tar
    let pkg = r#"{"name":"benchpkg","version":"0.0.1","main":"index.js"}"#;
    write(dir.join("package.json"), pkg).unwrap();
    // create some extra files
    for i in 0..10 {
        let mut p = PathBuf::from(&dir);
        p.push(format!("file{}.txt", i));
        write(p, format!("some content {}", i)).unwrap();
    }

    b.iter(|| {
        let tar = espm::publisher::pack_current_package(&dir).unwrap();
        test::black_box(tar);
    });
}
