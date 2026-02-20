use super::*;

#[test]
fn parse_install_with_force() {
    let cli = Cli::try_parse_from(["espm", "install", "--dev", "--force"]).unwrap();
    match cli.command {
        Commands::Install { dev, force, .. } => {
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
        Commands::Add { specifier, dev, .. } => {
            assert_eq!(specifier, "npm:lodash@4.17.21");
            assert!(dev);
        }
        _ => panic!("Expected add command"),
    }
}
