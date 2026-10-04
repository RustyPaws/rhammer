use serde::{Deserialize, Serialize};

/// Parameters of the "Run Map" dialog (per config).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
