//! Works out game configurations from a picked game folder.
//!
//! Every path is rooted at the folder's name (`Portal 2/portal2`), see [`super`].

use super::WebFs;
use crate::config::{import_hammer, GameConfig};
use std::path::Path;

fn join(dir: &str, rest: &str) -> String {
    if dir.is_empty() {
        rest.to_string()
    } else if rest.is_empty() {
        dir.to_string()
    } else {
        format!("{dir}/{rest}")
    }
}

/// Rewrites an absolute path from Hammer's config (`C:\...\Portal 2\portal2`) into one below the
/// picked folder (`Portal 2/portal2`): everything up to the last component named like the folder
/// (`orig`) is replaced by `root`. Paths that do not mention the folder are returned unchanged.
pub fn virtualize(root: &str, orig: &str, p: &str) -> String {
    if p.is_empty() {
        return String::new();
    }
    let norm = p.replace('\\', "/");
    let comps: Vec<&str> = norm.split('/').collect();
    match comps.iter().rposition(|c| c.eq_ignore_ascii_case(orig)) {
        Some(i) => join(root, &comps[i + 1..].join("/")),
        None => norm,
    }
}

fn virtualize_config(g: &mut GameConfig, root: &str, orig: &str) {
    let v = |s: &mut String| *s = virtualize(root, orig, s);
    v(&mut g.game_dir);
    for f in &mut g.fgds {
        *f = virtualize(root, orig, f);
    }
    v(&mut g.game_exe);
    v(&mut g.game_exe_dir);
    v(&mut g.bsp_exe);
    v(&mut g.vis_exe);
    v(&mut g.light_exe);
    v(&mut g.map_dir);
    v(&mut g.bsp_dir);
    v(&mut g.prefab_dir);
}

pub async fn detect(web: &WebFs, root: &str, orig: &str) -> Result<Vec<GameConfig>, String> {
    // 1. Hammer's own configuration carries the FGDs, default entities and directories.
    let hammer = join(root, "bin/GameConfig.txt");
    if web.ensure_file(&hammer).await.is_some() {
        if let Ok(list) = import_hammer(web, Path::new(&hammer)) {
            let mut out = vec![];
            for mut g in list {
                virtualize_config(&mut g, root, orig);
                if web.ensure_stat(&join(&g.game_dir, "gameinfo.txt")).await == 1 {
                    out.push(g);
                }
            }
            if !out.is_empty() {
                return Ok(out);
            }
        }
    }

    // 2. No usable Hammer config: look for a gameinfo.txt in the folder or one level below.
    let mut candidates = vec![root.to_string()];
    candidates.extend(web.ensure_dir(root).await.into_iter().filter(|(_, d)| *d).map(|(n, _)| join(root, &n)));
    for game_dir in candidates {
        if web.ensure_stat(&join(&game_dir, "gameinfo.txt")).await != 1 {
            continue;
        }
        let bin = join(root, "bin");
        let fgds: Vec<String> = web
            .ensure_dir(&bin)
            .await
            .into_iter()
            .filter(|(n, d)| !*d && n.to_ascii_lowercase().ends_with(".fgd"))
            .map(|(n, _)| join(&bin, &n))
            .collect();
        // prefer the game's own FGD (it includes the base ones)
        let leaf = game_dir.rsplit('/').next().unwrap_or(root);
        let own = join(&bin, &format!("{leaf}.fgd"));
        let fgds = match fgds.iter().find(|f| f.eq_ignore_ascii_case(&own)) {
            Some(f) => vec![f.clone()],
            None => fgds,
        };
        let g = GameConfig { name: orig.to_string(), map_dir: join(&game_dir, "maps"), game_dir, fgds, ..Default::default() };
        return Ok(vec![g]);
    }
    Err(format!("No gameinfo.txt found in '{orig}'. Pick the game folder (for example 'Portal 2')."))
}
