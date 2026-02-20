use super::*;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;
use std::sync::{Arc, OnceLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tar::Builder;
use tempfile::tempdir;

fn cwd_test_lock() -> &'static Arc<Semaphore> {
    static LOCK: OnceLock<Arc<Semaphore>> = OnceLock::new();
    LOCK.get_or_init(|| Arc::new(Semaphore::new(1)))
}

async fn acquire_cwd_lock() -> OwnedSemaphorePermit {
    cwd_test_lock()
        .clone()
        .acquire_owned()
        .await
        .unwrap()
}

struct CwdGuard {
    previous: PathBuf,
}

impl CwdGuard {
    fn enter(path: &Path) -> Result<Self> {
        let previous = env::current_dir().context("Failed to capture current directory")?;
        env::set_current_dir(path)
            .with_context(|| format!("Failed to switch to {}", path.display()))?;
        Ok(Self { previous })
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.previous);
    }
}

fn write_local_package_dir(path: &Path, package_name: &str, version: &str) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("Failed to create package dir {}", path.display()))?;
    let package_json = serde_json::json!({
        "name": package_name,
        "version": version
    });
    fs::write(
        path.join("package.json"),
        serde_json::to_string_pretty(&package_json)?,
    )
    .with_context(|| format!("Failed to write package.json in {}", path.display()))?;
    fs::write(path.join("index.js"), "export default 1;")
        .with_context(|| format!("Failed to write index.js in {}", path.display()))?;
    Ok(())
}

