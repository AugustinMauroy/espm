use anyhow::{Context, Result};
use colored::Colorize;
use flate2::read::GzDecoder;
use reqwest::Client;
use semver::{Version, VersionReq};
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::env;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use tar::Archive;
use tokio::time::{sleep, Duration};

use crate::jsr_npm::{JsrNpmRegistryResponse, NPMRegistryResponse};
use crate::logger::Logger;
use crate::models::{
    DependencyRequest, EspmJson, EspmLock, ImportMap, InstallOptions, LockPackage, ResolvedPackage,
};
use crate::publisher;
use crate::specifier::{
    jsr_package_to_npm_package, npm_tarball_url, parse_npm_dependency_name,
    requested_specifier_from_parts, Specifier,
};

pub async fn get_espm_json_path() -> Result<std::path::PathBuf> {
    let mut current_dir = env::current_dir().context("Failed to get current directory")?;
    let current_path = current_dir.join("espm.json");
    if current_path.exists() {
        return Ok(current_path);
    }

    while let Some(parent) = current_dir.parent() {
        let path = parent.join("espm.json");
        if path.exists() {
            return Ok(path);
        }
        current_dir = parent.to_path_buf();
    }

    Err(anyhow::anyhow!(
        "espm.json not found in any parent directory"
    ))
}

pub fn package_id(kind: &str, scope: Option<&str>, name: &str) -> String {
    match scope {
        Some(s) => format!("{}:@{}/{}", kind, s.trim_start_matches('@'), name),
        None => format!("{}:{}", kind, name),
    }
}

pub fn package_key(scope: Option<&str>, name: &str) -> String {
    match scope {
        Some(s) => format!("@{}/{}", s.trim_start_matches('@'), name),
        None => name.to_string(),
    }
}

pub fn parse_package_key(input: &str) -> Result<(Option<String>, String)> {
    if let Some((scope, name)) = parse_npm_dependency_name(input) {
        return Ok((scope, name));
    }

    if input.trim().is_empty() {
        return Err(anyhow::anyhow!("Package key cannot be empty"));
    }

    Ok((None, input.to_string()))
}

pub fn parse_lock_package_id(id: &str) -> Result<(String, Option<String>, String)> {
    let (kind, rest) = id
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("Invalid lock package id '{}'", id))?;
    let (scope, name) = parse_package_key(rest)?;
    Ok((kind.to_string(), scope, name))
}

pub fn npm_package_display(scope: Option<&str>, name: &str) -> String {
    match scope {
        Some(s) => format!("@{}/{}", s.trim_start_matches('@'), name),
        None => name.to_string(),
    }
}

