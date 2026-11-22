// SPDX-License-Identifier: AGPL-3.0-or-later

//! Source inspection shared by the repository checks of Pale, Gazette and Weave. It
//! builds on `serde`, `serde_json` and `syn`; each product's repository check builds on
//! it, Pale's for the source listing alone. It lists a repository's source files and
//! offers dependency, purity and syntax checks over them.
//!
//! Callers choose excluded directories and the rules applied to each source.
//! Product dependency graphs, naming rules and accepted outcomes stay with them.

pub mod dependencies;
pub mod purity;
pub mod rust_source;

use std::fs::{self, FileType};
use std::io;
use std::path::{Path, PathBuf};

/// A non-directory entry observed without following a symbolic link.
#[derive(Debug)]
pub struct SourceEntry {
    /// Path beneath the root supplied to [`source_entries`].
    pub path: PathBuf,
    /// Type of the directory entry itself, rather than a link's target.
    pub file_type: FileType,
}

/// Collect non-directory entries in deterministic path order.
///
/// Directory names in `excluded_directories` are skipped at every depth.
/// The root is resolved by the filesystem; symbolic links encountered below it
/// are reported as links and never traversed. The caller chooses
/// which entry kinds and file extensions its checks read. This is an inventory
/// helper, not protection against a tree changing between inspection and use.
///
/// # Errors
///
/// Returns the filesystem error if the root, a directory or an entry type cannot
/// be read. No partial inventory is returned on failure.
pub fn source_entries(root: &Path, excluded_directories: &[&str]) -> io::Result<Vec<SourceEntry>> {
    let mut entries = Vec::new();
    collect(root, excluded_directories, &mut entries)?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

fn collect(root: &Path, excluded: &[&str], output: &mut Vec<SourceEntry>) -> io::Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| excluded.contains(&name))
            {
                continue;
            }
            collect(&entry.path(), excluded, output)?;
        } else {
            output.push(SourceEntry {
                path: entry.path(),
                file_type,
            });
        }
    }
    Ok(())
}
