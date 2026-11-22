use std::collections::{BTreeMap, BTreeSet};

use rtepl_repository::dependencies::{check_dependencies, DependencyPolicy};
use serde_json::{json, Value};

use super::Tree;

fn fixture() -> (Tree, Value, DependencyPolicy) {
    let tree = Tree::new();
    for file in [
        "core/Cargo.toml",
        "app/Cargo.toml",
        "tests/support/Cargo.toml",
    ] {
        tree.file(file);
    }
    let metadata = json!({"packages": [
        {"name": "core", "manifest_path": tree.0.join("core/Cargo.toml"), "dependencies": []},
        {"name": "app", "manifest_path": tree.0.join("app/Cargo.toml"), "dependencies": [
            {"name": "core", "path": tree.0.join("core"), "kind": null, "optional": true, "target": "cfg(unix)", "rename": "renamed_core"},
            {"name": "fixture", "path": tree.0.join("tests/support"), "kind": "dev"},
            {"name": "shared", "path": null, "kind": null}
        ]},
        {"name": "fixture", "manifest_path": tree.0.join("tests/support/Cargo.toml"), "dependencies": []}
    ]});
    let policy = DependencyPolicy {
        production: BTreeMap::from([
            ("core".into(), BTreeSet::new()),
            ("app".into(), BTreeSet::from(["core".into()])),
        ]),
        test_support: BTreeMap::from([("fixture".into(), "tests/support".into())]),
        forbidden: BTreeSet::from([("core".into(), "app".into())]),
    };
    (tree, metadata, policy)
}

fn check(tree: &Tree, metadata: &Value, policy: &DependencyPolicy) -> Vec<String> {
    check_dependencies(&tree.0, &serde_json::to_vec(metadata).unwrap(), policy).unwrap()
}

#[test]
fn includes_optional_target_build_and_renamed_edges_but_not_registry_or_git() {
    let (tree, mut metadata, policy) = fixture();
    assert!(check(&tree, &metadata, &policy).is_empty());
    metadata["packages"][1]["dependencies"][0]["kind"] = json!("build");
    assert!(check(&tree, &metadata, &policy).is_empty());
    metadata["packages"][1]["dependencies"][0]["kind"] = json!("dev");
    assert!(!check(&tree, &metadata, &policy).is_empty());
}

#[test]
fn rejects_extra_missing_and_duplicate_packages() {
    let (tree, metadata, policy) = fixture();
    for name in ["unexpected", "core"] {
        let mut changed = metadata.clone();
        changed["packages"][1]["name"] = json!(name);
        assert!(!check(&tree, &changed, &policy).is_empty());
    }
    let mut changed = metadata;
    changed["packages"].as_array_mut().unwrap().remove(0);
    assert!(!check(&tree, &changed, &policy).is_empty());
}

#[test]
fn rejects_all_fixture_support_and_forbidden_edge_regressions() {
    let (tree, metadata, policy) = fixture();
    tree.file("tests/support/nested/Cargo.toml");
    for manifest in [
        "tests/support/Cargo.toml",
        "tests/support/nested/Cargo.toml",
    ] {
        let mut occupied = metadata.clone();
        occupied["packages"][0]["manifest_path"] = json!(tree.0.join(manifest));
        assert!(check(&tree, &occupied, &policy)
            .iter()
            .any(|s| s.contains("reserved for test support")));
    }
    let mut wrong_scope = metadata.clone();
    wrong_scope["packages"][1]["dependencies"][1]["kind"] = Value::Null;
    assert!(check(&tree, &wrong_scope, &policy)
        .iter()
        .any(|s| s.contains("outside dev")));
    let mut fixture_dep = metadata.clone();
    fixture_dep["packages"][2]["dependencies"] =
        json!([{"name": "core", "path": tree.0.join("core"), "kind": "dev"}]);
    assert!(check(&tree, &fixture_dep, &policy)
        .iter()
        .any(|s| s.contains("test support has local")));
    let mut misplaced = metadata.clone();
    misplaced["packages"][2]["manifest_path"] = json!(tree.0.join("core/Cargo.toml"));
    assert!(check(&tree, &misplaced, &policy)
        .iter()
        .any(|s| s.contains("test-support directory")));
    let mut missing = metadata.clone();
    missing["packages"].as_array_mut().unwrap().pop();
    assert!(check(&tree, &missing, &policy)
        .iter()
        .any(|s| s.contains("missing test-support")));
    let mut duplicate = metadata.clone();
    duplicate["packages"]
        .as_array_mut()
        .unwrap()
        .push(metadata["packages"][2].clone());
    assert!(check(&tree, &duplicate, &policy)
        .iter()
        .any(|s| s.contains("duplicate test-support")));
    let mut forbidden = metadata;
    forbidden["packages"][0]["dependencies"] =
        json!([{"name": "app", "path": tree.0.join("app"), "kind": "dev"}]);
    assert!(check(&tree, &forbidden, &policy)
        .iter()
        .any(|s| s.contains("must not depend")));
}