pub async fn fetch_json_with_retry<T: DeserializeOwned>(url: &str, attempts: u8) -> Result<T> {
    let client = Client::new();
    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 1..=attempts {
        match client.get(url).send().await {
            Ok(response) => {
                if !response.status().is_success() {
                    last_error = Some(anyhow::anyhow!(
                        "Request failed for {} with status {}",
                        url,
                        response.status()
                    ));
                } else {
                    match response.json::<T>().await {
                        Ok(parsed) => return Ok(parsed),
                        Err(e) => {
                            last_error =
                                Some(anyhow::anyhow!("Failed to parse JSON from {}: {}", url, e));
                        }
                    }
                }
            }
            Err(e) => {
                last_error = Some(anyhow::anyhow!("Request error for {}: {}", url, e));
            }
        }

        if attempt < attempts {
            sleep(Duration::from_millis(250 * attempt as u64)).await;
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown error for {}", url)))
}

pub async fn fetch_bytes_with_retry(url: &str, attempts: u8) -> Result<Vec<u8>> {
    let client = Client::new();
    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 1..=attempts {
        match client.get(url).send().await {
            Ok(response) => {
                if !response.status().is_success() {
                    last_error = Some(anyhow::anyhow!(
                        "Request failed for {} with status {}",
                        url,
                        response.status()
                    ));
                } else {
                    match response.bytes().await {
                        Ok(content) => return Ok(content.to_vec()),
                        Err(e) => {
                            last_error = Some(anyhow::anyhow!(
                                "Failed to read response body from {}: {}",
                                url,
                                e
                            ));
                        }
                    }
                }
            }
            Err(e) => {
                last_error = Some(anyhow::anyhow!("Request error for {}: {}", url, e));
            }
        }

        if attempt < attempts {
            sleep(Duration::from_millis(250 * attempt as u64)).await;
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown error for {}", url)))
}

pub fn package_value_is_esm(package_json: &serde_json::Value) -> bool {
    if let Some(t) = package_json.get("type").and_then(|v| v.as_str()) {
        if t == "module" {
            return true;
        }
    }

    if package_json.get("module").is_some() {
        return true;
    }

    if package_json.get("exports").is_some() {
        return true;
    }

    if let Some(main) = package_json.get("main").and_then(|v| v.as_str()) {
        if main.ends_with(".mjs") {
            return true;
        }
    }

    false
}

pub fn inspect_tgz_bytes_for_esm(bytes: &[u8], source_label: &str) -> Result<bool> {
    let tar = GzDecoder::new(Cursor::new(bytes));
    let mut archive = Archive::new(tar);

    for entry in archive
        .entries()
        .with_context(|| format!("Failed to read entries from {}", source_label))?
    {
        let mut entry = entry?;
        let path = entry.path()?;
        if path.to_string_lossy().ends_with("package.json") {
            let mut content = String::new();
            entry
                .read_to_string(&mut content)
                .with_context(|| format!("Failed to read package.json from {}", source_label))?;
            let package_json: serde_json::Value = serde_json::from_str(&content)
                .with_context(|| format!("Invalid package.json in {}", source_label))?;
            return Ok(package_value_is_esm(&package_json));
        }
    }

    Ok(false)
}

pub fn inspect_dir_for_esm(path: &Path) -> Result<bool> {
    let package_json_path = path.join("package.json");
    if !package_json_path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(&package_json_path)
        .with_context(|| format!("Failed to read {}", package_json_path.display()))?;
    let package_json: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("Invalid {}", package_json_path.display()))?;
    Ok(package_value_is_esm(&package_json))
}

pub fn parse_package_name_field(name: &str) -> Result<(Option<String>, String)> {
    parse_npm_dependency_name(name)
        .ok_or_else(|| anyhow::anyhow!("Invalid package name '{}' in package.json", name))
}

pub fn read_package_manifest_from_dir(
    path: &Path,
) -> Result<(Option<String>, String, Option<String>)> {
    let package_json_path = path.join("package.json");
    let content = fs::read_to_string(&package_json_path)
        .with_context(|| format!("Failed to read {}", package_json_path.display()))?;
    let package_json: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", package_json_path.display()))?;

    let package_name = package_json
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'name' in {}", package_json_path.display()))?;
    let (scope, name) = parse_package_name_field(package_name)?;
    let version = package_json
        .get("version")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    Ok((scope, name, version))
}

pub fn read_package_manifest_from_tgz_bytes(
    bytes: &[u8],
    source_label: &str,
) -> Result<(Option<String>, String, Option<String>)> {
    let tar = GzDecoder::new(Cursor::new(bytes));
    let mut archive = Archive::new(tar);

    for entry in archive
        .entries()
        .with_context(|| format!("Failed to read entries from {}", source_label))?
    {
        let mut entry = entry?;
        let path = entry.path()?;
        if path.to_string_lossy().ends_with("package.json") {
            let mut content = String::new();
            entry
                .read_to_string(&mut content)
                .with_context(|| format!("Failed to read package.json from {}", source_label))?;
            let package_json: serde_json::Value = serde_json::from_str(&content)
                .with_context(|| format!("Invalid package.json in {}", source_label))?;

            let package_name = package_json
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing package name in {}", source_label))?;
            let (scope, name) = parse_package_name_field(package_name)?;
            let version = package_json
                .get("version")
                .and_then(|v| v.as_str())
                .map(str::to_string);

            return Ok((scope, name, version));
        }
    }

    Err(anyhow::anyhow!(
        "Could not find package.json inside tarball from {}",
        source_label
    ))
}

pub fn resolve_file_source_path(base_dir: &Path, raw_path: &str) -> PathBuf {
    let as_path = Path::new(raw_path);
    if as_path.is_absolute() {
        as_path.to_path_buf()
    } else {
        base_dir.join(as_path)
    }
}

pub async fn package_identity_from_specifier(
    specifier: &Specifier,
    base_dir: &Path,
) -> Result<(Option<String>, String, Option<String>)> {
    match specifier.kind.as_str() {
        "jsr" | "npm" => {
            let name = specifier
                .name
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Missing package name in '{}'", specifier.source))?;
            Ok((specifier.scope.clone(), name, specifier.version.clone()))
        }
        "file" => {
            let raw_path = specifier.path.as_deref().ok_or_else(|| {
                anyhow::anyhow!("Missing file path in specifier '{}'", specifier.source)
            })?;
            let resolved_path = resolve_file_source_path(base_dir, raw_path);
            if resolved_path.is_dir() {
                read_package_manifest_from_dir(&resolved_path)
            } else {
                let bytes = fs::read(&resolved_path).with_context(|| {
                    format!(
                        "Failed to read local package file {}",
                        resolved_path.display()
                    )
                })?;
                read_package_manifest_from_tgz_bytes(&bytes, &resolved_path.to_string_lossy())
            }
        }
        "http" | "https" => {
            let bytes = fetch_bytes_with_retry(&specifier.source, 3)
                .await
                .with_context(|| {
                    format!(
                        "Failed to inspect remote package metadata from {}",
                        specifier.source
                    )
                })?;
            read_package_manifest_from_tgz_bytes(&bytes, &specifier.source)
        }
        _ => Err(anyhow::anyhow!(
            "Unsupported specifier kind '{}'",
            specifier.kind
        )),
    }
}

pub fn preferred_lockfile_path(base_dir: &Path) -> std::path::PathBuf {
    base_dir.join("espm-lock.json")
}

pub fn legacy_lockfile_path(base_dir: &Path) -> std::path::PathBuf {
    base_dir.join("espm-lock.json")
}

pub fn write_lockfile(lock: &EspmLock, base_dir: &Path) -> Result<()> {
    let lock_path = preferred_lockfile_path(base_dir);
    fs::write(&lock_path, serde_json::to_string_pretty(lock)?)
        .with_context(|| format!("Failed to write lockfile at {}", lock_path.display()))
}

pub fn read_lockfile(base_dir: &Path) -> Result<Option<EspmLock>> {
    let preferred = preferred_lockfile_path(base_dir);
    let legacy = legacy_lockfile_path(base_dir);

    let chosen = if preferred.exists() {
        Some(preferred)
    } else if legacy.exists() {
        Some(legacy)
    } else {
        None
    };

    let Some(path) = chosen else {
        return Ok(None);
    };

    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read lockfile from {}", path.display()))?;
    let lock: EspmLock = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse lockfile from {}", path.display()))?;
    Ok(Some(lock))
}

pub fn should_install_locked_package(options: InstallOptions, package: &LockPackage) -> bool {
    options.include_dev || !package.dev
}

pub async fn install_from_lockfile(
    lock: &EspmLock,
    options: InstallOptions,
    require_esm: bool,
) -> Result<usize> {
    let mut installed_count = 0usize;

    for package in &lock.packages {
        if !should_install_locked_package(options, package) {
            continue;
        }

        let (kind, scope, name) = parse_lock_package_id(&package.id)?;

        if !options.force
            && should_skip_reinstall(scope.as_deref(), &name, &package.resolved_version)
        {
            Logger::info(&format!(
                "Skipping {}@{} (already installed)",
                package.id, package.resolved_version
            ));
            continue;
        }

        match kind.as_str() {
            "npm" | "jsr" => {
                download_tarball(
                    &package.tarball,
                    scope.as_deref().unwrap_or(""),
                    &name,
                    require_esm,
                )
                .await
                .with_context(|| format!("Failed to install {} from lockfile", package.id))?;
            }
            "file" => {
                let source_path = Path::new(&package.tarball);
                if source_path.is_dir() {
                    install_directory_to_node_modules(
                        source_path,
                        scope.as_deref(),
                        &name,
                        require_esm,
                    )?;
                } else {
                    let bytes = fs::read(source_path).with_context(|| {
                        format!(
                            "Failed to read local file package from {}",
                            source_path.display()
                        )
                    })?;
                    extract_tarball_to_node_modules(
                        &bytes,
                        scope.as_deref().unwrap_or(""),
                        &name,
                        &package.tarball,
                        require_esm,
                    )?;
                }
            }
            "http" | "https" => {
                let bytes = fetch_bytes_with_retry(&package.tarball, 3)
                    .await
                    .with_context(|| format!("Failed to download {}", package.tarball))?;
                extract_tarball_to_node_modules(
                    &bytes,
                    scope.as_deref().unwrap_or(""),
                    &name,
                    &package.tarball,
                    require_esm,
                )?;
            }
            _ => {
                Logger::warn(&format!(
                    "Skipping unsupported lockfile package kind '{}' for {}",
                    kind, package.id
                ));
                continue;
            }
        }

        installed_count += 1;
    }

    Ok(installed_count)
}

pub async fn fetch_npm_package_data(
    scope: Option<&str>,
    name: &str,
) -> Result<NPMRegistryResponse> {
    let package_name = npm_package_display(scope, name);
    let url = format!("https://registry.npmjs.org/{}", package_name);
    fetch_json_with_retry::<NPMRegistryResponse>(&url, 3)
        .await
        .with_context(|| format!("Failed to fetch NPM package data for {}", package_name))
}

pub async fn fetch_jsr_package_data(scope: &str, name: &str) -> Result<JsrNpmRegistryResponse> {
    let npm_package_name = jsr_package_to_npm_package(scope, name);
    let url = format!("https://npm.jsr.io/{}", npm_package_name);
    fetch_json_with_retry::<JsrNpmRegistryResponse>(&url, 3)
        .await
        .with_context(|| format!("Failed to fetch JSR package data for @{}/{}", scope, name))
}

pub fn lock_key_for_sort(entry: &LockPackage) -> String {
    format!("{}@{}", entry.id, entry.resolved_version)
}

pub fn package_install_path(scope: Option<&str>, name: &str) -> PathBuf {
    match scope {
        Some(s) if !s.is_empty() => Path::new("./node_modules")
            .join(format!("@{}", s.trim_start_matches('@')))
            .join(name),
        _ => Path::new("./node_modules").join(name),
    }
}

pub fn installed_package_version(scope: Option<&str>, name: &str) -> Result<Option<String>> {
    let package_dir = package_install_path(scope, name);
    let package_json_path = package_dir.join("package.json");

    if !package_json_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&package_json_path)
        .with_context(|| format!("Failed to read {}", package_json_path.display()))?;
    let data: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", package_json_path.display()))?;

    Ok(data
        .get("version")
        .and_then(|value| value.as_str())
        .map(str::to_string))
}

pub fn should_skip_reinstall(scope: Option<&str>, name: &str, expected_version: &str) -> bool {
    match installed_package_version(scope, name) {
        Ok(Some(installed)) => installed == expected_version,
        Ok(None) => false,
        Err(error) => {
            Logger::warn(&format!(
                "Could not inspect installed package {}{}: {}",
                scope
                    .map(|s| format!("@{}/", s.trim_start_matches('@')))
                    .unwrap_or_default(),
                name,
                error
            ));
            false
        }
    }
}

pub async fn download_tarball(
    tarball_url: &str,
    scope: &str,
    name: &str,
    require_esm: bool,
) -> Result<()> {
    let content = fetch_bytes_with_retry(tarball_url, 3)
        .await
        .with_context(|| format!("Failed to download tarball from {}", tarball_url))?;

    extract_tarball_to_node_modules(&content, scope, name, tarball_url, require_esm)
}

pub fn extract_tarball_to_node_modules(
    content: &[u8],
    scope: &str,
    name: &str,
    source_label: &str,
    require_esm: bool,
) -> Result<()> {
    let is_esm = inspect_tgz_bytes_for_esm(content, source_label)?;
    if !is_esm {
        let msg = format!(
            "Package {} (from {}) does not appear to be ESM",
            name, source_label
        );
        if require_esm {
            return Err(anyhow::anyhow!(msg));
        } else {
            Logger::warn(&msg);
        }
    }

    let tar = GzDecoder::new(Cursor::new(content));
    let mut archive = Archive::new(tar);

    fs::create_dir_all("./node_modules").context("Failed to create node_modules directory")?;

    let target_dir = package_install_path(if scope.is_empty() { None } else { Some(scope) }, name);

    if target_dir.exists() {
        fs::remove_dir_all(&target_dir).with_context(|| {
            format!(
                "Failed to clean existing directory {}",
                target_dir.display()
            )
        })?;
    }
    fs::create_dir_all(&target_dir)
        .with_context(|| format!("Failed to create directory {}", target_dir.display()))?;

    for entry in archive
        .entries()
        .with_context(|| format!("Failed to read entries from tarball {}", source_label))?
    {
        let mut entry = entry?;
        let path = entry.path()?;
        let mut components = path.components();
        components.next();
        let stripped_path: std::path::PathBuf = components.as_path().to_path_buf();
        if stripped_path.as_os_str().is_empty() {
            continue;
        }
        let out_path = target_dir.join(&stripped_path);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {:?}", parent))?;
        }
        entry
            .unpack(&out_path)
            .with_context(|| format!("Failed to unpack entry to {:?}", out_path))?;
    }

    Ok(())
}

pub fn copy_directory_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)
        .with_context(|| format!("Failed to create directory {}", dst.display()))?;

    for entry in fs::read_dir(src)
        .with_context(|| format!("Failed to read source directory {}", src.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let source_path = entry.path();
        let destination_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_directory_recursive(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }

    Ok(())
}

pub fn install_directory_to_node_modules(
    src: &Path,
    scope: Option<&str>,
    name: &str,
    require_esm: bool,
) -> Result<()> {
    let is_esm = inspect_dir_for_esm(src)?;
    if !is_esm {
        let msg = format!(
            "Local package {} at {} does not appear to be ESM",
            name,
            src.display()
        );
        if require_esm {
            return Err(anyhow::anyhow!(msg));
        } else {
            Logger::warn(&msg);
        }
    }

    let target_dir = package_install_path(scope, name);
    if target_dir.exists() {
        fs::remove_dir_all(&target_dir).with_context(|| {
            format!(
                "Failed to clean existing directory {}",
                target_dir.display()
            )
        })?;
    }
    copy_directory_recursive(src, &target_dir)
}

pub async fn download_jsr_package(
    scope: &str,
    name: &str,
    version: &str,
    require_esm: bool,
) -> Result<()> {
    let npm_package_name = jsr_package_to_npm_package(scope, name);

    let client = Client::new();
    let npm_jsr_url = format!("https://npm.jsr.io/{}", npm_package_name);

    let response = client
        .get(&npm_jsr_url)
        .send()
        .await
        .with_context(|| format!("Failed to fetch package data from {}", npm_jsr_url))?;
    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to fetch package data: {}",
            response.status()
        ));
    }

    let package_data: JsrNpmRegistryResponse = response
        .json()
        .await
        .with_context(|| format!("Failed to parse package data from {}", npm_jsr_url))?;
    let version_data = if version == "latest" {
        let versions = &package_data.versions;
        let (first_version, data) = versions
            .iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No versions available in package data"))?;
        Logger::info(&format!("Using latest version: {}", first_version));
        data
    } else {
        package_data
            .versions
            .get(version)
            .ok_or_else(|| anyhow::anyhow!("Version {} not found in package data", version))?
    };
    let tarball_url = &version_data.dist.tarball;

    download_tarball(tarball_url, scope, name, require_esm).await?;
    Ok(())
}

