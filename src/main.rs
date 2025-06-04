use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;
use flate2::read::GzDecoder;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::Path;
use tar::Archive;

struct Logger;

#[allow(dead_code)]
impl Logger {
    fn info(message: &str) {
        println!("{} {}", "[INFO]".cyan().bold(), message);
    }

    fn warn(message: &str) {
        eprintln!("{} {}", "[WARN]".yellow().bold(), message);
    }

    fn error(message: &str) {
        eprintln!("{} {}", "[ERROR]".red().bold(), message);
    }

    fn debug(message: &str) {
        println!("{} {}", "[DEBUG]".blue().bold(), message);
    }

    fn success(message: &str) {
        println!("{} {}", "[SUCCESS]".green().bold(), message);
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct ImportMap {
    imports: std::collections::HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scopes: Option<std::collections::HashMap<String, std::collections::HashMap<String, String>>>,
}

#[derive(Debug)]
#[allow(dead_code)]
struct Specifier {
    source: String, // e.g., "jsr:@scope/pkg@version", "npm:pkg@version", "file:../path", "http(s)://url/pkg.tgz"
    kind: String,   // "jsr", "npm", "file", "http"
    scope: Option<String>, // e.g., "@scope" for JSR/NPM packages
    name: Option<String>, // e.g., "pkg" for JSR/NPM packages
    version: Option<String>, // e.g., "0.220.0" for JSR/NPM packages if kind is "jsr" or "npm" and not specified => consider latest
    path: Option<String>,    // e.g., "../path" for file dependencies
}

impl Specifier {
    fn from_string(source: &str) -> Result<Self> {
        let source = source.trim();
        if source.starts_with("jsr:") || source.starts_with("npm:") {
            let kind = if source.starts_with("jsr:") {
                "jsr"
            } else {
                "npm"
            }
            .to_string();
            let rest = &source[4..];

            if rest.is_empty() {
                return Err(anyhow::anyhow!("Empty package specifier: {}", source));
            }

            let mut version: Option<String> = None;
            let package_name_full: &str;

            // Try to split by the last '@' for version.
            // Ensure it's not the first character (for scoped packages like @scope/name)
            // and that the part after '@' doesn't contain '/' (versions typically don't).
            if let Some(last_at_pos) = rest.rfind('@') {
                if last_at_pos > 0 {
                    // Check if '@' is not the first character
                    let potential_name = &rest[..last_at_pos];
                    let potential_version = &rest[last_at_pos + 1..];
                    if !potential_version.is_empty() && !potential_version.contains('/') {
                        package_name_full = potential_name;
                        version = Some(potential_version.to_string());
                    } else {
                        // The '@' is likely part of the package name or an invalid version format
                        package_name_full = rest;
                    }
                } else {
                    // The '@' is the first character, so it's part of a scoped package name
                    package_name_full = rest;
                }
            } else {
                // No '@' found (or only at the beginning), so the whole string is the package name part
                package_name_full = rest;
            }

            let mut scope: Option<String> = None;
            let name_str: String;

            if package_name_full.starts_with('@') {
                // Scoped package: @scope/name
                // Remove leading '@' and split by the first '/'
                let parts: Vec<&str> = package_name_full[1..].splitn(2, '/').collect();
                if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
                    scope = Some(parts[0].to_string());
                    name_str = parts[1].to_string();
                } else {
                    return Err(anyhow::anyhow!(
                        "Invalid scoped package name format: {}. Expected @scope/name.",
                        package_name_full
                    ));
                }
            } else {
                // Unscoped package: name
                if package_name_full.is_empty() || package_name_full.contains('/') {
                    return Err(anyhow::anyhow!(
                        "Invalid unscoped package name: {}. Cannot be empty or contain '/'.",
                        package_name_full
                    ));
                }
                name_str = package_name_full.to_string();
            }

            return Ok(Specifier {
                source: source.to_string(),
                kind,
                scope,
                name: Some(name_str),
                version,
                path: None,
            });
        } else if source.starts_with("file:") {
            return Ok(Specifier {
                source: source.to_string(),
                kind: "file".to_string(),
                scope: None,
                name: None,
                version: None,
                path: Some(source[5..].to_string()),
            });
        } else if source.starts_with("http://") || source.starts_with("https://") {
            let kind = if source.starts_with("https://") {
                "https"
            } else {
                "http"
            }
            .to_string();

            return Ok(Specifier {
                source: source.to_string(),
                kind,
                scope: None,
                name: None,
                version: None,
                path: None,
            });
        } else {
            return Err(anyhow::anyhow!("Invalid specifier format: {}", source));
        }
    }
}

// Convert JSR package name to NPM package name
fn jsr_package_to_npm_package(scope: &str, name: &str) -> String {
    format!(
        "@jsr/{}__{}",
        scope.replace('-', "__").replace('@', ""),
        name.replace('-', "__")
    )
}

async fn create_directory_if_not_exists(dir: &str) -> Result<()> {
    let path = Path::new(dir);
    if !path.exists() {
        fs::create_dir_all(path).with_context(|| format!("Failed to create directory: {}", dir))?;
    }
    Ok(())
}

async fn download_tarball(tarball_url: &str, scope: &str, name: &str) -> Result<()> {
    let client = Client::new();
    let response = client.get(tarball_url).send().await?;
    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to download tarball from {}: {}",
            tarball_url,
            response.status()
        ));
    }
    let content = response.bytes().await?;

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
    create_directory_if_not_exists(&target_dir).await?;

    println!(
        "Unpacking tarball for {}/{} into {}",
        scope, name, target_dir
    );

    // Extract the tarball into the target directory, stripping the first component (usually "package/")
    for entry in archive.entries().with_context(|| format!("Failed to read entries from tarball {}", tarball_url))? {
        let mut entry = entry?;
        let path = entry.path()?;
        let mut components = path.components();
        components.next(); // Strip the first component ("package" or similar)
        let stripped_path: std::path::PathBuf = components.as_path().to_path_buf();
        let out_path = Path::new(&target_dir).join(&stripped_path);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("Failed to create directory: {:?}", parent))?;
        }
        entry.unpack(&out_path).with_context(|| format!("Failed to unpack entry to {:?}", out_path))?;
    }


    Ok(())
}

