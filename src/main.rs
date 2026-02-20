use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;
use flate2::read::GzDecoder;
use reqwest::Client;
use semver::{Version, VersionReq};
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tar::Archive;
use tokio::time::{sleep, Duration};

mod cli;
mod jsr_npm;
mod logger;
mod models;
mod specifier;

use cli::{Cli, Commands};
use jsr_npm::{JsrNpmRegistryResponse, NPMRegistryResponse};
use logger::Logger;
use models::{
    DependencyRequest, EspmJson, EspmLock, ImportMap, InstallOptions, LockPackage, ResolvedPackage,
};
use specifier::{
    jsr_package_to_npm_package, npm_tarball_url, parse_npm_dependency_name,
    requested_specifier_from_parts, Specifier,
};


async fn create_directory_if_not_exists(dir: &str) -> Result<()> {
    let path = Path::new(dir);
    if !path.exists() {
        fs::create_dir_all(path).with_context(|| format!("Failed to create directory: {}", dir))?;
    }
    Ok(())
}

async fn get_espm_json_path() -> Result<std::path::PathBuf> {
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

fn package_id(kind: &str, scope: Option<&str>, name: &str) -> String {
    match scope {
        Some(s) => format!("{}:@{}/{}", kind, s.trim_start_matches('@'), name),
        None => format!("{}:{}", kind, name),
    }
}

fn npm_package_display(scope: Option<&str>, name: &str) -> String {
    match scope {
        Some(s) => format!("@{}/{}", s.trim_start_matches('@'), name),
        None => name.to_string(),
    }
}

async fn fetch_json_with_retry<T: DeserializeOwned>(url: &str, attempts: u8) -> Result<T> {
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
                            last_error = Some(anyhow::anyhow!(
                                "Failed to parse JSON from {}: {}",
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

async fn fetch_bytes_with_retry(url: &str, attempts: u8) -> Result<Vec<u8>> {
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

fn preferred_lockfile_path(base_dir: &Path) -> std::path::PathBuf {
    base_dir.join("espm-lock.json")
}

fn legacy_lockfile_path(base_dir: &Path) -> std::path::PathBuf {
    base_dir.join("espm-lock.json")
}

fn write_lockfile(lock: &EspmLock, base_dir: &Path) -> Result<()> {
    let lock_path = preferred_lockfile_path(base_dir);
    fs::write(&lock_path, serde_json::to_string_pretty(lock)?)
        .with_context(|| format!("Failed to write lockfile at {}", lock_path.display()))
}

fn read_lockfile(base_dir: &Path) -> Result<Option<EspmLock>> {
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

fn should_install_locked_package(options: InstallOptions, package: &LockPackage) -> bool {
    options.include_dev || !package.dev
}

async fn install_from_lockfile(lock: &EspmLock, options: InstallOptions) -> Result<usize> {
    let mut installed_count = 0usize;

    for package in &lock.packages {
        if !should_install_locked_package(options, package) {
            continue;
        }

        let specifier = Specifier::from_string(&package.source).with_context(|| {
            format!(
                "Invalid package source '{}' in lockfile entry '{}'",
                package.source, package.id
            )
        })?;

        if specifier.kind != "npm" && specifier.kind != "jsr" {
            Logger::warn(&format!(
                "Skipping unsupported lockfile package kind '{}' for {}",
                specifier.kind, package.id
            ));
            continue;
        }

        let name = specifier.name.as_deref().ok_or_else(|| {
            anyhow::anyhow!("Missing package name in lockfile source '{}'.", package.source)
        })?;

        if !options.force
            && should_skip_reinstall(specifier.scope.as_deref(), name, &package.resolved_version)
        {
            Logger::info(&format!(
                "Skipping {}@{} (already installed)",
                package.id,
                package.resolved_version
            ));
            continue;
        }

        download_tarball(
            &package.tarball,
            specifier.scope.as_deref().unwrap_or(""),
            name,
        )
        .await
        .with_context(|| format!("Failed to install {} from lockfile", package.id))?;

        installed_count += 1;
    }

    Ok(installed_count)
}

async fn fetch_npm_package_data(scope: Option<&str>, name: &str) -> Result<NPMRegistryResponse> {
    let package_name = npm_package_display(scope, name);
    let url = format!("https://registry.npmjs.org/{}", package_name);
    fetch_json_with_retry::<NPMRegistryResponse>(&url, 3)
        .await
        .with_context(|| format!("Failed to fetch NPM package data for {}", package_name))
}

async fn fetch_jsr_package_data(scope: &str, name: &str) -> Result<JsrNpmRegistryResponse> {
    let npm_package_name = jsr_package_to_npm_package(scope, name);
    let url = format!("https://npm.jsr.io/{}", npm_package_name);
    fetch_json_with_retry::<JsrNpmRegistryResponse>(&url, 3)
        .await
        .with_context(|| format!("Failed to fetch JSR package data for @{}/{}", scope, name))
}

fn lock_key_for_sort(entry: &LockPackage) -> String {
    format!("{}@{}", entry.id, entry.resolved_version)
}

fn package_install_path(scope: Option<&str>, name: &str) -> PathBuf {
    match scope {
        Some(s) if !s.is_empty() => Path::new("./node_modules")
            .join(format!("@{}", s.trim_start_matches('@')))
            .join(name),
        _ => Path::new("./node_modules").join(name),
    }
}

fn installed_package_version(scope: Option<&str>, name: &str) -> Result<Option<String>> {
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

fn should_skip_reinstall(scope: Option<&str>, name: &str, expected_version: &str) -> bool {
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

async fn download_tarball(tarball_url: &str, scope: &str, name: &str) -> Result<()> {
    let content = fetch_bytes_with_retry(tarball_url, 3)
        .await
        .with_context(|| format!("Failed to download tarball from {}", tarball_url))?;

    let tar = GzDecoder::new(Cursor::new(content));
    let mut archive = Archive::new(tar);

    // Ensure the base node_modules directory exists
    create_directory_if_not_exists("./node_modules").await?;

    // If scope is not empty, use @<scope>/<name>, else just <name>
    let target_dir = if !scope.is_empty() {
        format!("./node_modules/@{}/{}", scope.trim_start_matches('@'), name)
    } else {
        format!("./node_modules/{}", name)
    };

    if Path::new(&target_dir).exists() {
        fs::remove_dir_all(&target_dir)
            .with_context(|| format!("Failed to clean existing directory {}", target_dir))?;
    }
    create_directory_if_not_exists(&target_dir).await?;

    // Extract the tarball into the target directory, stripping the first component (usually "package/")
    for entry in archive
        .entries()
        .with_context(|| format!("Failed to read entries from tarball {}", tarball_url))?
    {
        let mut entry = entry?;
        let path = entry.path()?;
        let mut components = path.components();
        components.next(); // Strip the first component ("package" or similar)
        let stripped_path: std::path::PathBuf = components.as_path().to_path_buf();
        let out_path = Path::new(&target_dir).join(&stripped_path);
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

async fn download_jsr_package(scope: &str, name: &str, version: &str) -> Result<()> {
    let npm_package_name = jsr_package_to_npm_package(scope, name);

    let client = Client::new();
    let npm_jsr_url = format!("https://npm.jsr.io/{}", npm_package_name);
    // https://npm.jsr.io/@jsr/am__neuralnetwork

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
        // If "latest", pick the first version in the "versions" object
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

    download_tarball(&tarball_url, scope, name).await?;
    Ok(())
}

async fn download_npm_package(scope: Option<&str>, name: &str, version: &str) -> Result<()> {
    let npm_package_url = npm_tarball_url(scope, name, version);
    let install_scope = scope.unwrap_or("");

    download_tarball(&npm_package_url, install_scope, name).await?;
    Ok(())
}

fn parse_version_req(requirement: Option<&str>) -> Option<VersionReq> {
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

fn select_latest_compatible_version<I>(
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

async fn resolve_latest_npm_version(
    scope: Option<&str>,
    name: &str,
    requirement: Option<&str>,
) -> Result<String> {
    let package_data = fetch_npm_package_data(scope, name).await?;

    let latest_tag = package_data.dist_tags.get("latest").map(String::as_str);
    let versions = package_data.versions.keys().cloned();

    select_latest_compatible_version(versions, latest_tag, requirement)
}

async fn resolve_latest_jsr_version(
    scope: &str,
    name: &str,
    requirement: Option<&str>,
) -> Result<String> {
    let package_data = fetch_jsr_package_data(scope, name).await?;

    let latest_tag = package_data.dist_tags.get("latest").map(String::as_str);
    let versions = package_data.versions.keys().cloned();

    select_latest_compatible_version(versions, latest_tag, requirement)
}

async fn resolve_npm_package(
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

async fn resolve_jsr_package(
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

async fn download_package(specifier: &Specifier, _is_dev: bool) -> Result<()> {
    let scope = specifier.scope.as_deref().unwrap_or("default");
    let name = specifier.name.as_deref().unwrap_or("unknown");
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
            download_jsr_package(scope, name, version).await?;
        }
        "npm" => {
            // Gérer le cas "latest" sans version explicite
            if (version == "latest" || version.is_empty()) && specifier.scope.is_none() {
                Logger::warn(&format!(
                    "Adding NPM package {} wihout version is not supported. Please specify a version.",
                    name.cyan()
                ));
                return Ok(());
            }

            download_npm_package(specifier.scope.as_deref(), name, version).await?;
        }
        "file" => {
            Logger::info("Adding file:// non encore supporté");
            // vous pourriez appeler quelque chose comme download_file_package(...)
        }
        "http" | "https" => {
            Logger::info("Adding HTTP(S) non encore supporté");
            // vous pourriez appeler quelque chose comme download_http_package(...)
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

fn sort_imports(import_map: &mut ImportMap) {
    let mut entries: Vec<(String, String)> = import_map.imports.drain().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    import_map.imports = entries.into_iter().collect();
}

async fn handle_add_command(specifier: String, is_dev: bool) -> Result<()> {
    let specifier = Specifier::from_string(&specifier)
        .with_context(|| format!("Failed to parse specifier: {}", specifier))?;

    download_package(&specifier, is_dev)
        .await
        .with_context(|| format!("Failed to download package: {}", specifier.source))?;

    // Load espm.json (or create if missing)
    let espm_json_path = match get_espm_json_path().await {
        Ok(path) => path,
        Err(_) => {
            // Create a new espm.json if not found with any key
            let new = serde_json::json!({});
            let path = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join("espm.json");
            fs::write(&path, serde_json::to_string_pretty(&new)?)
                .with_context(|| format!("Failed to create espm.json at {}", path.display()))?;
            path
        }
    };

    let content = fs::read_to_string(&espm_json_path)
        .with_context(|| format!("Failed to read espm.json from {}", espm_json_path.display()))?;
    let mut espm_json: EspmJson = serde_json::from_str(&content)?;

    // Determine which import_map to update
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

    // Add or update the dependency in the import_map
    let dep_name = if let Some(scope) = &specifier.scope {
        format!("@{}/{}", scope, specifier.name.as_deref().unwrap_or(""))
    } else {
        specifier.name.as_deref().unwrap_or("").to_string()
    };
    import_map
        .imports
        .insert(dep_name, specifier.source.clone());

    sort_imports(import_map);

    fs::write(&espm_json_path, serde_json::to_string_pretty(&espm_json)?)
        .with_context(|| format!("Failed to write espm.json at {}", espm_json_path.display()))?;

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

fn requests_from_import_map(import_map: &ImportMap, dev: bool) -> Result<Vec<DependencyRequest>> {
    let mut requests = Vec::new();

    for value in import_map.imports.values() {
        let specifier = Specifier::from_string(value)
            .with_context(|| format!("Invalid dependency specifier '{}'", value))?;

        match specifier.kind.as_str() {
            "npm" | "jsr" => {
                let name = specifier
                    .name
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("Missing package name in '{}'", value))?;

                requests.push(DependencyRequest {
                    kind: specifier.kind,
                    scope: specifier.scope,
                    name,
                    requirement: specifier.version,
                    dev,
                });
            }
            other => {
                Logger::warn(&format!(
                    "Skipping unsupported dependency kind '{}' for '{}'",
                    other,
                    value
                ));
            }
        }
    }

    Ok(requests)
}

fn dependency_requests_from_package(pkg: &ResolvedPackage) -> Vec<DependencyRequest> {
    pkg.dependencies
        .iter()
        .filter_map(|(dep_name, requirement)| {
            parse_npm_dependency_name(dep_name).map(|(scope, name)| DependencyRequest {
                kind: "npm".to_string(),
                scope,
                name,
                requirement: Some(requirement.clone()),
                dev: pkg.dev,
            })
        })
        .collect()
}

async fn resolve_dependency_request(request: &DependencyRequest) -> Result<ResolvedPackage> {
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
            let scope = request
                .scope
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("JSR package '{}' is missing scope", request.name))?;
            resolve_jsr_package(scope, &request.name, request.requirement.as_deref(), request.dev)
                .await
        }
        _ => Err(anyhow::anyhow!(
            "Unsupported dependency kind '{}'",
            request.kind
        )),
    }
}

async fn install_resolved_package(pkg: &ResolvedPackage, options: InstallOptions) -> Result<bool> {
    if !options.force && should_skip_reinstall(pkg.scope.as_deref(), &pkg.name, &pkg.version) {
        Logger::info(&format!(
            "Skipping {}@{} (already installed)",
            pkg.key,
            pkg.version
        ));
        return Ok(false);
    }

    let scope = pkg.scope.as_deref().unwrap_or("");
    download_tarball(&pkg.tarball, scope, &pkg.name)
        .await
        .with_context(|| format!("Failed to install {}", pkg.source))?;

    Ok(true)
}

fn finalize_lockfile(mut entries: Vec<LockPackage>) -> EspmLock {
    entries.sort_by_key(lock_key_for_sort);
    EspmLock {
        version: 1,
        packages: entries,
    }
}

fn build_install_queue(espm_json: &EspmJson, options: InstallOptions) -> Result<VecDeque<DependencyRequest>> {
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

async fn try_install_from_lockfile(base_dir: &Path, options: InstallOptions) -> Result<Option<usize>> {
    match read_lockfile(base_dir) {
        Ok(Some(lock)) => {
            Logger::info("Using lockfile for deterministic install.");
            write_lockfile(&lock, base_dir)?;
            let installed_count = install_from_lockfile(&lock, options).await?;
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

async fn handle_install_command(dev: bool, force: bool) -> Result<()> {
    let options = InstallOptions {
        include_dev: dev,
        force,
    };

    Logger::info(&format!(
        "Installing {} dependencies...",
        options.dependency_scope_label()
    ));

    let espm_json_path = get_espm_json_path().await?;
    // Read and parse espm.json
    let content = fs::read_to_string(&espm_json_path)
        .with_context(|| format!("Failed to read espm.json from {}", espm_json_path.display()))?;
    let espm_json: EspmJson = serde_json::from_str(&content).with_context(|| {
        format!(
            "Failed to parse espm.json from {}",
            espm_json_path.display()
        )
    })?;

    let base_dir = espm_json_path.parent().unwrap_or_else(|| Path::new("."));

    if let Some(installed_count) = try_install_from_lockfile(base_dir, options).await? {
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
        let resolved = resolve_dependency_request(&request)
            .await
            .with_context(|| format!("Failed to resolve {} package {}", request.kind, request.name))?;

        if let Some(existing_version) = installed_versions.get(&resolved.key) {
            if existing_version != &resolved.version {
                Logger::warn(&format!(
                    "Version conflict for {}: keeping {}, skipping {}",
                    resolved.key,
                    existing_version,
                    resolved.version
                ));
            }
            continue;
        }

        let installed_now = install_resolved_package(&resolved, options).await?;
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
}

async fn handle_init_command() -> Result<()> {
    // Create a new espm.json if it doesn't exist
    let espm_json_path = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("espm.json");
    let cwd = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));

    if !espm_json_path.exists() {
        let new_espm_json = EspmJson {
            name: Some(cwd.file_name().unwrap_or_default().to_string_lossy().to_string()),
            import_map: None,
            import_map_dev: None 
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

async fn handle_remove_command(package: String) -> Result<()> {
    let espm_json_path = get_espm_json_path().await?;
    let content = fs::read_to_string(&espm_json_path)?;
    let mut espm_json: EspmJson = serde_json::from_str(&content)?;

    let mut found = false;

    // Try to parse as a specifier, fallback to using as a plain package name
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

    // Remove from import_map
    if let Some(import_map) = &mut espm_json.import_map {
        for name in &possible_names {
            if import_map.imports.remove(name).is_some() {
                found = true;
            }
        }
        // Also try to remove by specifier string if present
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

    // Remove from import_map_dev
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
        // Remove from node_modules
        for name in &possible_names {
            let node_modules_path = Path::new("./node_modules");
            let pkg_path = node_modules_path.join(name);
            if pkg_path.exists() {
                if let Err(e) = fs::remove_dir_all(&pkg_path) {
                    Logger::warn(&format!("Failed to remove directory {:?}: {}", pkg_path, e));
                }
            }
            // If the package is scoped, remove the scope directory if empty
            if name.starts_with('@') {
                if let Some((scope, _)) = name.split_once('/') {
                    let scope_path = node_modules_path.join(scope);
                    if scope_path.exists() && scope_path.read_dir().map(|mut d| d.next().is_none()).unwrap_or(false) {
                        if let Err(e) = fs::remove_dir_all(&scope_path) {
                            Logger::warn(&format!("Failed to remove scope directory {:?}: {}", scope_path, e));
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

async fn handle_update_command(package: String) -> Result<()> {
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
                resolve_latest_npm_version(
                    parsed.scope.as_deref(),
                    name,
                    parsed.version.as_deref(),
                )
                .await?
            }
            _ => {
                Logger::warn(&format!(
                    "Skipping '{}' because '{}' dependencies are not updateable yet.",
                    dep_key,
                    parsed.kind
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
        download_package(&new_specifier, is_dev).await.with_context(|| {
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

#[tokio::main]
async fn main() -> Result<()> {
    // Handle HTTP or HTTPS proxy if defined in environment
    if let Ok(proxy) = env::var("HTTP_PROXY").or_else(|_| env::var("HTTPS_PROXY")) {
        env::set_var("HTTP_PROXY", &proxy);
        env::set_var("HTTPS_PROXY", &proxy);
        env::set_var("ALL_PROXY", &proxy);
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Add { specifier, dev } => {
            handle_add_command(specifier.clone(), dev).await?;
        }
        Commands::Install { dev, force } => handle_install_command(dev, force).await?,
        Commands::Init => handle_init_command().await?,
        Commands::Remove { package } => handle_remove_command(package).await?,
        Commands::Update { specifier } => handle_update_command(specifier).await?,
        Commands::Publish { .. } => {
            Logger::warn("The 'publish' command is not implemented yet.");
        }
        Commands::Setup { .. } => {
            Logger::warn("The 'setup' command is not implemented yet.");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let selected = select_latest_compatible_version(versions, Some("2.0.0"), Some("^1.0.0"))
            .unwrap();
        assert_eq!(selected, "1.2.0");
    }

    #[test]
    fn test_select_latest_compatible_version_uses_latest_tag_without_req() {
        let versions = vec!["1.0.0", "1.2.0", "1.3.0"]
            .into_iter()
            .map(String::from);
        let selected =
            select_latest_compatible_version(versions, Some("1.2.0"), None).unwrap();
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
        assert!(!should_skip_reinstall(None, "package-that-does-not-exist", "1.0.0"));
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
}