pub async fn download_npm_package(
    scope: Option<&str>,
    name: &str,
    version: &str,
    require_esm: bool,
) -> Result<()> {
    let npm_package_url = npm_tarball_url(scope, name, version);
    let install_scope = scope.unwrap_or("");

    download_tarball(&npm_package_url, install_scope, name, require_esm).await?;
    Ok(())
}

pub fn parse_version_req(requirement: Option<&str>) -> Option<VersionReq> {
    let req = requirement?.trim();
    if req.is_empty() || req == "latest" {
        return None;
    }

    if let Ok(parsed) = VersionReq::parse(req) {
        return Some(parsed);
    }

    let exact = format!("={}", req);
    VersionReq::parse(&exact).ok()
}

pub fn select_latest_compatible_version<I>(
    versions: I,
    latest_tag: Option<&str>,
    requirement: Option<&str>,
) -> Result<String>
where
    I: Iterator<Item = String>,
{
    let parsed_versions: Vec<(Version, String)> = versions
        .filter_map(|raw| Version::parse(&raw).ok().map(|v| (v, raw)))
        .collect();

    if parsed_versions.is_empty() {
        return Err(anyhow::anyhow!("No valid semver versions available"));
    }

    let version_req = parse_version_req(requirement);

    if let Some(req) = version_req {
        let best = parsed_versions
            .iter()
            .filter(|(v, _)| req.matches(v))
            .max_by(|a, b| a.0.cmp(&b.0));

        if let Some((_, raw)) = best {
            return Ok(raw.clone());
        }

        return Err(anyhow::anyhow!(
            "No version matches requirement '{}'",
            requirement.unwrap_or_default()
        ));
    }

    if let Some(tag) = latest_tag {
        if Version::parse(tag).is_ok() {
            return Ok(tag.to_string());
        }
    }

    parsed_versions
        .iter()
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, raw)| raw.clone())
        .ok_or_else(|| anyhow::anyhow!("Unable to select latest version"))
}

