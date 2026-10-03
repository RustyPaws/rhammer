//! Import of Hammer's own `bin/GameConfig.txt`.

use crate::config::{get_string, GameConfig};
use crate::kv::{self as kv, NodeList, Value};
use std::path::Path;

/// Parse Hammer's `GameConfig.txt`.
pub fn import_hammer(path: &Path) -> anyhow::Result<Vec<GameConfig>> {
    let text = String::from_utf8_lossy(&std::fs::read(path)?).into_owned();
    let root = kv::parse(&text)?;
    let mut out = Vec::new();
    if let Some(games) = root.iter().find(|n| n.key.eq_ignore_ascii_case("Configs")).map(|n| n.children()) {
        if let Some(games) = games.get_block("Games") {
            for g in games {
                if let Value::Block(c) = &g.value {
                    let hammer = c.get_block("Hammer").unwrap_or(&[]);
                    out.push(GameConfig::from_nodes(&g.key, get_string(c, "GameDir"), hammer));
                }
            }
        }
    }
    Ok(out)
}
