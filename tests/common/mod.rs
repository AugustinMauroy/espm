#![allow(dead_code)]

use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tar::Builder;

pub fn write_local_package_dir(path: &Path, package_name: &str, version: &str) {
    fs::create_dir_all(path).expect("create package dir");
    let package_json = serde_json::json!({
        "name": package_name,
        "version": version
    });
    fs::write(
        path.join("package.json"),
        serde_json::to_string_pretty(&package_json).expect("serialize package json"),
    )
    .expect("write package.json");
    fs::write(path.join("index.js"), "export default 1;").expect("write index.js");
}

pub fn create_tgz_package_bytes(package_name: &str, version: &str) -> Vec<u8> {
    let mut tar_gz = Vec::new();
    {
        let encoder = GzEncoder::new(&mut tar_gz, Compression::default());
        let mut builder = Builder::new(encoder);

        let package_json = serde_json::json!({
            "name": package_name,
            "version": version
        });
        let package_json_bytes = serde_json::to_vec(&package_json).expect("serialize package json");

        let mut header = tar::Header::new_gnu();
        header.set_size(package_json_bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                "package/package.json",
                package_json_bytes.as_slice(),
            )
            .expect("append package.json");

        let index_bytes = b"export default 1;";
        let mut index_header = tar::Header::new_gnu();
        index_header.set_size(index_bytes.len() as u64);
        index_header.set_mode(0o644);
        index_header.set_cksum();
        builder
            .append_data(&mut index_header, "package/index.js", &index_bytes[..])
            .expect("append index.js");

        let encoder = builder.into_inner().expect("finish tar builder");
        encoder.finish().expect("finish gzip encoder");
    }
    tar_gz
}

pub fn run_espm(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_espm"))
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|_| panic!("run espm {:?}", args))
}

pub fn assert_success(output: &std::process::Output, context: &str) {
    assert!(
        output.status.success(),
        "{} failed\nstdout:\n{}\nstderr:\n{}",
        context,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn handle_connection(mut stream: TcpStream, body: &[u8]) {
    let mut buffer = [0u8; 1024];
    let _ = stream.read(&mut buffer);

    let headers = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );

    stream
        .write_all(headers.as_bytes())
        .expect("write http headers");
    stream.write_all(body).expect("write http body");
    stream.flush().expect("flush stream");
}

pub fn start_tgz_http_server(
    body: Vec<u8>,
) -> (SocketAddr, Arc<AtomicBool>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    listener
        .set_nonblocking(true)
        .expect("set nonblocking listener");
    let addr = listener.local_addr().expect("local addr");
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);

    let handle = thread::spawn(move || {
        while running_clone.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => handle_connection(stream, &body),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });

    (addr, running, handle)
}
