//! Persisted application settings, stored in a KeyValues file (`rhammer_config.txt`)
//! in the working directory.

use crate::config::{get_string as s, import_hammer, CompileSettings, GameConfig};
use crate::kv::{self as kv, Node, NodeList, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct Settings {
    pub games: Vec<GameConfig>,
    pub active: usize,
    pub compile: CompileSettings,
    pub last_map: String,
}

impl Settings {
    pub fn path() -> PathBuf {
        PathBuf::from("rhammer_config.txt")
    }

    pub fn active_game(&self) -> Option<&GameConfig> {
        self.games.get(self.active)
    }

    pub fn load() -> Settings {
        let mut st = Settings::default();
        if let Ok(bytes) = std::fs::read(Self::path()) {
            if let Ok(root) = kv::parse(&String::from_utf8_lossy(&bytes)) {
                if let Some(rh) = root.iter().find(|n| n.key == "rhammer").map(|n| n.children()) {
                    st.active = s(rh, "active").and_then(|v| v.parse().ok()).unwrap_or(0);
                    st.last_map = s(rh, "last_map").unwrap_or_default();
                    if let Some(games) = rh.get_block("games") {
                        for g in games {
                            if let Value::Block(c) = &g.value {
                                let hammer = c.get_block("Hammer").unwrap_or(&[]);
                                st.games.push(GameConfig::from_nodes(g.key.trim_matches('"'), s(c, "GameDir"), hammer));
                            }
                        }
                    }
                    if let Some(c) = rh.get_block("compile") {
                        st.compile = CompileSettings::from_nodes(c);
                    }
                }
            }
        }
        if st.games.is_empty() {
            st.auto_detect();
        }
        if st.active >= st.games.len() {
            st.active = 0;
        }
        st
    }

    /// First run: try the local `Portal 2` link, then Hammer's own config files.
    pub fn auto_detect(&mut self) {
        let candidates = [
            "Portal 2/bin/GameConfig.txt",
            "C:/Program Files (x86)/Steam/steamapps/common/Portal 2/bin/GameConfig.txt",
        ];
        for c in candidates {
            if let Ok(g) = import_hammer(Path::new(c)) {
                if !g.is_empty() {
                    self.games = g;
                    return;
                }
            }
        }
    }

    pub fn save(&self) {
        let games: Vec<Node> = self.games.iter().map(|g| g.to_node()).collect();
        let root = vec![Node::block(
            "rhammer",
            vec![
                Node::str("active", self.active.to_string()),
                Node::str("last_map", self.last_map.clone()),
                Node::block("games", games),
                self.compile.to_node(),
            ],
        )];
        let _ = std::fs::write(Self::path(), kv::to_string(&root));
    }
}