pub async fn resolve_latest_npm_version(
    scope: Option<&str>,
    name: &str,
    requirement: Option<&str>,
) -> Result<String> {
    let package_data = fetch_npm_package_data(scope, name).await?;

    let latest_tag = package_data.dist_tags.get("latest").map(String::as_str);
    let versions = package_data.versions.keys().cloned();

    select_latest_compatible_version(versions, latest_tag, requirement)
}

pub async fn resolve_latest_jsr_version(
    scope: &str,
    name: &str,
    requirement: Option<&str>,
) -> Result<String> {
    let package_data = fetch_jsr_package_data(scope, name).await?;

    let latest_tag = package_data.dist_tags.get("latest").map(String::as_str);
    let versions = package_data.versions.keys().cloned();

    select_latest_compatible_version(versions, latest_tag, requirement)
}

pub async fn resolve_npm_package(
    scope: Option<&str>,
    name: &str,
    requirement: Option<&str>,
    dev: bool,
) -> Result<ResolvedPackage> {
    let package_data = fetch_npm_package_data(scope, name).await?;
    let latest_tag = package_data.dist_tags.get("latest").map(String::as_str);
    let version = select_latest_compatible_version(
        package_data.versions.keys().cloned(),
        latest_tag,
        requirement,
    )?;

    let version_info = package_data
        .versions
        .get(&version)
        .ok_or_else(|| anyhow::anyhow!("Version {} not found in package metadata", version))?;

    Ok(ResolvedPackage {
        kind: "npm".to_string(),
        key: package_id("npm", scope, name),
        source: requested_specifier_from_parts("npm", scope, name, &version),
        scope: scope.map(|s| s.trim_start_matches('@').to_string()),
        name: name.to_string(),
        requested: requirement.map(str::to_string),
        version,
        tarball: version_info.dist.tarball.clone(),
        dependencies: version_info.dependencies.clone().unwrap_or_default(),
        dev,
    })
}

pub async fn resolve_jsr_package(
    scope: &str,
    name: &str,
    requirement: Option<&str>,
    dev: bool,
) -> Result<ResolvedPackage> {
    let package_data = fetch_jsr_package_data(scope, name).await?;
    let latest_tag = package_data.dist_tags.get("latest").map(String::as_str);
    let version = select_latest_compatible_version(
        package_data.versions.keys().cloned(),
        latest_tag,
        requirement,
    )?;

    let version_info = package_data
        .versions
        .get(&version)
        .ok_or_else(|| anyhow::anyhow!("Version {} not found in package metadata", version))?;

    Ok(ResolvedPackage {
        kind: "jsr".to_string(),
        key: package_id("jsr", Some(scope), name),
        source: requested_specifier_from_parts("jsr", Some(scope), name, &version),
        scope: Some(scope.trim_start_matches('@').to_string()),
        name: name.to_string(),
        requested: requirement.map(str::to_string),
        version,
        tarball: version_info.dist.tarball.clone(),
        dependencies: version_info.dependencies.clone().unwrap_or_default(),
        dev,
    })
}

