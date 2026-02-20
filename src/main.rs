use clap::Parser;
mod cli;
mod jsr_npm;
mod logger;
mod models;
mod specifier;
mod installer;

use cli::{Cli, Commands};
use logger::Logger;
pub use anyhow::{Context, Result};
pub use colored::Colorize;
pub use std::env;
pub use std::fs;
pub use std::path::{Path, PathBuf};
pub use std::collections::HashMap;
pub use models::{EspmJson, EspmLock, LockPackage, InstallOptions, ImportMap, ResolvedPackage, DependencyRequest};
pub use specifier::{Specifier, jsr_package_to_npm_package, npm_tarball_url, parse_npm_dependency_name, requested_specifier_from_parts};
pub use installer::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Ok(proxy) = std::env::var("HTTP_PROXY").or_else(|_| std::env::var("HTTPS_PROXY")) {
        std::env::set_var("HTTP_PROXY", &proxy);
        std::env::set_var("HTTPS_PROXY", &proxy);
        std::env::set_var("ALL_PROXY", &proxy);
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Add { specifier, dev, require_esm } => {
            let require_esm_env = std::env::var("ESPM_REQUIRE_ESM").map(|v| v == "1" || v.to_lowercase() == "true").unwrap_or(false);
            let effective = require_esm || require_esm_env;
            installer::handle_add_command(specifier.clone(), dev, effective).await?;
        }
        Commands::Install { dev, force, require_esm } => {
            let require_esm_env = std::env::var("ESPM_REQUIRE_ESM").map(|v| v == "1" || v.to_lowercase() == "true").unwrap_or(false);
            let effective = require_esm || require_esm_env;
            installer::handle_install_command(dev, force, effective).await?;
        }
        Commands::Init => installer::handle_init_command().await?,
        Commands::Remove { package } => installer::handle_remove_command(package).await?,
        Commands::Update { specifier } => installer::handle_update_command(specifier).await?,
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
#[path = "main.test.rs"]
mod tests;
