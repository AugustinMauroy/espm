use clap::Parser;

#[derive(Parser, Debug)]
#[clap(version, about= "espm - ECMAScript Package Manager", long_about = None)]
pub struct Cli {
    /// Print debug messages while the command runs
    #[clap(short, long, global = true, default_value = "false")]
    pub verbose: bool,

    #[clap(subcommand)]
    pub command: Commands,
}

#[derive(Parser, Debug)]
pub enum Commands {
    #[clap(name = "add", about = "Add a package dependency")]
    Add {
        #[clap(value_parser, required = true)]
        specifier: String,

        #[clap(short, long, default_value = "false")]
        dev: bool,

        /// Require packages to be ESM
        #[clap(long, default_value = "false")]
        require_esm: bool,
    },
    #[clap(name = "remove", about = "Remove a dependency")]
    Remove { package: String },
    #[clap(name = "install", about = "Install dependencies")]
    Install {
        #[clap(short, long, default_value = "false")]
        dev: bool,

        #[clap(long, default_value = "false")]
        force: bool,

        /// Require packages to be ESM
        #[clap(long, default_value = "false")]
        require_esm: bool,
    },
    #[clap(name = "update", about = "Update a package dependency")]
    Update {
        #[clap(value_parser, required = true)]
        specifier: String,
    },
    #[clap(name = "init", about = "Initialize espm.json")]
    Init,
    #[clap(name = "publish", about = "Publish a package")]
    Publish {
        #[clap(long, default_value = "false")]
        npm: bool,

        /// Prepare the publish payload without sending it
        #[clap(short = 'n', long, default_value = "false")]
        dry_run: bool,
    },
    #[clap(name = "setup", about = "Use the right version of espm")]
    Setup {
        #[clap(long, default_value = "latest")]
        version: String,
    },
}

#[cfg(test)]
#[path = "cli.test.rs"]
mod tests;