/// espm - ECMAScript Package Manager
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Parser, Debug)]
enum Commands {
    // Add a package dependency
    #[clap(name = "add", about = "Add a package dependency")]
    Add {
        // The package source (e.g., jsr:@scope/pkg@version, npm:pkg@version, file:../path, http(s)://url/pkg.tgz)
        #[clap(value_parser, required = true)]
        specifier: String,

        /// Add as a development dependency
        #[clap(short, long, default_value = "false")]
        dev: bool,
    },
    // install prod dependencies listed in espm.json if dev is false, or all dependencies if dev is true
    #[clap(name = "install", about = "Install dependencies")]
    Install {
        // install all dependencies
        #[clap(short, long, default_value = "false")]
        dev: bool,
    },
}

async fn handle_add_command(specifier: String, is_dev: bool) -> Result<()> {
    let specifier = Specifier::from_string(&specifier)
        .with_context(|| format!("Failed to parse specifier: {}", specifier))?;

    match specifier.kind.as_str() {
        "jsr" => {
            let scope = specifier.scope.as_deref().unwrap_or("default");
            let name = specifier.name.as_deref().unwrap_or("unknown");
            let version = specifier.version.as_deref().unwrap_or("latest");

            // Convert JSR package name to NPM package name if needed
            let npm_package_name = jsr_package_to_npm_package(scope, name);
            Logger::info(&format!(
                "Adding package {}@{} as {}",
                npm_package_name.cyan(),
                version.bold(),
                if is_dev {
                    "development dependency"
                } else {
                    "dependency"
                }
            ));
            let npm_jsr_url = format!("https://npm.jsr.io/{}", npm_package_name);
            Logger::info(&format!(
                "Fetching package data from: {}",
                npm_jsr_url.cyan()
            ));

            let client = Client::new();
            let response =
                client.get(&npm_jsr_url).send().await.with_context(|| {
                    format!("Failed to fetch package data from {}", npm_jsr_url)
                })?;
            if !response.status().is_success() {
                return Err(anyhow::anyhow!(
                    "Failed to fetch package data: {}",
                    response.status()
                ));
            }

            let package_data: serde_json::Value = response
                .json()
                .await
                .with_context(|| format!("Failed to parse package data from {}", npm_jsr_url))?;
            let version_data = package_data
                .get("versions")
                .and_then(|v| v.get(version))
                .ok_or_else(|| anyhow::anyhow!("Version {} not found in package data", version))?;
            let tarball_url = version_data
                .get("dist")
                .and_then(|d| d.get("tarball"))
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow::anyhow!("Tarball URL not found in version data"))?;
            download_tarball(tarball_url, scope, name).await?;

            // todo(@AugustinMauroy): add logic to update espm.json with the new dependency

            return Ok(());
        }
        "npm" => {
            let name = specifier.name.as_deref().unwrap_or("unknown");
            let version = specifier.version.as_deref().unwrap_or("latest");

            if (version == "latest" || version.is_empty()) && !specifier.scope.is_some() {
                Logger::warn(&format!(
                    "Adding NPM package {} without a specific version is not supported yet. Please specify a version.",
                    name.cyan()
                ));
                return Ok(());
            }

            Logger::info(&format!(
                "Adding NPM package {}@{} as {}",
                name.cyan(),
                version.bold(),
                if is_dev {
                    "development dependency"
                } else {
                    "dependency"
                }
            ));

            
            let npm_package_url = if let Some(scope) = &specifier.scope {
                format!(
                    "https://registry.npmjs.org/@{}/{}/-/{}-{}.tgz",
                    scope.trim_start_matches('@'),
                    name,
                    name,
                    version
                )
            } else {
                format!(
                    "https://registry.npmjs.org/{}/-/{}-{}.tgz",
                    name, name, version
                )
            };
            Logger::info(&format!(
                "Fetching package tarball from: {}",
                npm_package_url.cyan()
            ));

            download_tarball(&npm_package_url, "", name).await?;

            // todo(@AugustinMauroy): add logic to update espm.json with the new dependency

            return Ok(());
        }
        "file" => {
            Logger::info("Adding file:// isn nott supported yet");
        }
        "http" | "https" => {
            Logger::info("Adding HTTP(S) is not supported yet");
        }
        _ => {
            return Err(anyhow::anyhow!(
                "Unsupported package kind: {}",
                specifier.kind
            ));
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Handle HTTP proxy if defined in environment
    if let Ok(proxy) = env::var("HTTP_PROXY") {
        Logger::debug(&format!("Using HTTP_PROXY: {}", proxy));
        env::set_var("HTTPS_PROXY", &proxy); // reqwest uses HTTPS_PROXY for https requests
        env::set_var("ALL_PROXY", &proxy); // Some tools might use this
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Add { specifier, dev } => {
            match handle_add_command(specifier.clone(), dev).await {
                Ok(_) => Logger::success(&format!(
                    "Successfully processed 'add' command for '{}'",
                    specifier
                )),
                Err(e) => Logger::error(&format!(
                    "Error processing 'add' command for '{}': {}",
                    specifier, e
                )),
            }
        }
        Commands::Install { dev } => {
            Logger::info(&format!("Installing dependencies (dev: {})", dev));
            // Placeholder for install logic
            // This would typically read from espm.json and install dependencies accordingly
            if dev {
                Logger::info("Installing development dependencies...");
            } else {
                Logger::info("Installing production dependencies...");
            }
            // Simulate installation process
            Logger::success("All dependencies installed successfully (simulated).");
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
        assert_eq!(npm_name, "@jsr/scope__my__pkg");
    }
}
