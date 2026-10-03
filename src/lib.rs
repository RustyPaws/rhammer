#![allow(dead_code)]

#[cfg(all(feature = "local", feature = "web"))]
compile_error!("features `local` and `web` are mutually exclusive");
#[cfg(not(any(feature = "local", feature = "web")))]
compile_error!("enable exactly one of the features `local` or `web`");

pub mod app;
pub mod assets;
#[cfg(feature = "local")]
pub mod compile;
pub mod config;
pub mod editor;
pub mod formats;
pub mod kv;
pub mod platform;
pub mod render3d;
pub mod ui;
