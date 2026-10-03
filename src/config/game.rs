use crate::config::get_string as s;
use crate::kv::Node;
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

impl GameConfig {
    pub(crate) fn from_nodes(name: &str, game_dir: Option<String>, hammer: &[Node]) -> GameConfig {
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

    pub(crate) fn to_node(&self) -> Node {
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