#[test]
fn rejects_external_paths_and_malformed_or_unreadable_input() {
    let (tree, mut metadata, policy) = fixture();
    let outside = Tree::new();
    outside.file("Cargo.toml");
    metadata["packages"][1]["dependencies"][0]["path"] = json!(outside.0);
    assert!(check(&tree, &metadata, &policy)
        .iter()
        .any(|s| s.contains("external checkout")));
    metadata["packages"][0]["manifest_path"] = json!(outside.0.join("Cargo.toml"));
    assert!(check(&tree, &metadata, &policy)
        .iter()
        .any(|s| s.contains("manifest outside")));
    assert!(check_dependencies(&tree.0, b"{}", &policy).is_err());
    metadata["packages"][1]["dependencies"][0]["path"] = json!(tree.0.join("missing"));
    assert!(check_dependencies(&tree.0, &serde_json::to_vec(&metadata).unwrap(), &policy).is_err());
}

#[cfg(unix)]
#[test]
fn canonical_paths_expose_outside_dependencies_behind_links() {
    let (tree, mut metadata, policy) = fixture();
    let outside = Tree::new();
    std::os::unix::fs::symlink(&outside.0, tree.0.join("alias")).unwrap();
    metadata["packages"][1]["dependencies"][0]["path"] = json!(tree.0.join("alias"));
    assert!(check(&tree, &metadata, &policy)
        .iter()
        .any(|s| s.contains("external checkout")));
}

#[test]
fn external_allowlists_cover_optional_target_build_git_and_renamed_dependencies() {
    use rtepl_repository::dependencies::check_external_dependencies;
    let (_tree, metadata, _) = fixture();
    let policy = BTreeMap::from([
        ("core".into(), BTreeSet::new()),
        ("app".into(), BTreeSet::from(["shared".into()])),
    ]);
    let check = |metadata: &Value| {
        check_external_dependencies(&serde_json::to_vec(metadata).unwrap(), &policy).unwrap()
    };
    assert!(check(&metadata).is_empty());
    for dependency in [
        json!({"name":"runtime", "kind":null, "optional":true}),
        json!({"name":"randomness", "kind":"build", "target":"cfg(windows)"}),
        json!({"name":"network", "kind":null, "rename":"safe", "source":"git+https://example.com/repo"}),
    ] {
        let mut changed = metadata.clone();
        changed["packages"][0]["dependencies"] = json!([dependency]);
        assert!(check(&changed)
            .iter()
            .any(|failure| failure.contains("outside dependency")));
    }
    let mut development = metadata.clone();
    development["packages"][0]["dependencies"] = json!([{"name":"fixture", "kind":"dev"}]);
    assert!(check(&development).is_empty());
    let mut missing = metadata.clone();
    missing["packages"].as_array_mut().unwrap().remove(0);
    assert!(check(&missing)
        .iter()
        .any(|failure| failure.contains("missing selected package core")));
    let mut duplicate = metadata.clone();
    duplicate["packages"]
        .as_array_mut()
        .unwrap()
        .push(metadata["packages"][0].clone());
    assert!(check(&duplicate)
        .iter()
        .any(|failure| failure.contains("duplicate selected package core")));
    assert!(check_external_dependencies(b"{}", &policy).is_err());
    assert!(
        check_external_dependencies(&serde_json::to_vec(&metadata).unwrap(), &BTreeMap::new())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn consumer_can_include_development_dependencies_in_the_allowlist() {
    use rtepl_repository::dependencies::{check_external_dependencies_in_scope, DependencyScope};
    let (_tree, mut metadata, _) = fixture();
    metadata["packages"][0]["dependencies"] =
        json!([{"name":"runtime", "kind":"dev", "optional":true, "target":"cfg(unix)"}]);
    let mut policy = BTreeMap::from([("core".into(), BTreeSet::new())]);
    let bytes = serde_json::to_vec(&metadata).unwrap();
    assert!(
        check_external_dependencies_in_scope(&bytes, &policy, DependencyScope::Production)
            .unwrap()
            .is_empty()
    );
    assert!(check_external_dependencies_in_scope(
        &bytes,
        &policy,
        DependencyScope::AllDeclarations
    )
    .unwrap()
    .iter()
    .any(|failure| failure.contains("runtime")));
    policy.get_mut("core").unwrap().insert("runtime".into());
    assert!(check_external_dependencies_in_scope(
        &bytes,
        &policy,
        DependencyScope::AllDeclarations
    )
    .unwrap()
    .is_empty());
}
