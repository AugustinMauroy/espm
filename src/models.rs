use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
pub struct ImportMap {
    #[serde(skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub imports: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<HashMap<String, HashMap<String, String>>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EspmJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "is_import_map_empty")]
    pub import_map: Option<ImportMap>,
    #[serde(skip_serializing_if = "is_import_map_empty")]
    pub import_map_dev: Option<ImportMap>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct EspmLock {
    pub version: u32,
    pub packages: Vec<LockPackage>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LockPackage {
    pub id: String,
    pub source: String,
    pub resolved_version: String,
    pub tarball: String,
    pub requested: Option<String>,
    pub dev: bool,
}

#[derive(Clone, Debug)]
pub struct DependencyRequest {
    pub source: String,
    pub kind: String,
    pub scope: Option<String>,
    pub name: String,
    pub requirement: Option<String>,
    pub dev: bool,
}

#[derive(Clone, Debug)]
pub struct ResolvedPackage {
    pub kind: String,
    pub key: String,
    pub source: String,
    pub scope: Option<String>,
    pub name: String,
    pub requested: Option<String>,
    pub version: String,
    pub tarball: String,
    pub dependencies: HashMap<String, String>,
    pub dev: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct InstallOptions {
    pub include_dev: bool,
    pub force: bool,
}

impl InstallOptions {
    pub fn dependency_scope_label(&self) -> &'static str {
        if self.include_dev {
            "development"
        } else {
            "production"
        }
    }

    pub fn summary_suffix(&self) -> &'static str {
        if self.include_dev {
            " (prod + dev)"
        } else {
            ""
        }
    }
}

fn is_import_map_empty(map: &Option<ImportMap>) -> bool {
    match map {
        None => true,
        Some(m) => m.imports.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_options_labels() {
        let prod = InstallOptions {
            include_dev: false,
            force: false,
        };
        let with_dev = InstallOptions {
            include_dev: true,
            force: true,
        };

        assert_eq!(prod.dependency_scope_label(), "production");
        assert_eq!(prod.summary_suffix(), "");
        assert_eq!(with_dev.dependency_scope_label(), "development");
        assert_eq!(with_dev.summary_suffix(), " (prod + dev)");
    }

    #[test]
    fn espm_json_skips_empty_import_maps() {
        let data = EspmJson {
            name: Some("demo".to_string()),
            import_map: Some(ImportMap {
                imports: HashMap::new(),
                scopes: None,
            }),
            import_map_dev: None,
        };

        let serialized = serde_json::to_string(&data).unwrap();
        assert!(serialized.contains("name"));
        assert!(!serialized.contains("import_map"));
    }
}
