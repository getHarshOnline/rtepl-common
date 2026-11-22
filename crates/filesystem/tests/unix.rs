//! Descriptor flags and metadata changes observed through real files.
#![cfg(unix)]

use std::{
    fs::{self, File},
    io::Read,
    os::unix::fs::{symlink, PermissionsExt},
    process::Command,
    sync::mpsc,
    thread,
    time::Duration,
};

use rtepl_filesystem::unix::{open_readonly_nofollow, FileIdentity};
use rustix::fs::{fcntl_getfl, OFlags};
use rustix::io::{fcntl_getfd, FdFlags};

#[test]
fn opens_readonly_with_close_on_exec_and_the_requested_blocking_mode() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("file");
    fs::write(&path, b"contents").unwrap();
    for nonblocking in [false, true] {
        let mut file = open_readonly_nofollow(&path, nonblocking).unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "contents");
        assert!(fcntl_getfd(&file).unwrap().contains(FdFlags::CLOEXEC));
        let flags = fcntl_getfl(&file).unwrap();
        assert_eq!(flags & OFlags::ACCMODE, OFlags::RDONLY);
        assert_eq!(flags.contains(OFlags::NONBLOCK), nonblocking);
    }
    let link = directory.path().join("link");
    symlink(&path, &link).unwrap();
    assert!(open_readonly_nofollow(&link, false).is_err());
    assert!(open_readonly_nofollow(&link, true).is_err());
}

#[test]
fn nonblocking_fifo_open_does_not_require_a_writer() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fifo");
    // rustix's mkfifoat is unavailable on Apple platforms.
    assert!(Command::new("mkfifo")
        .arg(&path)
        .status()
        .unwrap()
        .success());
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(open_readonly_nofollow(&path, true));
    });
    let file = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("opening a FIFO waited for a writer")
        .unwrap();
    assert!(fcntl_getfl(&file).unwrap().contains(OFlags::NONBLOCK));
}

#[test]
fn identity_detects_resize_permissions_and_path_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("file");
    fs::write(&path, b"one").unwrap();
    let held = File::open(&path).unwrap();
    let original = FileIdentity::from(&held.metadata().unwrap());
    assert_eq!(original, FileIdentity::from(&fs::metadata(&path).unwrap()));
    fs::write(&path, b"longer").unwrap();
    let resized = FileIdentity::from(&held.metadata().unwrap());
    assert_ne!(original, resized);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let executable = FileIdentity::from(&held.metadata().unwrap());
    assert_ne!(resized, executable);
    fs::rename(&path, directory.path().join("old")).unwrap();
    fs::write(&path, b"longer").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let replacement = FileIdentity::from(&fs::metadata(&path).unwrap());
    assert_ne!(executable.inode, replacement.inode);
    assert_ne!(FileIdentity::from(&held.metadata().unwrap()), replacement);
}
