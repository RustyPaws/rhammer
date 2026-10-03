//! Platform file access. Everything that reads game files goes through [`Vfs`] so the same
//! code runs on the desktop (`std::fs`) and in the browser (File System Access API).
//!
//! Paths are ordinary `Path`s used lexically; what they are relative to is up to the
//! implementation (the real filesystem locally, the picked folder on the web).

use std::path::{Path, PathBuf};
use std::rc::Rc;

#[cfg(feature = "web")]
mod web;
#[cfg(feature = "web")]
pub use web::{storage_get, storage_set, supported as web_supported, WebEvent, WebFs};
#[cfg(feature = "local")]
mod local;
#[cfg(feature = "local")]
pub use local::LocalFs;

pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
}

pub trait Vfs {
    /// Whole file. `None` if it is missing (or, for asynchronous backends, not loaded yet).
    fn read(&self, path: &Path) -> Option<Vec<u8>>;
    /// `len` bytes starting at `off`; used for random access into large archives.
    fn read_range(&self, path: &Path, off: u64, len: usize) -> Option<Vec<u8>>;
    fn is_file(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    /// Entries of a directory; empty if it does not exist.
    fn read_dir(&self, path: &Path) -> Vec<DirEntry>;
    /// Modification stamp, used to notice files changed on disk. `None` if unsupported.
    fn modified(&self, _path: &Path) -> Option<u64> {
        None
    }
    /// A stable spelling of `path` (symlinks and `..` resolved where the backend can).
    fn canonical(&self, path: &Path) -> PathBuf {
        path.to_path_buf()
    }
    fn exists(&self, path: &Path) -> bool {
        self.is_file(path) || self.is_dir(path)
    }
    /// Number of asynchronous requests still in flight (browser build).
    fn pending(&self) -> usize {
        0
    }
    /// Counter that grows whenever a read had to answer "not available yet" because its data is
    /// still being fetched. Compare before/after an operation to learn whether it was cut short.
    fn stalls(&self) -> usize {
        0
    }
}

pub type SharedVfs = Rc<dyn Vfs>;

/// The file access of the current build.
#[cfg(feature = "local")]
pub fn default_vfs() -> SharedVfs {
    Rc::new(LocalFs)
}
