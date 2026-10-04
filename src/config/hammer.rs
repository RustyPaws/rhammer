//! Import of Hammer's own `bin/GameConfig.txt`.

use crate::config::GameConfig;
use crate::kv::{self as kv, NodeList, Value};
use crate::platform::Vfs;
use std::path::Path;

/// Parse Hammer's `GameConfig.txt`.
pub fn import_hammer(vfs: &dyn Vfs, path: &Path) -> anyhow::Result<Vec<GameConfig>> {
    let bytes = vfs.read(path).ok_or_else(|| anyhow::anyhow!("cannot read {}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let root = kv::parse(&text)?;
    let mut out = Vec::new();
    if let Some(games) = root.iter().find(|n| n.key.eq_ignore_ascii_case("Configs")).map(|n| n.children()) {
        if let Some(games) = games.get_block("Games") {
            for g in games {
                if let Value::Block(c) = &g.value {
                    out.push(GameConfig::from_hammer_block(&g.key, c));
                }
            }
        }
    }
    Ok(out)
}
