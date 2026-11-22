// SPDX-License-Identifier: AGPL-3.0-or-later

//! Check a Cargo workspace against its owner's declared local dependency graph, for the
//! repository checks of Gazette and Weave. It builds on `serde` to read Cargo's
//! metadata output, and nothing else in this repository builds on it. The caller
//! supplies the policy and the locked metadata; this module reports every edge that
//! departs from it.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Rules for local Cargo packages. Registry and Git dependencies are not local edges.
#[derive(Debug, Default)]
pub struct DependencyPolicy {
    /// Exact package names and their normal or build-time local dependencies.
    /// Target-specific and optional declarations count even when not enabled.
    pub production: BTreeMap<String, BTreeSet<String>>,
    /// Fixture-only package names and their exact directories relative to the root.
    /// These packages may have no local dependencies and may only be dev dependencies.
    pub test_support: BTreeMap<String, PathBuf>,
    /// Local edges forbidden in every scope, including dev dependencies.
    pub forbidden: BTreeSet<(String, String)>,
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    manifest_path: PathBuf,
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    path: Option<PathBuf>,
    kind: Option<String>,
}

/// Compare `cargo metadata --no-deps --format-version 1` output with a policy.
///
/// All manifest and dependency directories must exist. Canonical paths expose
/// outside checkouts even when a path uses `..` or a symbolic link. This is a
/// repository check, not a reader protected against concurrent filesystem edits.
/// The caller runs Cargo with its required locked/offline settings.
///
/// # Errors
/// Returns an error for malformed metadata or unreadable paths. Policy failures
/// are returned as messages, including extra or missing packages and edges.
pub fn check_dependencies(
    root: &Path,
    metadata_json: &[u8],
    policy: &DependencyPolicy,
) -> io::Result<Vec<String>> {
    let metadata: Metadata = serde_json::from_slice(metadata_json)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let root = root.canonicalize()?;
    let support_directories = policy
        .test_support
        .values()
        .map(|directory| root.join(directory).canonicalize())
        .collect::<io::Result<Vec<_>>>()?;
    let mut failures = Vec::new();
    let mut observed = BTreeMap::new();
    let mut support_seen = BTreeSet::new();
    for package in metadata.packages {
        let manifest = package.manifest_path.canonicalize()?;
        if !manifest.starts_with(&root) {
            failures.push(format!(
                "{} has a manifest outside the repository",
                package.name
            ));
        }
        if let Some(directory) = policy.test_support.get(&package.name) {
            if manifest != root.join(directory).join("Cargo.toml").canonicalize()? {
                failures.push(format!(
                    "{} is outside its declared test-support directory",
                    package.name
                ));
            }
            if !support_seen.insert(package.name.clone()) {
                failures.push(format!("duplicate test-support package {}", package.name));
            }
            if package
                .dependencies
                .iter()
                .any(|dependency| dependency.path.is_some())
            {
                failures.push(format!(
                    "{} test support has local dependencies",
                    package.name
                ));
            }
            continue;
        }
        if support_directories
            .iter()
            .any(|directory| manifest.starts_with(directory))
        {
            failures.push(format!(
                "{} occupies a directory reserved for test support",
                package.name
            ));
        }
        let mut inward = BTreeSet::new();
        for dependency in package.dependencies {
            let Some(path) = dependency.path else {
                continue;
            };
            if !path.canonicalize()?.starts_with(&root) {
                failures.push(format!(
                    "{} depends on an external checkout: {}",
                    package.name,
                    path.display()
                ));
            }
            if policy
                .forbidden
                .contains(&(package.name.clone(), dependency.name.clone()))
            {
                failures.push(format!(
                    "{} must not depend on {}",
                    package.name, dependency.name
                ));
            }
            if policy.test_support.contains_key(&dependency.name)
                && dependency.kind.as_deref() != Some("dev")
            {
                failures.push(format!(
                    "{} uses test support {} outside dev-dependencies",
                    package.name, dependency.name
                ));
            }
            if dependency.kind.as_deref() != Some("dev") {
                inward.insert(dependency.name);
            }
        }
        if observed.insert(package.name.clone(), inward).is_some() {
            failures.push(format!("duplicate package {}", package.name));
        }
    }
    for name in policy.test_support.keys() {
        if !support_seen.contains(name) {
            failures.push(format!("missing test-support package {name}"));
        }
    }
    if observed != policy.production {
        failures.push(format!(
            "local dependency graph differs: expected {:#?}, observed {observed:#?}",
            policy.production
        ));
    }
    Ok(failures)
}

/// Allowed registry and Git dependencies for each selected package.
///
/// A missing package entry leaves it outside this check. An empty set forbids
/// every outside production dependency. Names are Cargo package names, even
/// when imported under an alias; optional, target-specific and build dependencies
/// all count. Development dependencies remain outside this production rule.
pub type ExternalDependencyPolicy = BTreeMap<String, BTreeSet<String>>;

/// Which dependency declarations an outside-crate allowlist covers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DependencyScope {
    /// Normal and build dependencies, including optional and target-specific ones.
    #[default]
    Production,
    /// Every declaration, including development dependencies.
    AllDeclarations,
}

/// Check outside dependencies in the same Cargo metadata used by the local graph.
///
/// Use alongside [`check_dependencies`], which still checks all local edges and
/// rejects paths outside the repository. This checks declarations, not transitive
/// dependencies or the contents of an allowed crate.
///
/// # Errors
/// Returns an error for malformed Cargo metadata. Missing selected packages and
/// disallowed dependencies are returned as policy failures.
pub fn check_external_dependencies(
    metadata_json: &[u8],
    policy: &ExternalDependencyPolicy,
) -> io::Result<Vec<String>> {
    check_external_dependencies_in_scope(metadata_json, policy, DependencyScope::Production)
}

/// Check outside dependencies with an explicit choice of development scope.
///
/// The package allowlists have the same meaning as [`check_external_dependencies`].
/// `AllDeclarations` also requires every dev dependency to be allowed.
///
/// # Errors
/// Returns an error for malformed Cargo metadata; policy failures are messages.
pub fn check_external_dependencies_in_scope(
    metadata_json: &[u8],
    policy: &ExternalDependencyPolicy,
    scope: DependencyScope,
) -> io::Result<Vec<String>> {
    let metadata: Metadata = serde_json::from_slice(metadata_json)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut seen = BTreeSet::new();
    let mut failures = Vec::new();
    for package in metadata.packages {
        let Some(allowed) = policy.get(&package.name) else {
            continue;
        };
        if !seen.insert(package.name.clone()) {
            failures.push(format!("duplicate selected package {}", package.name));
        }
        for dependency in package.dependencies {
            if dependency.path.is_none()
                && (scope == DependencyScope::AllDeclarations
                    || dependency.kind.as_deref() != Some("dev"))
                && !allowed.contains(&dependency.name)
            {
                failures.push(format!(
                    "{} must not use outside dependency {}",
                    package.name, dependency.name
                ));
            }
        }
    }
    for name in policy.keys() {
        if !seen.contains(name) {
            failures.push(format!("missing selected package {name}"));
        }
    }
    failures.sort();
    failures.dedup();
    Ok(failures)
}
