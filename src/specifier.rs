use anyhow::Result;

#[derive(Debug)]
#[allow(dead_code)]
pub struct Specifier {
    pub source: String,
    pub kind: String,
    pub scope: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub path: Option<String>,
}

impl Specifier {
    pub fn from_string(source: &str) -> Result<Self> {
        let source = source.trim();
        if source.starts_with("jsr:") || source.starts_with("npm:") {
            let kind = if source.starts_with("jsr:") {
                "jsr"
            } else {
                "npm"
            }
            .to_string();
            let rest = &source[4..];

            if rest.is_empty() {
                return Err(anyhow::anyhow!("Empty package specifier: {}", source));
            }

            let mut version: Option<String> = None;
            let package_name_full: &str;

            if let Some(last_at_pos) = rest.rfind('@') {
                if last_at_pos > 0 {
                    let potential_name = &rest[..last_at_pos];
                    let potential_version = &rest[last_at_pos + 1..];
                    if !potential_version.is_empty() && !potential_version.contains('/') {
                        package_name_full = potential_name;
                        version = Some(potential_version.to_string());
                    } else {
                        package_name_full = rest;
                    }
                } else {
                    package_name_full = rest;
                }
            } else {
                package_name_full = rest;
            }

            let mut scope: Option<String> = None;
            let name_str = if let Some(stripped) = package_name_full.strip_prefix('@') {
                let parts: Vec<&str> = stripped.splitn(2, '/').collect();
                if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
                    scope = Some(parts[0].to_string());
                    parts[1].to_string()
                } else {
                    return Err(anyhow::anyhow!(
                        "Invalid scoped package name format: {}. Expected @scope/name.",
                        package_name_full
                    ));
                }
            } else {
                if package_name_full.is_empty() || package_name_full.contains('/') {
                    return Err(anyhow::anyhow!(
                        "Invalid unscoped package name: {}. Cannot be empty or contain '/'.",
                        package_name_full
                    ));
                }
                package_name_full.to_string()
            };
            Ok(Specifier {
                source: source.to_string(),
                kind,
                scope,
                name: Some(name_str),
                version,
                path: None,
            })
        } else if source.starts_with("file:") {
            if let Some(stripped) = source.strip_prefix("file:") {
                Ok(Specifier {
                    source: source.to_string(),
                    kind: "file".to_string(),
                    scope: None,
                    name: None,
                    version: None,
                    path: Some(stripped.to_string()),
                })
            } else {
                Err(anyhow::anyhow!("Invalid specifier format: {}", source))
            }
        } else if source.starts_with("http://") || source.starts_with("https://") {
            let kind = if source.starts_with("https://") {
                "https"
            } else {
                "http"
            }
            .to_string();

            Ok(Specifier {
                source: source.to_string(),
                kind,
                scope: None,
                name: None,
                version: None,
                path: None,
            })
        } else {
            Err(anyhow::anyhow!("Invalid specifier format: {}", source))
        }
    }
}

pub fn jsr_package_to_npm_package(scope: &str, name: &str) -> String {
    format!("@jsr/{}__{}", scope.trim_start_matches('@'), name)
}

pub fn npm_tarball_url(scope: Option<&str>, name: &str, version: &str) -> String {
    if let Some(scope) = scope {
        let normalized_scope = scope.trim_start_matches('@');
        format!(
            "https://registry.npmjs.org/@{}/{}/-/{}-{}.tgz",
            normalized_scope, name, name, version
        )
    } else {
        format!(
            "https://registry.npmjs.org/{}/-/{}-{}.tgz",
            name, name, version
        )
    }
}

pub fn parse_npm_dependency_name(input: &str) -> Option<(Option<String>, String)> {
    if input.starts_with('@') {
        if let Some(stripped) = input.strip_prefix('@') {
            let parts: Vec<&str> = stripped.splitn(2, '/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Some((Some(parts[0].to_string()), parts[1].to_string()));
        }
        }
        return None;
    }

    if input.is_empty() || input.contains('/') {
        return None;
    }

    Some((None, input.to_string()))
}

pub fn requested_specifier_from_parts(
    kind: &str,
    scope: Option<&str>,
    name: &str,
    version: &str,
) -> String {
    match kind {
        "jsr" => format!("jsr:@{}/{}@{}", scope.unwrap_or_default(), name, version),
        "npm" => {
            if let Some(s) = scope {
                format!("npm:@{}/{}@{}", s, name, version)
            } else {
                format!("npm:{}@{}", name, version)
            }
        }
        _ => format!("{}:{}@{}", kind, name, version),
    }
}

#[cfg(test)]
#[path = "specifier.test.rs"]
mod tests;
