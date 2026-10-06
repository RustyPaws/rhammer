//! Persisted application settings, stored as TOML (`rhammer.toml`) in the working directory.

#[cfg(feature = "local")]
use crate::config::import_hammer;
use crate::config::{CompileSettings, GameConfig};
use crate::ui::layout::{self, UiLayout};
use serde::{Deserialize, Serialize};
#[cfg(feature = "local")]
use std::path::Path;
use std::path::PathBuf;

/// General editor options (Options > Editor options).
/// Look of the 2D views (Options > Editor options).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct View2dStyle {
    pub sel_color: [u8; 3],
    pub sel_width: f32,
    pub fill_point_entities: bool,
    pub background: [u8; 3],
    pub grid_brightness: f32,
}

impl Default for View2dStyle {
    fn default() -> Self {
        View2dStyle { sel_color: [255, 0, 0], sel_width: 2.0, fill_point_entities: false, background: [0, 0, 0], grid_brightness: 1.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorOptions {
    /// Steam folder (the one containing `steamapps`). When set it is used instead of the default search locations.
    pub steam_dir: Option<String>,
    /// Reopen the last map on every start, not only after a crash.
    pub restore_last_map: bool,
    /// Z toggles mouse-look in the 3D view (otherwise Z maximizes it).
    pub z_freelook: bool,
    /// Camera fly speed multiplier (0.25..=10).
    pub cam_speed: f32,
    pub view2d: View2dStyle,
}

impl Default for EditorOptions {
    fn default() -> Self {
        EditorOptions { steam_dir: None, restore_last_map: false, z_freelook: true, cam_speed: 1.0, view2d: View2dStyle::default() }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub editor: EditorOptions,
    pub games: Vec<GameConfig>,
    pub active: usize,
    pub compile: CompileSettings,
    pub last_map: String,
    #[serde(deserialize_with = "layout::lenient")]
    pub ui: UiLayout,
}

impl Settings {
    pub fn path() -> PathBuf {
        PathBuf::from("rhammer.toml")
    }

    /// The forced Steam folder, if one is set in the editor options.
    pub fn steam_dir(&self) -> Option<PathBuf> {
        self.editor.steam_dir.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(PathBuf::from)
    }

    pub fn active_game(&self) -> Option<&GameConfig> {
        self.games.get(self.active)
    }

    /// Where the settings text is kept in the browser.
    #[cfg(feature = "web")]
    const STORAGE_KEY: &'static str = "rhammer_config";

    pub fn load() -> Settings {
        #[cfg(feature = "local")]
        let text = std::fs::read(Self::path()).ok().map(|b| String::from_utf8_lossy(&b).into_owned());
        #[cfg(feature = "web")]
        let text = crate::platform::storage_get(Self::STORAGE_KEY);
        Self::from_text(text.as_deref().unwrap_or(""))
    }

    pub fn from_text(text: &str) -> Settings {
        let mut st: Settings = match toml::from_str(text) {
            Ok(st) => st,
            Err(e) => {
                eprintln!("rhammer: cannot parse settings: {e}");
                Settings::default()
            }
        };
        #[cfg(feature = "local")]
        if st.games.is_empty() {
            st.auto_detect();
        }
        st.compile.normalize();
        if st.active >= st.games.len() {
            st.active = 0;
        }
        st
    }

    /// First run: try the local `Portal 2` link, then Hammer's own config files.
    #[cfg(feature = "local")]
    pub fn auto_detect(&mut self) {
        let candidates = [
            "Portal 2/bin/GameConfig.txt",
            "C:/Program Files (x86)/Steam/steamapps/common/Portal 2/bin/GameConfig.txt",
        ];
        for c in candidates {
            if let Ok(g) = import_hammer(&crate::platform::LocalFs, Path::new(c)) {
                if !g.is_empty() {
                    self.games = g;
                    return;
                }
            }
        }
    }

    pub fn save(&self) {
        let text = self.to_text();
        #[cfg(feature = "local")]
        let _ = std::fs::write(Self::path(), text);
        #[cfg(feature = "web")]
        crate::platform::storage_set(Self::STORAGE_KEY, &text);
    }

    pub fn to_text(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }
}
