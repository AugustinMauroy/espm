use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct JsrNpmDist {
    pub tarball: String,
    pub shasum: Option<String>,
    pub integrity: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsrNpmVersionInfo {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub dist: JsrNpmDist,
    pub dependencies: Option<std::collections::HashMap<String, String>>,
}

// https://npm.jsr.io/@jsr/<scope>__<name>
#[derive(Debug, Serialize, Deserialize)]
pub struct JsrNpmRegistryResponse {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "dist-tags")]
    pub dist_tags: std::collections::HashMap<String, String>,
    pub versions: std::collections::HashMap<String, JsrNpmVersionInfo>,
    pub time: std::collections::HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NPMAuthor {
    pub name: Option<String>,
    pub email: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NPMBugs {
    pub url: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NPMRepository {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub url: Option<String>,
    pub directory: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NPMDist {
    pub integrity: Option<String>,
    pub shasum: Option<String>,
    pub tarball: String,
    #[serde(rename = "fileCount")]
    pub file_count: Option<u32>,
    #[serde(rename = "unpackedSize")]
    pub unpacked_size: Option<u64>,
    #[serde(rename = "npm-signature")]
    pub npm_signature: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NPMUser {
    pub name: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NPMVersionInfo {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub keywords: Option<Vec<String>>,
    pub author: Option<NPMAuthor>,
    #[serde(rename = "_id")]
    pub id: String,
    pub homepage: Option<String>,
    pub repository: Option<NPMRepository>,
    pub bugs: Option<NPMBugs>,
    pub license: Option<String>, // SPDX identifier or custom string
    pub licenses: Option<Vec<serde_json::Value>>, // For arrays of license objects/strings
    pub main: Option<String>,
    pub bin: Option<serde_json::Value>,
    pub engines: Option<serde_json::Value>,
    pub scripts: Option<std::collections::HashMap<String, String>>,
    pub dependencies: Option<std::collections::HashMap<String, String>>,
    #[serde(rename = "devDependencies")]
    pub dev_dependencies: Option<std::collections::HashMap<String, String>>,
    #[serde(rename = "optionalDependencies")]
    pub optional_dependencies: Option<std::collections::HashMap<String, String>>,
    pub dist: NPMDist,
    #[serde(rename = "_npmUser")]
    pub npm_user: Option<NPMUser>,
    #[serde(rename = "_npmVersion")]
    pub npm_version: Option<String>,
    #[serde(rename = "_nodeVersion")]
    pub node_version: Option<String>,
    pub maintainers: Option<Vec<NPMUser>>,
    #[serde(rename = "_defaultsLoaded")]
    pub defaults_loaded: Option<bool>,
    #[serde(rename = "_engineSupported")]
    pub engine_supported: Option<bool>,
    #[serde(rename = "_from")]
    pub from: Option<String>,
    #[serde(rename = "_shasum")]
    pub shasum: Option<String>,
    pub files: Option<Vec<String>>,
    pub icon: Option<String>,
    pub jam: Option<serde_json::Value>,
    pub volo: Option<serde_json::Value>,
    #[serde(rename = "gitHead")]
    pub git_head: Option<String>,
    #[serde(rename = "_npmOperationalInternal")]
    pub npm_operational_internal: Option<std::collections::HashMap<String, String>>,
    pub directories: Option<serde_json::Value>,
}

// https://registry.npmjs.org/<package>
#[derive(Debug, Serialize, Deserialize)]
pub struct NPMRegistryResponse {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_rev")]
    pub rev: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "dist-tags")]
    pub dist_tags: std::collections::HashMap<String, String>,
    pub versions: std::collections::HashMap<String, NPMVersionInfo>,
    pub time: std::collections::HashMap<String, String>,
    pub maintainers: Option<Vec<NPMUser>>,
    pub author: Option<NPMAuthor>,
    pub repository: Option<NPMRepository>,
    pub homepage: Option<String>,
    pub keywords: Option<Vec<String>>,
    pub bugs: Option<NPMBugs>,
    pub license: Option<String>,
    pub readme: Option<String>,
    #[serde(rename = "readmeFilename")]
    pub readme_filename: Option<String>,
    pub users: Option<std::collections::HashMap<String, bool>>,
    pub contributors: Option<Vec<NPMUser>>,
}

#[cfg(test)]
mod tests {
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
}
