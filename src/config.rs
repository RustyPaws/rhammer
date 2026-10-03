//! Game configurations, modelled after Hammer's GameConfig.txt ("Options > Game Configurations").
//! Stored in a KeyValues file next to the executable's working dir (`rhammer_config.txt`),
//! and importable from Hammer's own `bin/GameConfig.txt`.

use crate::kv::{self, Node, NodeList, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct GameConfig {
    pub name: String,
    pub game_dir: String,
    /// FGD files (GameData0, GameData1, ...).
    pub fgds: Vec<String>,
    pub texture_format: String,
    pub map_format: String,
    pub default_texture_scale: f64,
    pub default_lightmap_scale: i32,
    pub game_exe: String,
    pub game_exe_dir: String,
    pub default_solid_entity: String,
    pub default_point_entity: String,
    pub bsp_exe: String,
    pub vis_exe: String,
    pub light_exe: String,
    pub map_dir: String,
    pub bsp_dir: String,
    pub prefab_dir: String,
    pub cordon_texture: String,
}

impl Default for GameConfig {
    fn default() -> Self {
        GameConfig {
            name: "New game".into(),
            game_dir: String::new(),
            fgds: vec![],
            texture_format: "5".into(),
            map_format: "4".into(),
            default_texture_scale: 0.25,
            default_lightmap_scale: 16,
            game_exe: String::new(),
            game_exe_dir: String::new(),
            default_solid_entity: "func_detail".into(),
            default_point_entity: "info_player_start".into(),
            bsp_exe: String::new(),
            vis_exe: String::new(),
            light_exe: String::new(),
            map_dir: String::new(),
            bsp_dir: String::new(),
            prefab_dir: String::new(),
            cordon_texture: "tools\\toolsskybox".into(),
        }
    }
}

/// Parameters of the "Run Map" dialog (per config).
#[derive(Clone, Debug, PartialEq)]
pub struct CompileSettings {
    pub run_bsp: bool,
    pub run_vis: bool,
    pub run_light: bool,
    pub bsp_params: String,
    pub vis_params: String,
    pub light_params: String,
    pub copy_to_game: bool,
    pub launch_game: bool,
    pub game_params: String,
    pub no_waiting: bool,
}

impl Default for CompileSettings {
    fn default() -> Self {
        CompileSettings {
            run_bsp: true,
            run_vis: true,
            run_light: true,
            bsp_params: "-game $gamedir \"$path\\$file\"".into(),
            vis_params: "-game $gamedir \"$path\\$file\"".into(),
            light_params: "-game $gamedir \"$path\\$file\"".into(),
            copy_to_game: true,
            launch_game: true,
            game_params: "-game $gamedir -hammer +map $file -sw -w 1280 -h 720".into(),
            no_waiting: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Settings {
    pub games: Vec<GameConfig>,
    pub active: usize,
    pub compile: CompileSettings,
    pub last_map: String,
}

fn s(nodes: &[Node], key: &str) -> Option<String> {
    nodes.get_str(key).map(|v| v.to_string())
}

impl GameConfig {
    fn from_nodes(name: &str, game_dir: Option<String>, hammer: &[Node]) -> GameConfig {
        let mut g = GameConfig { name: name.to_string(), ..Default::default() };
        g.game_dir = game_dir.unwrap_or_default();
        for i in 0..16 {
            if let Some(f) = s(hammer, &format!("GameData{i}")) {
                g.fgds.push(f);
            }
        }
        macro_rules! set {
            ($f:ident, $k:expr) => {
                if let Some(v) = s(hammer, $k) {
                    g.$f = v;
                }
            };
        }
        set!(texture_format, "TextureFormat");
        set!(map_format, "MapFormat");
        set!(game_exe, "GameExe");
        set!(game_exe_dir, "GameExeDir");
        set!(default_solid_entity, "DefaultSolidEntity");
        set!(default_point_entity, "DefaultPointEntity");
        set!(bsp_exe, "BSP");
        set!(vis_exe, "Vis");
        set!(light_exe, "Light");
        set!(map_dir, "MapDir");
        set!(bsp_dir, "BSPDir");
        set!(prefab_dir, "PrefabDir");
        set!(cordon_texture, "CordonTexture");
        if let Some(v) = s(hammer, "DefaultTextureScale").and_then(|v| v.parse().ok()) {
            g.default_texture_scale = v;
        }
        if let Some(v) = s(hammer, "DefaultLightmapScale").and_then(|v| v.parse().ok()) {
            g.default_lightmap_scale = v;
        }
        g
    }

    fn to_node(&self) -> Node {
        let mut c = vec![Node::str("GameDir", self.game_dir.clone())];
        let mut h = Vec::new();
        for (i, f) in self.fgds.iter().enumerate() {
            h.push(Node::str(format!("GameData{i}"), f.clone()));
        }
        let pairs: [(&str, String); 15] = [
            ("TextureFormat", self.texture_format.clone()),
            ("MapFormat", self.map_format.clone()),
            ("DefaultTextureScale", format!("{:.6}", self.default_texture_scale)),
            ("DefaultLightmapScale", self.default_lightmap_scale.to_string()),
            ("GameExe", self.game_exe.clone()),
            ("DefaultSolidEntity", self.default_solid_entity.clone()),
            ("DefaultPointEntity", self.default_point_entity.clone()),
            ("BSP", self.bsp_exe.clone()),
            ("Vis", self.vis_exe.clone()),
            ("Light", self.light_exe.clone()),
            ("GameExeDir", self.game_exe_dir.clone()),
            ("MapDir", self.map_dir.clone()),
            ("BSPDir", self.bsp_dir.clone()),
            ("PrefabDir", self.prefab_dir.clone()),
            ("CordonTexture", self.cordon_texture.clone()),
        ];
        for (k, v) in pairs {
            h.push(Node::str(k, v));
        }
        h.push(Node::str("MaterialExcludeCount", "0"));
        c.push(Node::block("Hammer", h));
        Node::block(format!("\"{}\"", self.name), c)
    }

    /// Resolve `$gamedir`, `$path`, `$file` style macros used in compile parameters.
    pub fn expand(&self, template: &str, map_path: &Path) -> String {
        let dir = map_path.parent().map(|p| p.display().to_string()).unwrap_or_default();
        let stem = map_path.file_stem().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        template
            .replace("$gamedir", &format!("\"{}\"", self.game_dir))
            .replace("$path", &dir)
            .replace("$file", &stem)
            .replace("$bspdir", &self.bsp_dir)
            .replace("$ext", "vmf")
    }

    /// Directory searched for `materials/`, `models/` etc. (the `GameDir`).
    pub fn game_path(&self) -> PathBuf {
        PathBuf::from(&self.game_dir)
    }
}

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
                    out.push(GameConfig::from_nodes(&g.key, s(c, "GameDir"), hammer));
                }
            }
        }
    }
    Ok(out)
}

