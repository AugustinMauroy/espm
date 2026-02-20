use anyhow::{Context, Result, anyhow};
use flate2::Compression;
use flate2::write::GzEncoder;
use reqwest::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tar::Builder;

use crate::installer::read_package_manifest_from_dir;
use crate::logger::Logger;

const NPM_REGISTRY_BASE: &str = "https://registry.npmjs.org/";
const JSR_REGISTRY_BASE: &str = "https://npm.jsr.io/";
const DRY_RUN_TARBALL: &str = "espm-publish.tgz";

pub fn pack_current_package(dir: &Path) -> Result<Vec<u8>> {
    Logger::debug(&format!("pack_current_package dir={}", dir.display()));

    let package_json_path = dir.join("package.json");
    if !package_json_path.exists() {
        return Err(anyhow!(
            "package.json not found at {}. Cannot publish without package metadata.",
            package_json_path.display()
        ));
    }

    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut builder = Builder::new(encoder);

    append_dir_filtered(&mut builder, dir, dir)?;

    let encoder = builder
        .into_inner()
        .context("Failed to finalize tar archive while packaging")?;
    let tarball = encoder
        .finish()
        .context("Failed to finish gzip compression while packaging")?;
    Logger::debug(&format!(
        "pack_current_package produced {} bytes",
        tarball.len()
    ));
    Ok(tarball)
}

fn append_dir_filtered(
    builder: &mut Builder<GzEncoder<Vec<u8>>>,
    base: &Path,
    dir: &Path,
) -> Result<()> {
    Logger::debug(&format!(
        "append_dir_filtered base={} dir={}",
        base.display(),
        dir.display()
    ));
    for entry in fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory {} while packaging", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str == "node_modules" || name_str == "target" || name_str == ".git" {
            continue;
        }

        let rel = path
            .strip_prefix(base)
            .with_context(|| format!("Failed to strip prefix for {}", path.display()))?;
        let archive_path: PathBuf = Path::new("package").join(rel);
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("Failed to read metadata for {}", path.display()))?;

        if metadata.is_dir() {
            builder.append_dir(&archive_path, &path).with_context(|| {
                format!("Failed to append directory {}", archive_path.display())
            })?;
            append_dir_filtered(builder, base, &path)?;
        } else if metadata.is_file() || metadata.file_type().is_symlink() {
            builder
                .append_path_with_name(&path, &archive_path)
                .with_context(|| format!("Failed to append file {}", archive_path.display()))?;
        }
    }

    Ok(())
}

fn package_name_for_registry(scope: Option<&str>, name: &str, use_npm: bool) -> Result<String> {
    Logger::debug(&format!(
        "package_name_for_registry scope={:?} name={} use_npm={}",
        scope, name, use_npm
    ));
    if use_npm {
        Ok(match scope {
            Some(scope) => format!("@{}/{}", scope.trim_start_matches('@'), name),
            None => name.to_string(),
        })
    } else {
        let scope = scope.map(|s| s.trim_start_matches('@')).ok_or_else(|| {
            anyhow!(
                "JSR registry requires scoped package names like @scope/{}",
                name
            )
        })?;
        Ok(format!("@{}/{}", scope, name))
    }
}

/// Perform a minimal publish request. Note: the official npm publish API expects
/// additional metadata; here we send the tarball bytes to the package endpoint
/// to avoid relying on undocumented flows during testing.
pub async fn publish_bytes_to_registry(
    registry_base: &str,
    package_name: &str,
    tarball: &[u8],
    token: &str,
) -> Result<()> {
    Logger::debug(&format!(
        "publish_bytes_to_registry registry={} package={} size={} token_present={}",
        registry_base,
        package_name,
        tarball.len(),
        !token.is_empty()
    ));
    let url = format!(
        "{}/{}",
        registry_base.trim_end_matches('/'),
        package_name.trim_start_matches('/')
    );

    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("Failed to build HTTP client for publish")?;

    let response = client
        .put(&url)
        .header(AUTHORIZATION, format!("Bearer {}", token))
        .header(CONTENT_TYPE, "application/octet-stream")
        .body(tarball.to_vec())
        .send()
        .await
        .with_context(|| format!("Failed to send publish request to {}", url))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if status.is_success() {
        Logger::debug(&format!(
            "publish_bytes_to_registry successful status={}",
            status
        ));
        Ok(())
    } else {
        Logger::debug(&format!(
            "publish_bytes_to_registry failed status={} body={}",
            status, body
        ));
        Err(anyhow!("Publish failed with status {}: {}", status, body))
    }
}

pub async fn publish_from_dir(dir: &Path, use_npm: bool, dry_run: bool) -> Result<()> {
    Logger::debug(&format!(
        "publish_from_dir dir={} use_npm={} dry_run={}",
        dir.display(),
        use_npm,
        dry_run
    ));

    let (scope, name, version) = read_package_manifest_from_dir(dir)
        .with_context(|| format!("Failed to read package.json from {}", dir.display()))?;
    let package_name = package_name_for_registry(scope.as_deref(), &name, use_npm)?;

    let tarball = pack_current_package(dir)
        .with_context(|| format!("Failed to package directory {}", dir.display()))?;

    if dry_run {
        Logger::debug("publish_from_dir performing dry_run path write");
        let output_path = dir.join(DRY_RUN_TARBALL);
        fs::write(&output_path, &tarball)
            .with_context(|| format!("Failed to write tarball to {}", output_path.display()))?;

        let version_suffix = version
            .as_deref()
            .map(|v| format!("@{}", v))
            .unwrap_or_default();

        Logger::info(&format!(
            "Dry-run: packed {}{} ({} bytes) to {}.",
            package_name,
            version_suffix,
            tarball.len(),
            output_path.display()
        ));
        Logger::success("Dry-run complete. No network requests were made.");
        return Ok(());
    }

    let token_var = if use_npm { "NPM_TOKEN" } else { "JSR_TOKEN" };
    let token = env::var(token_var).map_err(|_| {
        anyhow!(
            "{} is required to publish. Set it or use --dry-run to skip network requests.",
            token_var
        )
    })?;

    let registry_base = if use_npm {
        NPM_REGISTRY_BASE
    } else {
        JSR_REGISTRY_BASE
    };

    Logger::info(&format!(
        "Publishing {} to {} ({} bytes)...",
        package_name,
        registry_base,
        tarball.len()
    ));

    Logger::debug("about to call publish_bytes_to_registry");
    publish_bytes_to_registry(registry_base, &package_name, &tarball, &token).await?;
    Logger::success(&format!("Publish request sent for {}", package_name));
    Ok(())
}

#[cfg(test)]
#[path = "publisher.test.rs"]
mod tests;
