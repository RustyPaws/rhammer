//! Loading the active game's FGDs and material index.

use super::App;
use crate::assets::Materials;
use crate::config::GameConfig;
use crate::formats::fgd::Fgd;
use std::path::Path;

impl App {
    pub fn reload_game_data(&mut self) {
        let (fgd, mats) = load_game_data(&self.vfs, &self.vpks, self.settings.active_game(), self.settings.steam_dir().as_deref());
        self.fgd = fgd;
        self.mats = mats;
        self.thumb_tex.clear();
        self.sprite_tex.clear();
        self.sprites_key = u64::MAX;
        self.inst_cache.clear();
        self.models.clear();
        self.mv = Default::default();
        self.doc.model_bounds.clear();
        self.inst_key = u64::MAX;
        self.world_key = (u64::MAX, 0);
        if let Some(g) = self.settings.active_game() {
            self.tex_scale = g.default_texture_scale;
        }
        // in the browser the files arrive asynchronously: repeat until a pass fetches nothing new
        #[cfg(feature = "web")]
        {
            self.loading = self.vfs.pending() > 0;
        }
    }
}

pub(crate) fn load_game_data(vfs: &crate::platform::SharedVfs, vpks: &crate::assets::gamefs::VpkCache, g: Option<&GameConfig>, steam_dir: Option<&Path>) -> (Fgd, Materials) {
    let Some(g) = g else {
        return (Fgd::default(), Materials::new(vfs.clone(), vpks, Path::new(""), steam_dir));
    };
    let mut fgd = Fgd::default();
    if let Some(first) = g.fgds.first() {
        fgd = Fgd::load(&**vfs, Path::new(first));
        // additional FGDs are merged by loading them as includes
        if g.fgds.len() > 1 {
            // Simple approach: build a temporary include-all file in memory is not possible, so merge classes.
            for extra in &g.fgds[1..] {
                let e = Fgd::load(&**vfs, Path::new(extra));
                for (k, v) in e.classes {
                    fgd.classes.insert(k, v);
                }
                for n in e.names {
                    if !fgd.names.contains(&n) {
                        fgd.names.push(n);
                    }
                }
            }
            fgd.names.sort_by_key(|s| s.to_ascii_lowercase());
        }
    }
    let mats = Materials::new(vfs.clone(), vpks, &g.game_path(), steam_dir);
    (fgd, mats)
}