pub async fn download_package(
    specifier: &Specifier,
    _is_dev: bool,
    base_dir: &Path,
    require_esm: bool,
) -> Result<()> {
    let (identity_scope, identity_name, _) =
        package_identity_from_specifier(specifier, base_dir).await?;

    let scope = identity_scope.as_deref().unwrap_or("default");
    let name = identity_name.as_str();
    let version = specifier.version.as_deref().unwrap_or("latest");

    match specifier.kind.as_str() {
        "jsr" => {
            if (version == "latest" || version.is_empty()) && specifier.scope.is_none() {
                Logger::warn(&format!(
                    "Adding JSR package {} without version is not supported. Please specify a version.",
                    name.cyan()
                ));
                return Ok(());
            }
            download_jsr_package(scope, name, version, require_esm).await?;
        }
        "npm" => {
            if (version == "latest" || version.is_empty()) && specifier.scope.is_none() {
                Logger::warn(&format!(
                    "Adding NPM package {} wihout version is not supported. Please specify a version.",
                    name.cyan()
                ));
                return Ok(());
            }

            download_npm_package(specifier.scope.as_deref(), name, version, require_esm).await?;
        }
        "file" => {
            let path = specifier.path.as_deref().ok_or_else(|| {
                anyhow::anyhow!("Missing file path in specifier '{}'", specifier.source)
            })?;
            let resolved_path = resolve_file_source_path(base_dir, path);
            if resolved_path.is_dir() {
                install_directory_to_node_modules(
                    &resolved_path,
                    identity_scope.as_deref(),
                    name,
                    require_esm,
                )?;
            } else {
                let bytes = fs::read(&resolved_path).with_context(|| {
                    format!(
                        "Failed to read local package file {}",
                        resolved_path.display()
                    )
                })?;
                extract_tarball_to_node_modules(
                    &bytes,
                    identity_scope.as_deref().unwrap_or(""),
                    name,
                    &resolved_path.to_string_lossy(),
                    require_esm,
                )?;
            }
        }
        "http" | "https" => {
            let bytes = fetch_bytes_with_retry(&specifier.source, 3)
                .await
                .with_context(|| format!("Failed to download {}", specifier.source))?;
            extract_tarball_to_node_modules(
                &bytes,
                identity_scope.as_deref().unwrap_or(""),
                name,
                &specifier.source,
                require_esm,
            )?;
        }
        _ => {
            return Err(anyhow::anyhow!(
                "Kind de package non pris en charge: {}",
                specifier.kind
            ));
        }
    }
    Ok(())
}

pub fn sort_imports(import_map: &mut ImportMap) {
    let mut entries: Vec<(String, String)> = import_map.imports.drain().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    import_map.imports = entries.into_iter().collect();
}

pub async fn handle_add_command(specifier: String, is_dev: bool, require_esm: bool) -> Result<()> {
    let specifier = Specifier::from_string(&specifier)
        .with_context(|| format!("Failed to parse specifier: {}", specifier))?;

    let espm_json_path = match get_espm_json_path().await {
        Ok(path) => path,
        Err(_) => {
            let new = serde_json::json!({});
            let path = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join("espm.json");
            fs::write(&path, serde_json::to_string_pretty(&new)?)
                .with_context(|| format!("Failed to create espm.json at {}", path.display()))?;
            path
        }
    };

    let base_dir = espm_json_path.parent().unwrap_or_else(|| Path::new("."));
    let (identity_scope, identity_name, _) = package_identity_from_specifier(&specifier, base_dir)
        .await
        .with_context(|| {
            format!(
                "Failed to determine package identity for {}",
                specifier.source
            )
        })?;

    let content = fs::read_to_string(&espm_json_path)
        .with_context(|| format!("Failed to read espm.json from {}", espm_json_path.display()))?;
    let mut espm_json: EspmJson = serde_json::from_str(&content)?;

    let import_map = if is_dev {
        espm_json.import_map_dev.get_or_insert(ImportMap {
            imports: std::collections::HashMap::new(),
            scopes: None,
        })
    } else {
        espm_json.import_map.get_or_insert(ImportMap {
            imports: std::collections::HashMap::new(),
            scopes: None,
        })
    };

    let dep_name = package_key(identity_scope.as_deref(), &identity_name);
    import_map
        .imports
        .insert(dep_name, specifier.source.clone());

    sort_imports(import_map);

    fs::write(&espm_json_path, serde_json::to_string_pretty(&espm_json)?)
        .with_context(|| format!("Failed to write espm.json at {}", espm_json_path.display()))?;

    let lock_path = preferred_lockfile_path(base_dir);
    if lock_path.exists() {
        fs::remove_file(&lock_path).with_context(|| {
            format!("Failed to remove stale lockfile at {}", lock_path.display())
        })?;
    }

    handle_install_command(true, false, require_esm)
        .await
        .with_context(|| {
            format!(
                "Failed to refresh installation and lockfile after adding {}",
                specifier.source
            )
        })?;

    Logger::success(&format!(
        "Package {} added successfully to {}.",
        specifier.source.cyan(),
        if is_dev {
            "development dependencies".bold()
        } else {
            "dependencies".bold()
        }
    ));
    Ok(())
}

pub fn requests_from_import_map(
    import_map: &ImportMap,
    dev: bool,
) -> Result<Vec<DependencyRequest>> {
    let mut requests = Vec::new();

    for (dep_key, value) in &import_map.imports {
        let specifier = Specifier::from_string(value)
            .with_context(|| format!("Invalid dependency specifier '{}'", value))?;

        match specifier.kind.as_str() {
            "npm" | "jsr" => {
                let name = specifier
                    .name
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("Missing package name in '{}'", value))?;

                requests.push(DependencyRequest {
                    source: value.clone(),
                    kind: specifier.kind,
                    scope: specifier.scope,
                    name,
                    requirement: specifier.version,
                    dev,
                });
            }
            "file" | "http" | "https" => {
                let (scope, name) = parse_package_key(dep_key)?;
                requests.push(DependencyRequest {
                    source: value.clone(),
                    kind: specifier.kind,
                    scope,
                    name,
                    requirement: None,
                    dev,
                });
            }
            other => {
                Logger::warn(&format!(
                    "Skipping unsupported dependency kind '{}' for '{}'",
                    other, value
                ));
            }
        }
    }

    Ok(requests)
}

pub fn dependency_requests_from_package(pkg: &ResolvedPackage) -> Vec<DependencyRequest> {
    pkg.dependencies
        .iter()
        .filter_map(|(dep_name, requirement)| {
            parse_npm_dependency_name(dep_name).map(|(scope, name)| DependencyRequest {
                source: requested_specifier_from_parts("npm", scope.as_deref(), &name, requirement),
                kind: "npm".to_string(),
                scope,
                name,
                requirement: Some(requirement.clone()),
                dev: pkg.dev,
            })
        })
        .collect()
}

