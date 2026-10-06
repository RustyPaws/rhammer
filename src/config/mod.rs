//! Configuration layer: game configurations (modelled after Hammer's GameConfig.txt),
//! per-config compile settings, and the persisted application settings.

mod compile;
mod game;
mod hammer;
mod settings;

pub use compile::CompileSettings;
pub use game::GameConfig;
pub use hammer::import_hammer;
pub use settings::{EditorOptions, Settings, View2dStyle};
