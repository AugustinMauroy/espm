use clap::Parser;

#[derive(Parser, Debug)]
#[clap(version, about= "espm - ECMAScript Package Manager", long_about = None)]
pub struct Cli {
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
    },
    #[clap(name = "remove", about = "Remove a dependency")]
    Remove {
        package: String,
    },
    #[clap(name = "install", about = "Install dependencies")]
    Install {
        #[clap(short, long, default_value = "false")]
        dev: bool,

        #[clap(long, default_value = "false")]
        force: bool,
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
    },
    #[clap(name = "setup", about = "Use the right version of espm")]
    Setup {
        #[clap(long, default_value = "latest")]
        version: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_install_with_force() {
        let cli = Cli::try_parse_from(["espm", "install", "--dev", "--force"]).unwrap();
        match cli.command {
            Commands::Install { dev, force } => {
                assert!(dev);
                assert!(force);
            }
            _ => panic!("Expected install command"),
        }
    }

    #[test]
    fn parse_add_command() {
        let cli = Cli::try_parse_from(["espm", "add", "npm:lodash@4.17.21", "--dev"]).unwrap();
        match cli.command {
            Commands::Add { specifier, dev } => {
                assert_eq!(specifier, "npm:lodash@4.17.21");
                assert!(dev);
            }
            _ => panic!("Expected add command"),
        }
    }
}