fn write_tgz_package(path: &Path, package_name: &str, version: &str) -> Result<()> {
    let tar_gz = fs::File::create(path)
        .with_context(|| format!("Failed to create tarball {}", path.display()))?;
    let encoder = GzEncoder::new(tar_gz, Compression::default());
    let mut builder = Builder::new(encoder);

    let package_json = serde_json::json!({
        "name": package_name,
        "version": version
    });
    let package_json_bytes = serde_json::to_vec(&package_json)?;
    let mut header = tar::Header::new_gnu();
    header.set_size(package_json_bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(
        &mut header,
        "package/package.json",
        package_json_bytes.as_slice(),
    )?;

    let index_bytes = b"export default 1;";
    let mut index_header = tar::Header::new_gnu();
    index_header.set_size(index_bytes.len() as u64);
    index_header.set_mode(0o644);
    index_header.set_cksum();
    builder.append_data(&mut index_header, "package/index.js", &index_bytes[..])?;

    let mut encoder = builder.into_inner()?;
    encoder.flush()?;
    encoder.finish()?;
    Ok(())
}

#[test]
fn test_specifier_from_string_jsr() {
    let s = "jsr:@scope/pkg@1.2.3";
    let spec = Specifier::from_string(s).unwrap();
    assert_eq!(spec.kind, "jsr");
    assert_eq!(spec.scope.as_deref(), Some("scope"));
    assert_eq!(spec.name.as_deref(), Some("pkg"));
    assert_eq!(spec.version.as_deref(), Some("1.2.3"));
    assert_eq!(spec.path, None);
}

#[test]
fn test_specifier_from_string_jsr_without_version() {
    let s = "jsr:@scope/pkg";
    let spec = Specifier::from_string(s).unwrap();
    assert_eq!(spec.kind, "jsr");
    assert_eq!(spec.scope.as_deref(), Some("scope"));
    assert_eq!(spec.name.as_deref(), Some("pkg"));
    assert_eq!(spec.version, None);
    assert_eq!(spec.path, None);
}

#[test]
fn test_specifier_from_string_npm() {
    let s = "npm:pkg@4.5.6";
    let spec = Specifier::from_string(s).unwrap();
    assert_eq!(spec.kind, "npm");
    assert_eq!(spec.scope, None);
    assert_eq!(spec.name.as_deref(), Some("pkg"));
    assert_eq!(spec.version.as_deref(), Some("4.5.6"));
    assert_eq!(spec.path, None);
}

#[test]
fn test_specifier_from_string_npm_with_scope() {
    let s = "npm:@scope/pkg@7.8.9";
    let spec = Specifier::from_string(s).unwrap();
    assert_eq!(spec.kind, "npm");
    assert_eq!(spec.scope.as_deref(), Some("scope"));
    assert_eq!(spec.name.as_deref(), Some("pkg"));
    assert_eq!(spec.version.as_deref(), Some("7.8.9"));
    assert_eq!(spec.path, None);
}

#[test]
fn test_specifier_from_string_npm_without_version() {
    let s = "npm:pkg";
    let spec = Specifier::from_string(s).unwrap();
    assert_eq!(spec.kind, "npm");
    assert_eq!(spec.scope, None);
    assert_eq!(spec.name.as_deref(), Some("pkg"));
    assert_eq!(spec.version, None);
    assert_eq!(spec.path, None);
}

#[test]
fn test_specifier_from_string_npm_with_scope_without_version() {
    let s = "npm:@scope/pkg";
    let spec = Specifier::from_string(s).unwrap();
    assert_eq!(spec.kind, "npm");
    assert_eq!(spec.scope.as_deref(), Some("scope"));
    assert_eq!(spec.name.as_deref(), Some("pkg"));
    assert_eq!(spec.version, None);
    assert_eq!(spec.path, None);
}

#[test]
fn test_specifier_from_string_file() {
    let s = "file:../local/path";
    let spec = Specifier::from_string(s).unwrap();
    assert_eq!(spec.kind, "file");
    assert_eq!(spec.scope, None);
    assert_eq!(spec.name, None);
    assert_eq!(spec.version, None);
    assert_eq!(spec.path.as_deref(), Some("../local/path"));
}

#[test]
fn test_specifier_from_string_http() {
    let s = "http://example.com/pkg.tgz";
    let spec = Specifier::from_string(s).unwrap();
    assert_eq!(spec.kind, "http");
    assert_eq!(spec.scope, None);
    assert_eq!(spec.name, None);
    assert_eq!(spec.version, None);
    assert_eq!(spec.path, None);
    assert_eq!(spec.source, "http://example.com/pkg.tgz");
    assert_eq!(spec.name, None);
}

#[test]
fn test_specifier_from_string_invalid() {
    let s = "invalidstring";
    let res = Specifier::from_string(s);
    assert!(res.is_err());
}

#[test]
fn test_jsr_package_to_npm_package() {
    let npm_name = jsr_package_to_npm_package("scope", "my-pkg");
    assert_eq!(npm_name, "@jsr/scope__my-pkg");
}

#[test]
fn test_npm_tarball_url_unscoped() {
    let url = npm_tarball_url(None, "lodash", "4.17.21");
    assert_eq!(
        url,
        "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz"
    );
}

#[test]
fn test_npm_tarball_url_scoped() {
    let url = npm_tarball_url(Some("types"), "node", "20.0.0");
    assert_eq!(
        url,
        "https://registry.npmjs.org/@types/node/-/node-20.0.0.tgz"
    );
}

#[test]
fn test_npm_tarball_url_scoped_with_at_prefix() {
    let url = npm_tarball_url(Some("@types"), "node", "20.0.0");
    assert_eq!(
        url,
        "https://registry.npmjs.org/@types/node/-/node-20.0.0.tgz"
    );
}

#[test]
fn test_select_latest_compatible_version_with_caret_req() {
    let versions = vec!["1.0.0", "1.2.0", "2.0.0"]
        .into_iter()
        .map(String::from);
    let selected =
        select_latest_compatible_version(versions, Some("2.0.0"), Some("^1.0.0")).unwrap();
    assert_eq!(selected, "1.2.0");
}

#[test]
fn test_select_latest_compatible_version_uses_latest_tag_without_req() {
    let versions = vec!["1.0.0", "1.2.0", "1.3.0"]
        .into_iter()
        .map(String::from);
    let selected = select_latest_compatible_version(versions, Some("1.2.0"), None).unwrap();
    assert_eq!(selected, "1.2.0");
}

#[test]
fn test_select_latest_compatible_version_fallback_to_max() {
    let versions = vec!["1.0.0", "1.2.0", "1.3.0"]
        .into_iter()
        .map(String::from);
    let selected =
        select_latest_compatible_version(versions, Some("not-semver"), None).unwrap();
    assert_eq!(selected, "1.3.0");
}

#[test]
fn test_parse_npm_dependency_name_unscoped() {
    let parsed = parse_npm_dependency_name("lodash").unwrap();
    assert_eq!(parsed.0, None);
    assert_eq!(parsed.1, "lodash");
}

#[test]
fn test_parse_npm_dependency_name_scoped() {
    let parsed = parse_npm_dependency_name("@types/node").unwrap();
    assert_eq!(parsed.0.as_deref(), Some("types"));
    assert_eq!(parsed.1, "node");
}

#[test]
fn test_parse_npm_dependency_name_invalid() {
    assert!(parse_npm_dependency_name("bad/name").is_none());
}

#[test]
fn test_requested_specifier_from_parts_npm_scoped() {
    let spec = requested_specifier_from_parts("npm", Some("types"), "node", "20.0.0");
    assert_eq!(spec, "npm:@types/node@20.0.0");
}

#[test]
fn test_should_install_locked_package() {
    let package = LockPackage {
        id: "npm:lodash".to_string(),
        source: "npm:lodash@4.17.21".to_string(),
        resolved_version: "4.17.21".to_string(),
        tarball: "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz".to_string(),
        requested: Some("^4.17.0".to_string()),
        dev: true,
    };

    let prod_only = InstallOptions {
        include_dev: false,
        force: false,
    };
    let with_dev = InstallOptions {
        include_dev: true,
        force: false,
    };

    assert!(!should_install_locked_package(prod_only, &package));
    assert!(should_install_locked_package(with_dev, &package));
}

#[test]
fn test_lockfile_paths() {
    let base = Path::new("/tmp/espm-test");
    assert_eq!(
        preferred_lockfile_path(base).to_string_lossy(),
        "/tmp/espm-test/espm-lock.json"
    );
    assert_eq!(
        legacy_lockfile_path(base).to_string_lossy(),
        "/tmp/espm-test/espm-lock.json"
    );
}

#[test]
fn test_package_install_path_unscoped() {
    let path = package_install_path(None, "lodash");
    assert_eq!(path.to_string_lossy(), "./node_modules/lodash");
}

#[test]
fn test_package_install_path_scoped() {
    let path = package_install_path(Some("types"), "node");
    assert_eq!(path.to_string_lossy(), "./node_modules/@types/node");
}

#[test]
fn test_should_skip_reinstall_when_not_installed() {
    assert!(!should_skip_reinstall(
        None,
        "package-that-does-not-exist",
        "1.0.0"
    ));
}

#[test]
fn test_install_options_labels() {
    let prod = InstallOptions {
        include_dev: false,
        force: false,
    };
    let with_dev = InstallOptions {
        include_dev: true,
        force: true,
    };

    assert_eq!(prod.dependency_scope_label(), "production");
    assert_eq!(prod.summary_suffix(), "");
    assert_eq!(with_dev.dependency_scope_label(), "development");
    assert_eq!(with_dev.summary_suffix(), " (prod + dev)");
}

#[test]
fn test_requests_from_import_map_supports_file_http_and_https() {
    let mut imports = HashMap::new();
    imports.insert("local-pkg".to_string(), "file:./pkg".to_string());
    imports.insert(
        "remote-pkg".to_string(),
        "https://example.com/remote.tgz".to_string(),
    );
    imports.insert(
        "remote-http-pkg".to_string(),
        "http://example.com/remote-http.tgz".to_string(),
    );

    let map = ImportMap {
        imports,
        scopes: None,
    };

    let requests = requests_from_import_map(&map, false).unwrap();
    assert_eq!(requests.len(), 3);

    let kinds: Vec<String> = requests.iter().map(|req| req.kind.clone()).collect();
    assert!(kinds.contains(&"file".to_string()));
    assert!(kinds.contains(&"http".to_string()));
    assert!(kinds.contains(&"https".to_string()));
}

#[tokio::test]
async fn test_install_from_lockfile_supports_file_directory_package() {
    let _lock = acquire_cwd_lock().await;
    let temp = tempdir().unwrap();
    let _cwd = CwdGuard::enter(temp.path()).unwrap();

    let local_pkg_dir = temp.path().join("pkg");
    write_local_package_dir(&local_pkg_dir, "local-pkg", "1.2.3").unwrap();

    let lock = EspmLock {
        version: 1,
        packages: vec![LockPackage {
            id: "file:local-pkg".to_string(),
            source: "file:./pkg".to_string(),
            resolved_version: "1.2.3".to_string(),
            tarball: local_pkg_dir.to_string_lossy().to_string(),
            requested: None,
            dev: false,
        }],
    };

    let installed = install_from_lockfile(
        &lock,
        InstallOptions {
            include_dev: false,
            force: true,
        },
        false,
    )
    .await
    .unwrap();

    assert_eq!(installed, 1);
    assert!(temp
        .path()
        .join("node_modules/local-pkg/package.json")
        .exists());
}

#[tokio::test]
async fn test_install_command_writes_file_dependency_into_lockfile() {
    let _lock = acquire_cwd_lock().await;
    let temp = tempdir().unwrap();
    let _cwd = CwdGuard::enter(temp.path()).unwrap();

    let local_pkg_dir = temp.path().join("pkg");
    write_local_package_dir(&local_pkg_dir, "local-pkg", "1.2.3").unwrap();

    let espm = serde_json::json!({
        "name": "demo",
        "import_map": {
            "imports": {
                "local-pkg": "file:./pkg"
            }
        }
    });
    fs::write(
        temp.path().join("espm.json"),
        serde_json::to_string_pretty(&espm).unwrap(),
    )
    .unwrap();

    handle_install_command(false, true, false).await.unwrap();

    let lock_content = fs::read_to_string(temp.path().join("espm-lock.json")).unwrap();
    let lock: EspmLock = serde_json::from_str(&lock_content).unwrap();
    assert_eq!(lock.packages.len(), 1);
    assert_eq!(lock.packages[0].id, "file:local-pkg");
    assert_eq!(lock.packages[0].source, "file:./pkg");
    let expected_path = local_pkg_dir.canonicalize().unwrap();
    let actual_path = PathBuf::from(&lock.packages[0].tarball)
        .canonicalize()
        .unwrap();
    assert_eq!(actual_path, expected_path);
    assert!(temp
        .path()
        .join("node_modules/local-pkg/package.json")
        .exists());
}

#[tokio::test]
async fn test_remove_command_cleans_file_dependency_artifacts() {
    let _lock = acquire_cwd_lock().await;
    let temp = tempdir().unwrap();
    let _cwd = CwdGuard::enter(temp.path()).unwrap();

    let espm = serde_json::json!({
        "name": "demo",
        "import_map": {
            "imports": {
                "local-pkg": "file:./pkg"
            }
        }
    });
    fs::write(
        temp.path().join("espm.json"),
        serde_json::to_string_pretty(&espm).unwrap(),
    )
    .unwrap();

    let installed_dir = temp.path().join("node_modules/local-pkg");
    fs::create_dir_all(&installed_dir).unwrap();
    fs::write(installed_dir.join("package.json"), "{}").unwrap();

    handle_remove_command("local-pkg".to_string())
        .await
        .unwrap();

    assert!(!installed_dir.exists());
    let updated_content = fs::read_to_string(temp.path().join("espm.json")).unwrap();
    let updated: EspmJson = serde_json::from_str(&updated_content).unwrap();
    assert!(updated
        .import_map
        .map(|map| map.imports.is_empty())
        .unwrap_or(true));
}

#[tokio::test]
async fn test_update_keeps_file_dependency_unchanged() {
    let _lock = acquire_cwd_lock().await;
    let temp = tempdir().unwrap();
    let _cwd = CwdGuard::enter(temp.path()).unwrap();

    let espm = serde_json::json!({
        "name": "demo",
        "import_map": {
            "imports": {
                "local-pkg": "file:./pkg"
            }
        }
    });
    fs::write(
        temp.path().join("espm.json"),
        serde_json::to_string_pretty(&espm).unwrap(),
    )
    .unwrap();

    handle_update_command("local-pkg".to_string())
        .await
        .unwrap();

    let updated_content = fs::read_to_string(temp.path().join("espm.json")).unwrap();
    let updated: EspmJson = serde_json::from_str(&updated_content).unwrap();
    assert_eq!(
        updated
            .import_map
            .unwrap()
            .imports
            .get("local-pkg")
            .unwrap(),
        "file:./pkg"
    );
}

#[test]
fn test_package_identity_from_file_tarball() {
    let temp = tempdir().unwrap();
    let tarball_path = temp.path().join("local-pkg.tgz");
    write_tgz_package(&tarball_path, "local-pkg", "9.9.9").unwrap();

    let spec =
        Specifier::from_string(&format!("file:{}", tarball_path.to_string_lossy())).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (scope, name, version) = runtime
        .block_on(package_identity_from_specifier(&spec, temp.path()))
        .unwrap();

    assert!(scope.is_none());
    assert_eq!(name, "local-pkg");
    assert_eq!(version.as_deref(), Some("9.9.9"));
}

#[tokio::test]
async fn test_add_command_refreshes_stale_lockfile() {
    let _lock = acquire_cwd_lock().await;
    let temp = tempdir().unwrap();
    let _cwd = CwdGuard::enter(temp.path()).unwrap();

    let existing_pkg_dir = temp.path().join("existing-pkg");
    write_local_package_dir(&existing_pkg_dir, "existing-pkg", "1.0.0").unwrap();

    let new_pkg_dir = temp.path().join("new-pkg");
    write_local_package_dir(&new_pkg_dir, "new-pkg", "2.0.0").unwrap();

    let espm = serde_json::json!({
        "name": "demo",
        "import_map": {
            "imports": {
                "existing-pkg": "file:./existing-pkg"
            }
        }
    });
    fs::write(
        temp.path().join("espm.json"),
        serde_json::to_string_pretty(&espm).unwrap(),
    )
    .unwrap();

    let stale_lock = EspmLock {
        version: 1,
        packages: vec![LockPackage {
            id: "file:existing-pkg".to_string(),
            source: "file:./existing-pkg".to_string(),
            resolved_version: "1.0.0".to_string(),
            tarball: existing_pkg_dir.to_string_lossy().to_string(),
            requested: None,
            dev: false,
        }],
    };
    fs::write(
        temp.path().join("espm-lock.json"),
        serde_json::to_string_pretty(&stale_lock).unwrap(),
    )
    .unwrap();

    handle_add_command("file:./new-pkg".to_string(), false, false)
        .await
        .unwrap();

    let lock_content = fs::read_to_string(temp.path().join("espm-lock.json")).unwrap();
    let lock: EspmLock = serde_json::from_str(&lock_content).unwrap();
    assert_eq!(lock.packages.len(), 2);
    assert!(lock
        .packages
        .iter()
        .any(|pkg| pkg.id == "file:existing-pkg" && pkg.source == "file:./existing-pkg"));
    assert!(lock
        .packages
        .iter()
        .any(|pkg| pkg.id == "file:new-pkg" && pkg.source == "file:./new-pkg"));

    let updated_content = fs::read_to_string(temp.path().join("espm.json")).unwrap();
    let updated: EspmJson = serde_json::from_str(&updated_content).unwrap();
    assert_eq!(
        updated.import_map.unwrap().imports.get("new-pkg").unwrap(),
        "file:./new-pkg"
    );
}