pub async fn resolve_dependency_request(
    request: &DependencyRequest,
    base_dir: &Path,
) -> Result<ResolvedPackage> {
    match request.kind.as_str() {
        "npm" => {
            resolve_npm_package(
                request.scope.as_deref(),
                &request.name,
                request.requirement.as_deref(),
                request.dev,
            )
            .await
        }
        "jsr" => {
            let scope = request.scope.as_deref().ok_or_else(|| {
                anyhow::anyhow!("JSR package '{}' is missing scope", request.name)
            })?;
            resolve_jsr_package(
                scope,
                &request.name,
                request.requirement.as_deref(),
                request.dev,
            )
            .await
        }
        "file" => {
            let specifier = Specifier::from_string(&request.source)
                .with_context(|| format!("Invalid file specifier '{}'", request.source))?;
            let raw_path = specifier.path.as_deref().ok_or_else(|| {
                anyhow::anyhow!("Missing file path in specifier '{}'", request.source)
            })?;
            let resolved_path = resolve_file_source_path(base_dir, raw_path);
            let version = if resolved_path.is_dir() {
                read_package_manifest_from_dir(&resolved_path)?.2
            } else {
                let bytes = fs::read(&resolved_path).with_context(|| {
                    format!(
                        "Failed to read local package file {}",
                        resolved_path.display()
                    )
                })?;
                read_package_manifest_from_tgz_bytes(&bytes, &resolved_path.to_string_lossy())?.2
            }
            .unwrap_or_else(|| "0.0.0".to_string());

            Ok(ResolvedPackage {
                kind: "file".to_string(),
                key: package_id("file", request.scope.as_deref(), &request.name),
                source: request.source.clone(),
                scope: request.scope.clone(),
                name: request.name.clone(),
                requested: None,
                version,
                tarball: resolved_path.to_string_lossy().to_string(),
                dependencies: HashMap::new(),
                dev: request.dev,
            })
        }
        "http" | "https" => {
            let bytes = fetch_bytes_with_retry(&request.source, 3)
                .await
                .with_context(|| format!("Failed to inspect remote package {}", request.source))?;
            let version = read_package_manifest_from_tgz_bytes(&bytes, &request.source)
                .map(|(_, _, version)| version.unwrap_or_else(|| "0.0.0".to_string()))
                .unwrap_or_else(|_| "0.0.0".to_string());

            Ok(ResolvedPackage {
                kind: request.kind.clone(),
                key: package_id(&request.kind, request.scope.as_deref(), &request.name),
                source: request.source.clone(),
                scope: request.scope.clone(),
                name: request.name.clone(),
                requested: None,
                version,
                tarball: request.source.clone(),
                dependencies: HashMap::new(),
                dev: request.dev,
            })
        }
        _ => Err(anyhow::anyhow!(
            "Unsupported dependency kind '{}'",
            request.kind
        )),
    }
}

pub async fn install_resolved_package(
    pkg: &ResolvedPackage,
    options: InstallOptions,
    require_esm: bool,
) -> Result<bool> {
    if !options.force && should_skip_reinstall(pkg.scope.as_deref(), &pkg.name, &pkg.version) {
        Logger::info(&format!(
            "Skipping {}@{} (already installed)",
            pkg.key, pkg.version
        ));
        return Ok(false);
    }

    let scope = pkg.scope.as_deref().unwrap_or("");
    match pkg.kind.as_str() {
        "npm" | "jsr" => {
            download_tarball(&pkg.tarball, scope, &pkg.name, require_esm)
                .await
                .with_context(|| format!("Failed to install {}", pkg.source))?;
        }
        "file" => {
            let source_path = Path::new(&pkg.tarball);
            if source_path.is_dir() {
                install_directory_to_node_modules(
                    source_path,
                    pkg.scope.as_deref(),
                    &pkg.name,
                    require_esm,
                )?;
            } else {
                let bytes = fs::read(source_path).with_context(|| {
                    format!(
                        "Failed to read local package file {}",
                        source_path.display()
                    )
                })?;
                extract_tarball_to_node_modules(
                    &bytes,
                    scope,
                    &pkg.name,
                    &pkg.tarball,
                    require_esm,
                )?;
            }
        }
        "http" | "https" => {
            let bytes = fetch_bytes_with_retry(&pkg.tarball, 3)
                .await
                .with_context(|| format!("Failed to download {}", pkg.tarball))?;
            extract_tarball_to_node_modules(&bytes, scope, &pkg.name, &pkg.tarball, require_esm)?;
        }
        other => {
            return Err(anyhow::anyhow!("Unsupported package kind '{}'", other));
        }
    }

    Ok(true)
}

pub fn finalize_lockfile(mut entries: Vec<LockPackage>) -> EspmLock {
    entries.sort_by_key(lock_key_for_sort);
    EspmLock {
        version: 1,
        packages: entries,
    }
}

pub fn build_install_queue(
    espm_json: &EspmJson,
    options: InstallOptions,
) -> Result<VecDeque<DependencyRequest>> {
    let mut queue: VecDeque<DependencyRequest> = VecDeque::new();

    if let Some(import_map) = &espm_json.import_map {
        for request in requests_from_import_map(import_map, false)? {
            queue.push_back(request);
        }
    }

    if options.include_dev {
        if let Some(import_map_dev) = &espm_json.import_map_dev {
            for request in requests_from_import_map(import_map_dev, true)? {
                queue.push_back(request);
            }
        }
    }

    Ok(queue)
}

pub async fn try_install_from_lockfile(
    base_dir: &Path,
    options: InstallOptions,
    require_esm: bool,
) -> Result<Option<usize>> {
    match read_lockfile(base_dir) {
        Ok(Some(lock)) => {
            Logger::info("Using lockfile for deterministic install.");
            write_lockfile(&lock, base_dir)?;
            let installed_count = install_from_lockfile(&lock, options, require_esm).await?;
            Ok(Some(installed_count))
        }
        Ok(None) => Ok(None),
        Err(error) => {
            Logger::warn(&format!(
                "Failed to use lockfile ({}). Falling back to registry resolution.",
                error
            ));
            Ok(None)
        }
    }
}

