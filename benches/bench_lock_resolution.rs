#![feature(test)]
extern crate test;
extern crate espm;

use test::Bencher;
use std::collections::{HashMap, VecDeque};

use espm::installer::select_latest_compatible_version;
use espm::specifier::parse_npm_dependency_name;
use espm::models::{ResolvedPackage, DependencyRequest};

fn make_versions(n: usize) -> Vec<String> {
    (1..=n).map(|i| format!("0.{}.0", i)).collect()
}

fn make_registry_entry(versions: &[String]) -> (Vec<String>, HashMap<String, HashMap<String, String>>, Option<String>) {
    let mut deps_map: HashMap<String, HashMap<String, String>> = HashMap::new();
    for v in versions {
        // each version depends on dep-a and dep-b at specific versions
        let mut deps = HashMap::new();
        deps.insert("dep-a".to_string(), "^0.1.0".to_string());
        deps.insert("dep-b".to_string(), "0.2.0".to_string());
        deps_map.insert(v.clone(), deps);
    }

    // latest tag points to the highest version
    let latest = versions.last().cloned();
    (versions.to_vec(), deps_map, latest)
}

#[bench]
fn bench_simulated_lock_resolution(b: &mut Bencher) {
    // Build a simulated registry for many packages
    let pkg_count = 30usize;
    let versions_per_pkg = 12usize;

    let mut registry_versions: HashMap<String, Vec<String>> = HashMap::new();
    let mut registry_deps: HashMap<String, HashMap<String, HashMap<String, String>>> = HashMap::new();
    let mut registry_latest: HashMap<String, String> = HashMap::new();

    for i in 0..pkg_count {
        let name = format!("pkg{}", i);
        let versions = make_versions(versions_per_pkg);
        let (vers, deps_map, latest) = make_registry_entry(&versions);
        registry_versions.insert(name.clone(), vers.clone());
        registry_deps.insert(name.clone(), deps_map);
        if let Some(lat) = latest { registry_latest.insert(name, lat); }
    }

    // ensure dependency packages exist in the registry (no deps)
    let dep_versions = make_versions(4);
    let mut dep_deps_map: HashMap<String, HashMap<String, String>> = HashMap::new();
    for v in &dep_versions {
        dep_deps_map.insert(v.clone(), HashMap::new());
    }
    registry_versions.insert("dep-a".to_string(), dep_versions.clone());
    registry_deps.insert("dep-a".to_string(), dep_deps_map.clone());
    if let Some(lat) = dep_versions.last().cloned() { registry_latest.insert("dep-a".to_string(), lat); }

    registry_versions.insert("dep-b".to_string(), dep_versions.clone());
    registry_deps.insert("dep-b".to_string(), dep_deps_map.clone());
    if let Some(lat) = dep_versions.last().cloned() { registry_latest.insert("dep-b".to_string(), lat); }

    // initial requests: top-level depends on some pkgs with various requirements
    let mut initial_requests: Vec<DependencyRequest> = Vec::new();
    for i in 0..10 {
        let name = format!("pkg{}", i);
        initial_requests.push(DependencyRequest {
            source: format!("npm:{}", name),
            kind: "npm".to_string(),
            scope: None,
            name: name.clone(),
            requirement: if i % 2 == 0 { Some("^0.3.0".to_string()) } else { None },
            dev: false,
        });
    }

    b.iter(|| {
        // perform a breadth-first resolution using only in-memory registry
        let mut resolved: HashMap<String, ResolvedPackage> = HashMap::new();
        let mut queue: VecDeque<DependencyRequest> = initial_requests.clone().into();

        while let Some(req) = queue.pop_front() {
            if resolved.contains_key(&req.name) {
                continue;
            }

            // lookup available versions
            let versions = registry_versions.get(&req.name).cloned().unwrap_or_default();
            let latest_tag = registry_latest.get(&req.name).map(String::as_str);

            // call the exact function used in real resolution
            let chosen = select_latest_compatible_version(versions.into_iter(), latest_tag, req.requirement.as_deref()).expect("no version");

            // get dependencies for that version from our simulated registry
            let deps = registry_deps
                .get(&req.name)
                .and_then(|m| m.get(&chosen))
                .cloned()
                .unwrap_or_default();

            let rp = ResolvedPackage {
                kind: "npm".to_string(),
                key: format!("npm:{}", req.name),
                source: format!("npm:{}@{}", req.name, chosen),
                scope: None,
                name: req.name.clone(),
                requested: req.requirement.clone(),
                version: chosen.clone(),
                tarball: format!("https://registry/{}/-/-{}.tgz", req.name, chosen),
                dependencies: deps.clone(),
                dev: req.dev,
            };

            // push its dependency requests
            for (dep_name, req_str) in deps.iter() {
                if let Some((scope, name)) = parse_npm_dependency_name(dep_name) {
                    queue.push_back(DependencyRequest {
                        source: format!("npm:{}", dep_name),
                        kind: "npm".to_string(),
                        scope,
                        name,
                        requirement: Some(req_str.clone()),
                        dev: req.dev,
                    });
                }
            }

            resolved.insert(req.name.clone(), rp);
        }

        test::black_box(resolved);
    });
}