fn b(v: bool) -> &'static str {
    if v { "1" } else { "0" }
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
                                st.games.push(GameConfig::from_nodes(
                                    g.key.trim_matches('"'),
                                    s(c, "GameDir"),
                                    hammer,
                                ));
                            }
                        }
                    }
                    if let Some(c) = rh.get_block("compile") {
                        let d = CompileSettings::default();
                        let flag = |k: &str, def: bool| c.get_str(k).map(|v| v == "1").unwrap_or(def);
                        st.compile = CompileSettings {
                            run_bsp: flag("run_bsp", d.run_bsp),
                            run_vis: flag("run_vis", d.run_vis),
                            run_light: flag("run_light", d.run_light),
                            bsp_params: s(c, "bsp_params").unwrap_or(d.bsp_params),
                            vis_params: s(c, "vis_params").unwrap_or(d.vis_params),
                            light_params: s(c, "light_params").unwrap_or(d.light_params),
                            copy_to_game: flag("copy_to_game", d.copy_to_game),
                            launch_game: flag("launch_game", d.launch_game),
                            game_params: s(c, "game_params").unwrap_or(d.game_params),
                            no_waiting: flag("no_waiting", d.no_waiting),
                        };
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
        let c = &self.compile;
        let comp = vec![
            Node::str("run_bsp", b(c.run_bsp)),
            Node::str("run_vis", b(c.run_vis)),
            Node::str("run_light", b(c.run_light)),
            Node::str("bsp_params", c.bsp_params.clone()),
            Node::str("vis_params", c.vis_params.clone()),
            Node::str("light_params", c.light_params.clone()),
            Node::str("copy_to_game", b(c.copy_to_game)),
            Node::str("launch_game", b(c.launch_game)),
            Node::str("game_params", c.game_params.clone()),
            Node::str("no_waiting", b(c.no_waiting)),
        ];
        let root = vec![Node::block(
            "rhammer",
            vec![
                Node::str("active", self.active.to_string()),
                Node::str("last_map", self.last_map.clone()),
                Node::block("games", games),
                Node::block("compile", comp),
            ],
        )];
        let _ = std::fs::write(Self::path(), kv::to_string(&root));
    }
}