pub async fn handle_install_command(dev: bool, force: bool, require_esm: bool) -> Result<()> {
    let options = InstallOptions {
        include_dev: dev,
        force,
    };

    Logger::info(&format!(
        "Installing {} dependencies...",
        options.dependency_scope_label()
    ));

    let espm_json_path = get_espm_json_path().await?;
    let content = fs::read_to_string(&espm_json_path)
        .with_context(|| format!("Failed to read espm.json from {}", espm_json_path.display()))?;
    let espm_json: EspmJson = serde_json::from_str(&content).with_context(|| {
        format!(
            "Failed to parse espm.json from {}",
            espm_json_path.display()
        )
    })?;

    let base_dir = espm_json_path.parent().unwrap_or_else(|| Path::new("."));

    // Transactional install: move existing `node_modules` out of the way before
    // attempting installation. If installation fails, restore the previous
    // `node_modules`. On success, remove the backup.
    fn node_modules_path() -> PathBuf {
        Path::new("./node_modules").to_path_buf()
    }

    fn backup_node_modules() -> Result<Option<PathBuf>> {
        let nm = node_modules_path();
        if nm.exists() {
            let mut backup = Path::new("./node_modules.backup").to_path_buf();
            let mut i = 0;
            while backup.exists() {
                i += 1;
                backup = Path::new(&format!("./node_modules.backup.{}", i)).to_path_buf();
            }
            fs::rename(&nm, &backup).with_context(|| {
                format!(
                    "Failed to move existing node_modules to {}",
                    backup.display()
                )
            })?;
            return Ok(Some(backup));
        }
        Ok(None)
    }

    fn restore_node_modules(backup: Option<PathBuf>) -> Result<()> {
        if let Some(backup) = backup {
            let nm = node_modules_path();
            if nm.exists() {
                fs::remove_dir_all(&nm).with_context(|| {
                    format!(
                        "Failed to remove incomplete node_modules at {}",
                        nm.display()
                    )
                })?;
            }
            fs::rename(&backup, &nm).with_context(|| {
                format!("Failed to restore node_modules from {}", backup.display())
            })?;
        }
        Ok(())
    }

    fn remove_node_modules_backup(backup: Option<PathBuf>) -> Result<()> {
        if let Some(backup) = backup {
            if backup.exists() {
                fs::remove_dir_all(&backup).with_context(|| {
                    format!(
                        "Failed to remove node_modules backup at {}",
                        backup.display()
                    )
                })?;
            }
        }
        Ok(())
    }

    let backup = match backup_node_modules() {
        Ok(b) => b,
        Err(e) => {
            Logger::warn(&format!("Failed to backup existing node_modules: {}", e));
            None
        }
    };

    // Run the original installation logic and decide whether to restore on error
    let install_result: Result<()> = (async {
        if let Some(installed_count) =
            try_install_from_lockfile(base_dir, options, require_esm).await?
        {
            Logger::info(&format!(
                "Installed {} package(s){} from lockfile.",
                installed_count,
                options.summary_suffix()
            ));
            Logger::success("Installation completed successfully.");
            return Ok(());
        }

        let mut queue = build_install_queue(&espm_json, options)?;

        if queue.is_empty() {
            Logger::warn("No installable dependencies found in espm.json. Skipping installation.");
            return Ok(());
        }

        let mut installed_versions: HashMap<String, String> = HashMap::new();
        let mut lock_entries: BTreeMap<String, LockPackage> = BTreeMap::new();
        let mut installed_count = 0usize;

        while let Some(request) = queue.pop_front() {
            let resolved = resolve_dependency_request(&request, base_dir)
                .await
                .with_context(|| {
                    format!(
                        "Failed to resolve {} package {}",
                        request.kind, request.name
                    )
                })?;

            if let Some(existing_version) = installed_versions.get(&resolved.key) {
                if existing_version != &resolved.version {
                    Logger::warn(&format!(
                        "Version conflict for {}: keeping {}, skipping {}",
                        resolved.key, existing_version, resolved.version
                    ));
                }
                continue;
            }

            let installed_now = install_resolved_package(&resolved, options, require_esm).await?;
            if installed_now {
                installed_count += 1;
            }
            installed_versions.insert(resolved.key.clone(), resolved.version.clone());

            lock_entries.insert(
                resolved.key.clone(),
                LockPackage {
                    id: resolved.key.clone(),
                    source: resolved.source.clone(),
                    resolved_version: resolved.version.clone(),
                    tarball: resolved.tarball.clone(),
                    requested: resolved.requested.clone(),
                    dev: resolved.dev,
                },
            );

            for transitive in dependency_requests_from_package(&resolved) {
                queue.push_back(transitive);
            }
        }

        let lockfile = finalize_lockfile(lock_entries.into_values().collect());
        write_lockfile(&lockfile, base_dir)?;

        Logger::info(&format!(
            "Installed {} package(s){}.",
            installed_count,
            options.summary_suffix()
        ));

        Logger::success("Installation completed successfully.");
        Ok(())
    })
    .await;

    match install_result {
        Ok(_) => {
            if let Err(e) = remove_node_modules_backup(backup) {
                Logger::warn(&format!("Failed to remove node_modules backup: {}", e));
            }
            Ok(())
        }
        Err(e) => {
            Logger::error(&format!("Installation failed: {}", e));
            if let Err(rest_err) = restore_node_modules(backup) {
                Logger::warn(&format!(
                    "Failed to restore node_modules from backup: {}",
                    rest_err
                ));
            }
            Err(e)
        }
    }
}

pub async fn handle_publish_command(npm: bool, dry_run: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;

    Logger::info(&format!(
        "Publishing from {} using {} registry{}.",
        cwd.display(),
        if npm { "npm" } else { "JSR" },
        if dry_run { " (dry-run)" } else { "" }
    ));

    publisher::publish_from_dir(&cwd, npm, dry_run).await
}

pub async fn handle_init_command() -> Result<()> {
    let espm_json_path = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("espm.json");
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

    if !espm_json_path.exists() {
        let new_espm_json = EspmJson {
            name: Some(
                cwd.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            ),
            import_map: None,
            import_map_dev: None,
        };
        fs::write(
            &espm_json_path,
            serde_json::to_string_pretty(&new_espm_json)?,
        )?;
        Logger::success(&format!(
            "Created new espm.json at {}",
            espm_json_path.display()
        ));
    } else {
        Logger::warn(&format!(
            "espm.json already exists at {}. Skipping initialization.",
            espm_json_path.display()
        ));
    }

    Ok(())
}

