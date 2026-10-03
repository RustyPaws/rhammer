//! Desktop backend: plain `std::fs`.

use super::{DirEntry, Vfs};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub struct LocalFs;

impl Vfs for LocalFs {
    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    fn read_range(&self, path: &Path, off: u64, len: usize) -> Option<Vec<u8>> {
        let mut f = std::fs::File::open(path).ok()?;
        f.seek(SeekFrom::Start(off)).ok()?;
        let mut buf = vec![0u8; len];
        f.read_exact(&mut buf).ok()?;
        Some(buf)
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn read_dir(&self, path: &Path) -> Vec<DirEntry> {
        let Ok(rd) = std::fs::read_dir(path) else { return vec![] };
        rd.flatten()
            .map(|e| DirEntry { name: e.file_name().to_string_lossy().into_owned(), is_dir: e.path().is_dir() })
            .collect()
    }

    fn modified(&self, path: &Path) -> Option<u64> {
        let t = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
        t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_nanos() as u64)
    }

    fn canonical(&self, path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }
}
