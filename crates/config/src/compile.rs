use crate::{flag_str, get_string};
use rhammer_kv::{Node, NodeList};

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

impl CompileSettings {
    pub(crate) fn from_nodes(c: &[Node]) -> CompileSettings {
        let d = CompileSettings::default();
        let flag = |k: &str, def: bool| c.get_str(k).map(|v| v == "1").unwrap_or(def);
        CompileSettings {
            run_bsp: flag("run_bsp", d.run_bsp),
            run_vis: flag("run_vis", d.run_vis),
            run_light: flag("run_light", d.run_light),
            bsp_params: get_string(c, "bsp_params").unwrap_or(d.bsp_params),
            vis_params: get_string(c, "vis_params").unwrap_or(d.vis_params),
            light_params: get_string(c, "light_params").unwrap_or(d.light_params),
            copy_to_game: flag("copy_to_game", d.copy_to_game),
            launch_game: flag("launch_game", d.launch_game),
            game_params: get_string(c, "game_params").unwrap_or(d.game_params),
            no_waiting: flag("no_waiting", d.no_waiting),
        }
    }

    pub(crate) fn to_node(&self) -> Node {
        Node::block(
            "compile",
            vec![
                Node::str("run_bsp", flag_str(self.run_bsp)),
                Node::str("run_vis", flag_str(self.run_vis)),
                Node::str("run_light", flag_str(self.run_light)),
                Node::str("bsp_params", self.bsp_params.clone()),
                Node::str("vis_params", self.vis_params.clone()),
                Node::str("light_params", self.light_params.clone()),
                Node::str("copy_to_game", flag_str(self.copy_to_game)),
                Node::str("launch_game", flag_str(self.launch_game)),
                Node::str("game_params", self.game_params.clone()),
                Node::str("no_waiting", flag_str(self.no_waiting)),
            ],
        )
    }
}