pub async fn handle_remove_command(package: String) -> Result<()> {
    let espm_json_path = get_espm_json_path().await?;
    let content = fs::read_to_string(&espm_json_path)?;
    let mut espm_json: EspmJson = serde_json::from_str(&content)?;

    let mut found = false;

    let (possible_names, original_specifier) = match Specifier::from_string(&package) {
        Ok(spec) => {
            let mut names = Vec::new();
            if let Some(scope) = &spec.scope {
                if let Some(name) = &spec.name {
                    names.push(format!("@{}/{}", scope, name));
                }
            }
            if let Some(name) = &spec.name {
                names.push(name.clone());
            }
            (names, Some(spec.source))
        }
        Err(_) => (vec![package.clone()], None),
    };

    if let Some(import_map) = &mut espm_json.import_map {
        for name in &possible_names {
            if import_map.imports.remove(name).is_some() {
                found = true;
            }
        }
        if let Some(spec_str) = &original_specifier {
            let to_remove: Vec<_> = import_map
                .imports
                .iter()
                .filter(|(_, v)| *v == spec_str)
                .map(|(k, _)| k.clone())
                .collect();
            for k in to_remove {
                import_map.imports.remove(&k);
                found = true;
            }
        }
    }

    if let Some(import_map_dev) = &mut espm_json.import_map_dev {
        for name in &possible_names {
            if import_map_dev.imports.remove(name).is_some() {
                found = true;
            }
        }
        if let Some(spec_str) = &original_specifier {
            let to_remove: Vec<_> = import_map_dev
                .imports
                .iter()
                .filter(|(_, v)| *v == spec_str)
                .map(|(k, _)| k.clone())
                .collect();
            for k in to_remove {
                import_map_dev.imports.remove(&k);
                found = true;
            }
        }
    }

    if !found {
        Logger::warn(&format!("Package '{}' not found in espm.json.", package));
    } else {
        for name in &possible_names {
            let node_modules_path = Path::new("./node_modules");
            let pkg_path = node_modules_path.join(name);
            if pkg_path.exists() {
                if let Err(e) = fs::remove_dir_all(&pkg_path) {
                    Logger::warn(&format!("Failed to remove directory {:?}: {}", pkg_path, e));
                }
            }
            if name.starts_with('@') {
                if let Some((scope, _)) = name.split_once('/') {
                    let scope_path = node_modules_path.join(scope);
                    if scope_path.exists()
                        && scope_path
                            .read_dir()
                            .map(|mut d| d.next().is_none())
                            .unwrap_or(false)
                    {
                        if let Err(e) = fs::remove_dir_all(&scope_path) {
                            Logger::warn(&format!(
                                "Failed to remove scope directory {:?}: {}",
                                scope_path, e
                            ));
                        }
                    }
                }
            }
        }
        fs::write(&espm_json_path, serde_json::to_string_pretty(&espm_json)?)?;
        Logger::success("Package removed successfully.");
    }

    Ok(())
}

pub async fn handle_update_command(package: String) -> Result<()> {
    let espm_json_path = get_espm_json_path().await?;
    let content = fs::read_to_string(&espm_json_path)
        .with_context(|| format!("Failed to read espm.json from {}", espm_json_path.display()))?;
    let mut espm_json: EspmJson = serde_json::from_str(&content).with_context(|| {
        format!(
            "Failed to parse espm.json from {}",
            espm_json_path.display()
        )
    })?;

    let mut targets: Vec<(bool, String, String)> = Vec::new();

    if let Some(import_map) = &espm_json.import_map {
        if let Some(spec) = import_map.imports.get(&package) {
            targets.push((false, package.clone(), spec.clone()));
        }
    }
    if let Some(import_map_dev) = &espm_json.import_map_dev {
        if let Some(spec) = import_map_dev.imports.get(&package) {
            targets.push((true, package.clone(), spec.clone()));
        }
    }

    if targets.is_empty() {
        return Err(anyhow::anyhow!(
            "Package '{}' not found in import_map or import_map_dev.",
            package
        ));
    }

    for (is_dev, dep_key, source) in targets {
        let parsed = Specifier::from_string(&source)
            .with_context(|| format!("Failed to parse specifier '{}'", source))?;

        let resolved_version = match parsed.kind.as_str() {
            "jsr" => {
                let scope = parsed.scope.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("JSR specifier '{}' is missing scope", parsed.source)
                })?;
                let name = parsed.name.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("JSR specifier '{}' is missing package name", parsed.source)
                })?;
                resolve_latest_jsr_version(scope, name, parsed.version.as_deref()).await?
            }
            "npm" => {
                let name = parsed.name.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("NPM specifier '{}' is missing package name", parsed.source)
                })?;
                resolve_latest_npm_version(parsed.scope.as_deref(), name, parsed.version.as_deref())
                    .await?
            }
            _ => {
                Logger::warn(&format!(
                    "Skipping '{}' because '{}' dependencies are not updateable yet.",
                    dep_key, parsed.kind
                ));
                continue;
            }
        };

        let scope = parsed.scope.as_deref();
        let name = parsed
            .name
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Dependency '{}' has no package name", dep_key))?;

        let new_source = match parsed.kind.as_str() {
            "jsr" => format!(
                "jsr:@{}/{}@{}",
                scope.unwrap_or_default(),
                name,
                resolved_version
            ),
            "npm" => {
                if let Some(scope) = scope {
                    format!("npm:@{}/{}@{}", scope, name, resolved_version)
                } else {
                    format!("npm:{}@{}", name, resolved_version)
                }
            }
            _ => unreachable!(),
        };

        if new_source == source {
            Logger::info(&format!(
                "{} is already up-to-date ({})",
                dep_key.cyan(),
                resolved_version
            ));
            continue;
        }

        let map_opt = if is_dev {
            &mut espm_json.import_map_dev
        } else {
            &mut espm_json.import_map
        };

        if let Some(map) = map_opt {
            map.imports.insert(dep_key.clone(), new_source.clone());
            sort_imports(map);
        }

        let new_specifier = Specifier::from_string(&new_source)
            .with_context(|| format!("Failed to parse updated specifier '{}'", new_source))?;
        let base_dir = espm_json_path.parent().unwrap_or_else(|| Path::new("."));
        let require_esm_env = std::env::var("ESPM_REQUIRE_ESM")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);
        download_package(&new_specifier, is_dev, base_dir, require_esm_env)
            .await
            .with_context(|| {
                format!(
                    "Failed to install updated package '{}' ({})",
                    dep_key, new_source
                )
            })?;

        Logger::success(&format!(
            "Updated {} to {}",
            dep_key.cyan(),
            resolved_version.green()
        ));
    }

    fs::write(&espm_json_path, serde_json::to_string_pretty(&espm_json)?)
        .with_context(|| format!("Failed to write espm.json at {}", espm_json_path.display()))?;

    Ok(())
}

// tests live in src/main.test.rs and are run against the public CLI behavior
