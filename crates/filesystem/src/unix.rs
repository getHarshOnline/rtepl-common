// SPDX-License-Identifier: AGPL-3.0-or-later

//! Unix file observations and read-only descriptor opening: what the process adapters of
//! Pale and Gazette use to open an executable and to tell whether it is still the file
//! they first saw. It builds on the standard library and `rustix`, and nothing else in
//! this repository builds on it. [`FileIdentity`] records the metadata to compare, and
//! [`open_readonly_nofollow`] returns a read-only descriptor that refuses a final
//! symbolic link. Callers check file type, ownership and whatever else their operation
//! needs.

use std::{
    fs::{File, Metadata},
    io,
    os::unix::fs::MetadataExt,
    path::Path,
};

/// Metadata used to compare a retained file with a later observation.
///
/// This is a local comparison, not a portable identifier or a content digest.
/// Callers check file type, ownership and any other facts their operation needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIdentity {
    /// Device containing the file.
    pub device: u64,
    /// Inode on that device.
    pub inode: u64,
    /// File type and permission bits.
    pub mode: u32,
    /// Observed length in bytes.
    pub length: u64,
    /// Modification time, in seconds.
    pub modified_seconds: i64,
    /// Nanosecond component of the modification time.
    pub modified_nanoseconds: i64,
    /// Metadata change time, in seconds.
    pub changed_seconds: i64,
    /// Nanosecond component of the metadata change time.
    pub changed_nanoseconds: i64,
}

impl From<&Metadata> for FileIdentity {
    fn from(metadata: &Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
            length: metadata.len(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
            changed_seconds: metadata.ctime(),
            changed_nanoseconds: metadata.ctime_nsec(),
        }
    }
}

/// Open for reading, refusing a symbolic link at the final path component.
///
/// The descriptor closes on exec. With `nonblocking`, opening a FIFO does not
/// wait for a writer. Ancestor components can still be symbolic links; this
/// function neither confines the path nor requires a regular executable file.
///
/// # Errors
/// Returns the operating system's error when opening fails.
pub fn open_readonly_nofollow(path: &Path, nonblocking: bool) -> io::Result<File> {
    use rustix::fs::{open, Mode, OFlags};

    let mut flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    if nonblocking {
        flags |= OFlags::NONBLOCK;
    }
    open(path, flags, Mode::empty())
        .map(File::from)
        .map_err(io::Error::from)
}
