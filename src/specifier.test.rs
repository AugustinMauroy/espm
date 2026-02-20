use super::*;

#[test]
fn parse_jsr_specifier() {
    let parsed = Specifier::from_string("jsr:@scope/pkg@1.2.3").unwrap();
    assert_eq!(parsed.kind, "jsr");
    assert_eq!(parsed.scope.as_deref(), Some("scope"));
    assert_eq!(parsed.name.as_deref(), Some("pkg"));
    assert_eq!(parsed.version.as_deref(), Some("1.2.3"));
}

#[test]
fn parse_npm_scoped_without_version() {
    let parsed = Specifier::from_string("npm:@types/node").unwrap();
    assert_eq!(parsed.kind, "npm");
    assert_eq!(parsed.scope.as_deref(), Some("types"));
    assert_eq!(parsed.name.as_deref(), Some("node"));
    assert!(parsed.version.is_none());
}

#[test]
fn parse_file_specifier() {
    let parsed = Specifier::from_string("file:../local").unwrap();
    assert_eq!(parsed.kind, "file");
    assert_eq!(parsed.path.as_deref(), Some("../local"));
}

#[test]
fn invalid_specifier_returns_error() {
    assert!(Specifier::from_string("invalid").is_err());
}

#[test]
fn helper_functions_behavior() {
    assert_eq!(
        jsr_package_to_npm_package("scope", "my-pkg"),
        "@jsr/scope__my-pkg"
    );
    assert_eq!(
        npm_tarball_url(Some("types"), "node", "20.0.0"),
        "https://registry.npmjs.org/@types/node/-/node-20.0.0.tgz"
    );
    assert_eq!(
        parse_npm_dependency_name("@types/node"),
        Some((Some("types".to_string()), "node".to_string()))
    );
    assert_eq!(
        requested_specifier_from_parts("npm", Some("types"), "node", "20.0.0"),
        "npm:@types/node@20.0.0"
    );
}
