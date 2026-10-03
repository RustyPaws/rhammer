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
