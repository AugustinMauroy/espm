use super::*;
use std::collections::HashMap;

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
