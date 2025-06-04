use std::fs;
use std::path::Path;
use std::env;
use std::io::Cursor;
use serde::{Deserialize, Serialize};
use colored::Colorize;
use reqwest::Client;
use flate2::read::GzDecoder;
use tar::Archive;
use anyhow::{Result, anyhow, Context};

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

// Import map structure equivalent to the TypeScript type
#[derive(Serialize, Deserialize, Debug)]
struct ImportMap {
    imports: std::collections::HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scopes: Option<std::collections::HashMap<String, std::collections::HashMap<String, String>>>,
}

// Constants for testing
const PACKAGE_SCOPE: &str = "@am";
const PACKAGE_NAME: &str = "neuralnetwork";
const PACKAGE_VERSION: &str = "1.0.0";

// Convert JSR package name to NPM package name
fn jsr_package_to_npm_package(scope: &str, name: &str) -> String {
    format!(
        "@jsr/{}__{}", 
        scope.replace('-', "__").replace('@', ""), 
        name.replace('-', "__")
    )
}

// Create directory if it doesn't exist
async fn create_directory_if_not_exists(dir: &str) -> Result<()> {
    let path = Path::new(dir);
    if !path.exists() {
        fs::create_dir_all(path)
            .with_context(|| format!("Failed to create directory: {}", dir))?;
    }
    Ok(())
}

// Download and extract a tarball
async fn download_tarball(tarball_url: &str, scope: &str, name: &str) -> Result<()> {
    let client = Client::new();
    let response = client.get(tarball_url).send().await?;
    
    if !response.status().is_success() {
        return Err(anyhow!("Failed to download tarball: {} {}", 
            response.status().as_u16(), 
            response.status().to_string()));
    }

    // Ensure the directory exists
    let package_dir = format!("./node_modules/{}/{}", scope, name);
    create_directory_if_not_exists(&package_dir).await?;

    // Get response bytes and extract tarball
    let bytes = response.bytes().await?;
    let tar = GzDecoder::new(Cursor::new(bytes));
    let mut archive = Archive::new(tar);
    archive.unpack(&package_dir)?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Handle HTTP proxy if defined in environment
    if let Ok(proxy) = env::var("HTTP_PROXY") {
        env::set_var("HTTPS_PROXY", &proxy);
        env::set_var("ALL_PROXY", &proxy);
    }

    let api_url = format!(
        "https://npm.jsr.io/{}", 
        jsr_package_to_npm_package(PACKAGE_SCOPE, PACKAGE_NAME)
    );

    let mut is_fetched = false;
    for _ in 0..10 {
        if is_fetched {
            break;
        }

        match async {
            let client = Client::new();
            let response = client.get(&api_url).send().await?;
            let data: serde_json::Value = response.json().await?;
            
            let version_data = &data["versions"][PACKAGE_VERSION];
            if version_data.is_null() {
                return Err(anyhow!("Version not found"));
            }
            
            let tarball_url = version_data["dist"]["tarball"].as_str()
                .ok_or_else(|| anyhow!("Tarball URL not found"))?;
            
            download_tarball(tarball_url, PACKAGE_SCOPE, PACKAGE_NAME).await?;
            
            Logger::success(&format!("Downloaded tarball for version {}", 
                PACKAGE_VERSION.magenta().bold()));
            
            Ok::<(), anyhow::Error>(())
        }.await {
            Ok(_) => {
                is_fetched = true;
            },
            Err(e) => {
                if e.to_string().contains("Version not found") {
                    Logger::error(&format!("Version {} not found for package {}", 
                        PACKAGE_VERSION.magenta().bold(), 
                        PACKAGE_NAME.magenta().bold()));
                    break;
                }
                Logger::error(&e.to_string());
            }
        }
    }

    Ok(())
}
