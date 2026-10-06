//! Game asset access: layered game file system, VPK archives, materials and MDL models.

#[cfg(feature = "local")]
pub mod audio;
pub mod gamefs;
pub mod materials;
pub mod mdl;
pub mod searchpaths;
pub mod sounds;
pub mod steam;
pub mod vpk;

pub use gamefs::GameFs;
pub use materials::{MatInfo, Materials};
