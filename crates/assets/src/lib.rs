//! Game asset access: layered game file system, VPK archives, materials and MDL models.

pub mod gamefs;
pub mod materials;
pub mod mdl;
pub mod vpk;

pub use gamefs::GameFs;
pub use materials::{MatInfo, Materials};
