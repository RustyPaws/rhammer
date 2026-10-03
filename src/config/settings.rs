//! Persisted application settings, stored in a KeyValues file (`rhammer_config.txt`)
//! in the working directory.

#[cfg(feature = "local")]
use crate::config::import_hammer;
use crate::config::{get_string as s, CompileSettings, GameConfig};
use crate::kv::{self as kv, Node, NodeList, Value};
#[cfg(feature = "local")]
use std::path::Path;
use std::path::PathBuf;

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
        let mut st = Settings::default();
        {
            if let Ok(root) = kv::parse(text) {
                if let Some(rh) = root.iter().find(|n| n.key == "rhammer").map(|n| n.children()) {
                    st.active = s(rh, "active").and_then(|v| v.parse().ok()).unwrap_or(0);
                    st.last_map = s(rh, "last_map").unwrap_or_default();
                    if let Some(games) = rh.get_block("games") {
                        for g in games {
                            if let Value::Block(c) = &g.value {
                                st.games.push(crate::config::game_from_block(g.key.trim_matches('"'), c));
                            }
                        }
                    }
                    if let Some(c) = rh.get_block("compile") {
                        st.compile = CompileSettings::from_nodes(c);
                    }
                }
            }
        }
        #[cfg(feature = "local")]
        if st.games.is_empty() {
            st.auto_detect();
        }
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
        kv::to_string(&root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_text_round_trips_with_hammer_extras() {
        let hammer = "\"Configs\" { \"Games\" { \"Portal 2\" { \"GameDir\" \"Portal 2/portal2\" \"Hammer\" { \"GameData0\" \"Portal 2/bin/portal2.fgd\" \"MapDir\" \"Portal 2/sdk_content/maps\" \"MaterialExcludeCount\" \"1\" \"MaterialExclusions\" { \"-1\" \"models\" } } } } }";
        let root = kv::parse(hammer).unwrap();
        let games = root[0].children().get_block("Games").unwrap();
        let Value::Block(c) = &games[0].value else { panic!() };
        let g = crate::config::game_from_block("Portal 2", c);
        assert_eq!(g.fgds, ["Portal 2/bin/portal2.fgd"]);
        assert_eq!(g.hammer_extra.iter().map(|n| n.key.as_str()).collect::<Vec<_>>(), ["MaterialExclusions"]);

        let st = Settings { games: vec![g.clone()], active: 0, last_map: "a/b.vmf".into(), ..Default::default() };
        let back = Settings::from_text(&st.to_text());
        assert_eq!(back.games, vec![g]);
        assert_eq!(back.last_map, "a/b.vmf");
    }
}
