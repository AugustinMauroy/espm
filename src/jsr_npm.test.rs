use super::*;

#[test]
fn deserialize_jsr_registry_response() {
    let raw = r#"{
            "name": "@jsr/scope__pkg",
            "description": "demo",
            "dist-tags": { "latest": "1.0.0" },
            "versions": {
                "1.0.0": {
                    "name": "@jsr/scope__pkg",
                    "version": "1.0.0",
                    "description": "demo",
                    "dist": { "tarball": "https://example.com/pkg.tgz", "shasum": null, "integrity": null },
                    "dependencies": { "chalk": "^4.0.0" }
                }
            },
            "time": { "1.0.0": "2024-01-01T00:00:00.000Z" }
        }"#;

    let parsed: JsrNpmRegistryResponse = serde_json::from_str(raw).unwrap();
    assert_eq!(
        parsed.dist_tags.get("latest").map(String::as_str),
        Some("1.0.0")
    );
    assert!(parsed.versions.contains_key("1.0.0"));
}

#[test]
fn deserialize_npm_registry_response() {
    let raw = r#"{
            "_id": "chalk",
            "_rev": "1-abc",
            "name": "chalk",
            "description": "demo",
            "dist-tags": { "latest": "4.1.2" },
            "versions": {
                "4.1.2": {
                    "name": "chalk",
                    "version": "4.1.2",
                    "description": "demo",
                    "_id": "chalk@4.1.2",
                    "dist": { "tarball": "https://example.com/chalk.tgz", "integrity": null, "shasum": null },
                    "dependencies": { "ansi-styles": "^4.1.0" }
                }
            },
            "time": { "4.1.2": "2024-01-01T00:00:00.000Z" }
        }"#;

    let parsed: NPMRegistryResponse = serde_json::from_str(raw).unwrap();
    assert_eq!(parsed.name, "chalk");
    assert_eq!(
        parsed.dist_tags.get("latest").map(String::as_str),
        Some("4.1.2")
    );
    assert!(parsed.versions.contains_key("4.1.2"));
}
