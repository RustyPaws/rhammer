//! The game's sounds: raw files under `sound/` and the named entries of the sound scripts
//! (`scripts/game_sounds*.txt`, listed by `game_sounds_manifest.txt`).

use crate::assets::gamefs::GameFs;
use crate::kv::{self, Node, Value};

#[derive(Default)]
pub struct SoundLists {
    /// Files relative to `sound/`, e.g. `ambient/wind.wav`.
    pub files: Vec<String>,
    /// Script entries: (name, first wave file relative to `sound/`).
    pub scripts: Vec<(String, String)>,
}

/// Source marks a wave's behaviour with leading characters (`*` stream, `#` dialog, `)` spatial ...).
pub fn strip_wave_prefix(wave: &str) -> &str {
    wave.trim_start_matches(|c| "*#@><^)}$!?(&".contains(c))
}

/// The first wave of a script entry body (`wave` or the first of `rndwave`).
fn first_wave(body: &[Node]) -> Option<String> {
    for n in body {
        match (&n.value, n.key.to_ascii_lowercase().as_str()) {
            (Value::Str(w), "wave") => return Some(strip_wave_prefix(w).replace('\\', "/")),
            (Value::Block(b), "rndwave") => {
                if let Some(w) = b.iter().find_map(|n| if let Value::Str(w) = &n.value { Some(w) } else { None }) {
                    return Some(strip_wave_prefix(w).replace('\\', "/"));
                }
            }
            _ => {}
        }
    }
    None
}

fn read_script(fs: &GameFs, path: &str, out: &mut Vec<(String, String)>) {
    let Some(bytes) = fs.read(path) else { return };
    let Ok(nodes) = kv::parse(&String::from_utf8_lossy(&bytes)) else { return };
    for n in &nodes {
        if let Value::Block(body) = &n.value {
            out.push((n.key.clone(), first_wave(body).unwrap_or_default()));
        }
    }
}

pub fn load(fs: &GameFs) -> SoundLists {
    let mut files: Vec<String> = ["wav", "mp3"].iter().flat_map(|e| fs.list("sound/", e)).map(|f| f.trim_start_matches("sound/").to_string()).collect();
    files.sort();
    let mut scripts = vec![];
    let mut paths: Vec<String> = vec![];
    if let Some(m) = fs.read("scripts/game_sounds_manifest.txt").and_then(|b| kv::parse(&String::from_utf8_lossy(&b)).ok()) {
        for block in &m {
            if let Value::Block(b) = &block.value {
                paths.extend(b.iter().filter(|n| n.key.eq_ignore_ascii_case("precache_file")).filter_map(|n| if let Value::Str(s) = &n.value { Some(s.clone()) } else { None }));
            }
        }
    }
    if paths.is_empty() {
        paths = fs.list("scripts/", "txt").into_iter().filter(|p| p.starts_with("scripts/game_sounds")).collect();
    }
    for p in paths {
        read_script(fs, &p, &mut scripts);
    }
    scripts.sort_by(|a, b| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase()));
    scripts.dedup_by(|a, b| a.0.eq_ignore_ascii_case(&b.0));
    SoundLists { files, scripts }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_entries_resolve_first_wave() {
        let nodes = kv::parse("\"A.One\" { \"channel\" \"CHAN_AUTO\" \"wave\" \"*ambient\\wind.wav\" }\n\"A.Two\" { \"rndwave\" { \"wave\" \")x/y.wav\" \"wave\" \"z.wav\" } }").unwrap();
        let waves: Vec<String> = nodes.iter().filter_map(|n| if let Value::Block(b) = &n.value { first_wave(b) } else { None }).collect();
        assert_eq!(waves, ["ambient/wind.wav", "x/y.wav"]);
    }
}
