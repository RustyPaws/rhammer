use serde::{Deserialize, Serialize};

/// What a Run Map step does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepKind {
    /// Run a program (`exe` + `args`) in the map's folder and wait for it.
    #[default]
    Run,
    /// Copy the file `exe` into the folder `args`.
    Copy,
    /// Start a program (`exe` + `args`) without waiting, e.g. the game.
    Launch,
}

impl StepKind {
    pub const ALL: [StepKind; 3] = [StepKind::Run, StepKind::Copy, StepKind::Launch];

    pub fn label(self) -> &'static str {
        match self {
            StepKind::Run => "Run and wait",
            StepKind::Copy => "Copy file",
            StepKind::Launch => "Launch",
        }
    }

    /// Labels of the `exe` and `args` fields for this kind.
    pub fn field_labels(self) -> (&'static str, &'static str) {
        match self {
            StepKind::Copy => ("File", "Into folder"),
            _ => ("Program", "Arguments"),
        }
    }
}

/// One step of a preset. Text fields may use the macros `$gamedir $path $file $bspdir $ext`
/// and `$bsp_exe $vis_exe $light_exe $game_exe $gameexedir` of the game configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Step {
    pub name: String,
    pub enabled: bool,
    pub kind: StepKind,
    pub exe: String,
    pub args: String,
}

impl Default for Step {
    fn default() -> Self {
        Step { name: "New step".into(), enabled: true, kind: StepKind::Run, exe: String::new(), args: String::new() }
    }
}

impl Step {
    fn run(name: &str, exe: &str, args: &str) -> Step {
        Step { name: name.into(), enabled: true, kind: StepKind::Run, exe: exe.into(), args: args.into() }
    }
}

/// A named list of steps ("Full", "Fast", ...).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    pub name: String,
    pub steps: Vec<Step>,
}

impl Default for Preset {
    fn default() -> Self {
        Preset { name: "New preset".into(), steps: vec![] }
    }
}

const COMPILER_ARGS: &str = "-game $gamedir \"$path\\$file\"";
const GAME_ARGS: &str = "-game $gamedir -hammer +map $file -sw -w 1280 -h 720";

impl Preset {
    /// BSP / VIS / RAD (with extra `vis` / `light` arguments), copy the BSP, launch the game.
    fn standard(name: &str, bsp: &str, vis: &str, light: &str, steps: [bool; 3]) -> Preset {
        let with = |base: &str, extra: &str| if extra.is_empty() { base.to_string() } else { format!("{extra} {base}") };
        let mut list = vec![
            Step::run("BSP", "$bsp_exe", &with(COMPILER_ARGS, bsp)),
            Step::run("VIS", "$vis_exe", &with(COMPILER_ARGS, vis)),
            Step::run("RAD", "$light_exe", &with(COMPILER_ARGS, light)),
        ];
        for (s, on) in list.iter_mut().zip(steps) {
            s.enabled = on;
        }
        list.push(Step { name: "Copy BSP to game".into(), enabled: true, kind: StepKind::Copy, exe: "$path\\$file.bsp".into(), args: "$bspdir".into() });
        list.push(Step { name: "Run game".into(), enabled: true, kind: StepKind::Launch, exe: "$game_exe".into(), args: GAME_ARGS.into() });
        Preset { name: name.into(), steps: list }
    }

    pub fn defaults() -> Vec<Preset> {
        vec![
            Preset::standard("Full", "", "", "", [true; 3]),
            Preset::standard("Fast", "", "-fast", "-fast", [true; 3]),
            Preset::standard("BSP only", "", "", "", [true, false, false]),
        ]
    }
}

/// Run Map presets, kept in the settings file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompileSettings {
    pub presets: Vec<Preset>,
    /// Index of the preset F9 runs.
    pub active: usize,
}

impl Default for CompileSettings {
    fn default() -> Self {
        CompileSettings { presets: Preset::defaults(), active: 0 }
    }
}

impl CompileSettings {
    /// After loading: make sure there is always an active preset.
    pub fn normalize(&mut self) {
        if self.presets.is_empty() {
            self.presets = Preset::defaults();
        }
        if self.active >= self.presets.len() {
            self.active = 0;
        }
    }

    pub fn active_preset(&self) -> Option<&Preset> {
        self.presets.get(self.active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_round_trip_through_toml() {
        let cs = CompileSettings::default();
        let text = toml::to_string_pretty(&cs).unwrap();
        let back: CompileSettings = toml::from_str(&text).unwrap();
        assert_eq!(cs, back);
        assert_eq!(back.presets.len(), 3);
    }

    #[test]
    fn old_flat_settings_fall_back_to_defaults() {
        let mut cs: CompileSettings = toml::from_str("run_bsp = true\nbsp_params = \"x\"\n").unwrap();
        cs.normalize();
        assert_eq!(cs, CompileSettings::default());
    }
}
