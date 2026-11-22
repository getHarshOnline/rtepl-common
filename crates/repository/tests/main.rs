// SPDX-License-Identifier: AGPL-3.0-or-later

//! Inventory behavior needed by repository source checks.

mod dependencies;
mod purity;
mod rust_source;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use rtepl_repository::source_entries;

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rtepl-source-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("temporary test root");
        Self(path)
    }
    fn file(&self, name: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().expect("parent")).expect("directories");
        fs::write(path, "source").expect("source");
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn exclusions_apply_at_each_depth_and_results_are_sorted() {
    let tree = Tree::new();
    for name in [
        "z.rs",
        "a/src/lib.rs",
        "a/target/generated.rs",
        "target/hidden.rs",
        "a/tests/example.rs",
    ] {
        tree.file(name);
    }
    let entries = source_entries(&tree.0, &["target"]).expect("inventory");
    let paths: Vec<_> = entries
        .iter()
        .map(|e| {
            e.path
                .strip_prefix(&tree.0)
                .expect("relative")
                .to_str()
                .expect("UTF-8")
        })
        .collect();
    assert_eq!(paths, ["a/src/lib.rs", "a/tests/example.rs", "z.rs"]);
    assert!(entries.iter().all(|e| e.file_type.is_file()));
    let all = source_entries(&tree.0, &[]).expect("unfiltered inventory");
    assert_eq!(all.len(), 5);
}

#[test]
fn missing_root_and_non_directory_root_are_errors() {
    let tree = Tree::new();
    tree.file("plain.rs");
    assert!(source_entries(&tree.0.join("missing"), &[]).is_err());
    assert!(source_entries(&tree.0.join("plain.rs"), &[]).is_err());
}

#[cfg(unix)]
#[test]
fn links_are_reported_without_following_targets_or_cycles() {
    use std::os::unix::fs::symlink;
    let tree = Tree::new();
    let outside = Tree::new();
    outside.file("private.rs");
    tree.file("regular.rs");
    symlink(&outside.0, tree.0.join("linked-dir")).expect("directory link");
    symlink(outside.0.join("private.rs"), tree.0.join("linked.rs")).expect("file link");
    symlink(&tree.0, tree.0.join("cycle")).expect("cycle link");
    symlink(outside.0.join("absent"), tree.0.join("broken.rs")).expect("broken link");
    let entries = source_entries(&tree.0, &[]).expect("inventory");
    assert_eq!(entries.len(), 5);
    assert_eq!(
        entries.iter().filter(|e| e.file_type.is_symlink()).count(),
        4
    );
    assert!(entries.iter().all(|e| !e.path.ends_with("private.rs")));
}
