//! Configuration layer: game configurations (modelled after Hammer's GameConfig.txt),
//! per-config compile settings, and the persisted application settings.

mod compile;
mod game;
mod hammer;
mod settings;

pub use compile::CompileSettings;
pub use game::GameConfig;
pub use hammer::import_hammer;
pub use settings::Settings;

use crate::kv::{Node, NodeList};

/// Read a string value from a KeyValues node list.
pub(crate) fn get_string(nodes: &[Node], key: &str) -> Option<String> {
    nodes.get_str(key).map(|v| v.to_string())
}

pub(crate) fn flag_str(v: bool) -> &'static str {
    if v { "1" } else { "0" }
}

/// Builds a [`GameConfig`] from one game block (Hammer's `GameConfig.txt` and our settings share the layout).
pub(crate) fn game_from_block(name: &str, c: &[Node]) -> GameConfig {
    let hammer = c.get_block("Hammer").unwrap_or(&[]);
    let extra: Vec<Node> = c
        .iter()
        .filter(|n| !n.key.eq_ignore_ascii_case("GameDir") && !n.key.eq_ignore_ascii_case("Hammer"))
        .cloned()
        .collect();
    GameConfig::from_block(name, get_string(c, "GameDir"), hammer, &extra)
}
