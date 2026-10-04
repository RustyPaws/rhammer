//! Steam install lookup for the `|appid_<id>|` search path macro: maps a Steam app id to the
//! folder it is installed in, using the `appmanifest_<id>.acf` files of every Steam library.

use crate::kv::{self as kv, NodeList};
use crate::platform::Vfs;
use std::path::{Path, PathBuf};

/// The `steamapps` folders of the Steam libraries a game can see.
pub struct SteamLibraries {
    steamapps: Vec<PathBuf>,
}

impl SteamLibraries {
    /// Libraries around `game_dir` (a mod in `steamapps/sourcemods` or a game in
    /// `steamapps/common`) plus the extra libraries listed in their `libraryfolders.vdf`.
    /// `steam_dir` (the Steam folder that contains `steamapps`, from the editor options) forces
    /// that Steam install; without it the default Steam install locations are searched.
    pub fn discover(vfs: &dyn Vfs, game_dir: &Path, steam_dir: Option<&Path>) -> SteamLibraries {
        let mut libs = SteamLibraries { steamapps: vec![] };
        for dir in game_dir.ancestors() {
            if dir.file_name().is_some_and(|n| n.eq_ignore_ascii_case("steamapps")) {
                libs.add(dir.to_path_buf());
            }
        }
        match steam_dir {
            Some(steam) => libs.add(steam.join("steamapps")),
            None => {
                for steam in default_steam_roots() {
                    libs.add(steam.join("steamapps"));
                }
            }
        }
        let mut i = 0;
        while i < libs.steamapps.len() {
            let known = libs.steamapps[i].clone();
            for extra in listed_libraries(vfs, &known) {
                libs.add(extra.join("steamapps"));
            }
            i += 1;
        }
        libs.steamapps.retain(|d| vfs.is_dir(d));
        libs
    }

    fn add(&mut self, steamapps: PathBuf) {
        if !self.steamapps.contains(&steamapps) {
            self.steamapps.push(steamapps);
        }
    }

    /// Install folder of Steam app `appid`, if one of the libraries has it installed.
    pub fn install_dir(&self, vfs: &dyn Vfs, appid: u32) -> Option<PathBuf> {
        self.steamapps.iter().find_map(|sa| {
            let bytes = vfs.read(&sa.join(format!("appmanifest_{appid}.acf")))?;
            let root = kv::parse(&String::from_utf8_lossy(&bytes)).ok()?;
            let state = root.iter().find(|n| n.key.eq_ignore_ascii_case("AppState"))?.children();
            let dir = sa.join("common").join(state.get_str("installdir")?);
            vfs.is_dir(&dir).then_some(dir)
        })
    }
}

/// Library paths named in `<steamapps>/libraryfolders.vdf` (both the old `"1" "<path>"` and the
/// new `"1" { "path" "<path>" }` layout).
fn listed_libraries(vfs: &dyn Vfs, steamapps: &Path) -> Vec<PathBuf> {
    let Some(bytes) = vfs.read(&steamapps.join("libraryfolders.vdf")) else { return vec![] };
    let Ok(root) = kv::parse(&String::from_utf8_lossy(&bytes)) else { return vec![] };
    let Some(list) = root.iter().find(|n| n.key.eq_ignore_ascii_case("libraryfolders")) else { return vec![] };
    list.children()
        .iter()
        .filter(|n| n.key.bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|n| n.as_str().or_else(|| n.children().get_str("path")))
        .map(PathBuf::from)
        .collect()
}

fn default_steam_roots() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        for var in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(p) = std::env::var_os(var) {
                v.push(PathBuf::from(p).join("Steam"));
            }
        }
    }
    #[cfg(not(windows))]
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        v.push(home.join(".steam/steam"));
        v.push(home.join(".local/share/Steam"));
        v.push(home.join("Library/Application Support/Steam"));
    }
    v
}
