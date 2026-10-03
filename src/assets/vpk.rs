//! Valve VPK (v1/v2) directory reader.

use std::collections::HashMap;
use crate::platform::SharedVfs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Entry {
    pub archive: u16,
    pub offset: u32,
    pub length: u32,
    pub preload: Vec<u8>,
}

pub struct Vpk {
    vfs: SharedVfs,
    base: PathBuf, // path prefix without "_dir.vpk"
    dir_path: PathBuf,
    data_start: u64,
    pub files: HashMap<String, Entry>,
}

fn cstr(b: &[u8], i: &mut usize) -> Option<String> {
    let st = *i;
    while *i < b.len() && b[*i] != 0 {
        *i += 1;
    }
    if *i >= b.len() {
        return None;
    }
    let s = String::from_utf8_lossy(&b[st..*i]).into_owned();
    *i += 1;
    Some(s)
}

impl Vpk {
    pub fn open(vfs: &SharedVfs, dir_path: &Path) -> Option<Vpk> {
        let hdr = vfs.read_range(dir_path, 0, 12)?;
        let sig = u32::from_le_bytes(hdr[0..4].try_into().ok()?);
        if sig != 0x55AA1234 {
            return None;
        }
        let version = u32::from_le_bytes(hdr[4..8].try_into().ok()?);
        let tree_size = u32::from_le_bytes(hdr[8..12].try_into().ok()?) as usize;
        let header_size = if version >= 2 { 28 } else { 12 };
        let tree = vfs.read_range(dir_path, header_size as u64, tree_size)?;
        let mut files = HashMap::new();
        let mut i = 0;
        loop {
            let ext = cstr(&tree, &mut i)?;
            if ext.is_empty() {
                break;
            }
            loop {
                let path = cstr(&tree, &mut i)?;
                if path.is_empty() {
                    break;
                }
                loop {
                    let name = cstr(&tree, &mut i)?;
                    if name.is_empty() {
                        break;
                    }
                    if i + 18 > tree.len() {
                        return None;
                    }
                    let preload_len = u16::from_le_bytes(tree[i + 4..i + 6].try_into().ok()?) as usize;
                    let archive = u16::from_le_bytes(tree[i + 6..i + 8].try_into().ok()?);
                    let offset = u32::from_le_bytes(tree[i + 8..i + 12].try_into().ok()?);
                    let length = u32::from_le_bytes(tree[i + 12..i + 16].try_into().ok()?);
                    i += 18;
                    let preload = tree.get(i..i + preload_len)?.to_vec();
                    i += preload_len;
                    let full = if path == " " {
                        format!("{name}.{ext}")
                    } else {
                        format!("{path}/{name}.{ext}")
                    };
                    files.insert(full.to_ascii_lowercase(), Entry { archive, offset, length, preload });
                }
            }
        }
        let s = dir_path.to_string_lossy();
        let base = PathBuf::from(s.trim_end_matches("_dir.vpk").to_string());
        Some(Vpk {
            vfs: vfs.clone(),
            base,
            dir_path: dir_path.to_path_buf(),
            data_start: (header_size + tree_size) as u64,
            files,
        })
    }

    pub fn dir_path(&self) -> &Path {
        &self.dir_path
    }

    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        let e = self.files.get(&name.to_ascii_lowercase().replace('\\', "/"))?;
        let mut out = e.preload.clone();
        if e.length > 0 {
            let (path, off) = if e.archive == 0x7fff {
                (self.dir_path.clone(), self.data_start + e.offset as u64)
            } else {
                (PathBuf::from(format!("{}_{:03}.vpk", self.base.display(), e.archive)), e.offset as u64)
            };
            out.extend(self.vfs.read_range(&path, off, e.length as usize)?);
        }
        Some(out)
    }
}
