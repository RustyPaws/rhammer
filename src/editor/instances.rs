//! func_instance support: loads referenced VMFs and flattens their brush geometry into world space.
//!
//! Instance files are located the way VBSP and Hammer do it (`DeterminePath`):
//! 1. relative to the directory of the VMF that contains the `func_instance`;
//! 2. relative to the `maps` directory found in that VMF's path;
//! 3. relative to any fallback directories (the game's map directory).
//!
//! Nested instances resolve against *their own* file, not the top-level map.
//!
//! Brushes are flattened into faces; point entities with a studio model become [`InstProp`]s.

use crate::editor::doc::{angles_matrix, entity_model};
use crate::editor::geom::{Plane, SolidGeo};
use crate::formats::fgd::Fgd;
use crate::formats::vmf::{Map, Solid, TexAxis};
use crate::platform::{SharedVfs, Vfs};
use glam::{DMat3, DVec3};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Maximum nesting of instances inside instances.
const MAX_DEPTH: usize = 16;

#[derive(Clone)]
pub struct InstFace {
    pub material: String,
    pub uaxis: TexAxis,
    pub vaxis: TexAxis,
    /// Polygon in the instance's local space (texture coordinates are computed from it).
    pub local: Vec<DVec3>,
    pub normal: DVec3,
    pub rot: DMat3,
    pub trans: DVec3,
}

impl InstFace {
    pub fn world_poly(&self) -> Vec<DVec3> {
        self.local.iter().map(|p| self.rot * *p + self.trans).collect()
    }
}

/// A studio model placed by a point entity inside an instance, in world space.
#[derive(Clone)]
pub struct InstProp {
    /// Lowercase model path with forward slashes, as returned by `entity_model`.
    pub model: String,
    pub origin: DVec3,
    pub rot: DMat3,
    pub skin: usize,
    pub scale: f64,
}

#[derive(Default, Clone)]
pub struct InstGeo {
    pub faces: Vec<InstFace>,
    pub props: Vec<InstProp>,
    /// World-space polygons, parallel to `faces`.
    pub polys: Vec<Vec<DVec3>>,
    pub min: DVec3,
    pub max: DVec3,
}

struct CacheEntry {
    modified: Option<u64>,
    /// `None` if the file failed to parse.
    map: Option<Rc<Map>>,
}

/// Loaded instance VMFs, keyed by canonical path. Entries are reloaded when the file changes on disk.
#[derive(Default)]
pub struct InstCache {
    vfs: Option<SharedVfs>,
    files: HashMap<PathBuf, CacheEntry>,
    /// number of files that may still be (re)loaded in this pass
    pub budget: i32,
    /// set when a load was skipped because the budget ran out
    pub starved: bool,
}

impl InstCache {
    pub fn new(vfs: SharedVfs) -> InstCache {
        InstCache { vfs: Some(vfs), ..Default::default() }
    }

    /// File access used to resolve and load instance files.
    fn vfs(&self) -> Option<SharedVfs> {
        self.vfs.clone()
    }

    pub fn clear(&mut self) {
        self.files.clear();
    }

    fn load(&mut self, path: &Path) -> Option<Rc<Map>> {
        let vfs = self.vfs.clone()?;
        let modified = vfs.modified(path);
        if let Some(e) = self.files.get(path) {
            if e.modified == modified {
                return e.map.clone();
            }
        }
        if self.budget <= 0 {
            self.starved = true;
            // keep showing the stale copy until there is budget to reload it
            return self.files.get(path).and_then(|e| e.map.clone());
        }
        self.budget -= 1;
        let map = vfs
            .read(path)
            .and_then(|b| Map::parse(&String::from_utf8_lossy(&b)).ok())
            .map(Rc::new);
        self.files.insert(path.to_path_buf(), CacheEntry { modified, map: map.clone() });
        map
    }
}

/// The outermost `maps` directory in `dir`'s path, if any (matches VBSP's search for `\maps\`).
fn maps_root(dir: &Path) -> Option<PathBuf> {
    let mut root = PathBuf::new();
    for c in dir.components() {
        root.push(c);
        if c.as_os_str().eq_ignore_ascii_case("maps") {
            return Some(root);
        }
    }
    None
}

/// Resolve the `file` key of a `func_instance` contained in a VMF located in `base_dir`.
pub fn resolve(vfs: &dyn Vfs, file: &str, base_dir: Option<&Path>, fallback: &[PathBuf]) -> Option<PathBuf> {
    let file = file.trim();
    if file.is_empty() {
        return None;
    }
    let mut rel = PathBuf::from(file.replace('\\', "/"));
    rel.set_extension("vmf");

    let mut dirs: Vec<PathBuf> = vec![];
    if let Some(d) = base_dir {
        dirs.push(d.to_path_buf());
        dirs.extend(maps_root(d));
    }
    dirs.extend(fallback.iter().cloned());

    let mut seen = Vec::with_capacity(dirs.len());
    for d in dirs {
        if seen.contains(&d) {
            continue;
        }
        let p = d.join(&rel);
        if vfs.is_file(&p) {
            return Some(vfs.canonical(&p));
        }
        seen.push(d);
    }
    None
}

fn add_solid(s: &Solid, rot: DMat3, trans: DVec3, out: &mut Vec<InstFace>) {
    let geo = SolidGeo::build(s);
    for (sd, poly) in s.sides.iter().zip(&geo.polys) {
        if poly.len() < 3 {
            continue;
        }
        let n = Plane::from_points(&sd.plane).map(|p| p.n).unwrap_or(DVec3::Z);
        out.push(InstFace {
            material: sd.material.clone(),
            uaxis: sd.uaxis.clone(),
            vaxis: sd.vaxis.clone(),
            local: poly.clone(),
            normal: n,
            rot,
            trans,
        });
    }
}

struct Collector<'a> {
    cache: &'a mut InstCache,
    fallback: &'a [PathBuf],
    fgd: &'a Fgd,
    /// files currently being expanded, to break include cycles
    stack: Vec<PathBuf>,
    faces: Vec<InstFace>,
    props: Vec<InstProp>,
}

impl Collector<'_> {
    /// Expand the instance `file` referenced from a VMF in `base_dir`, placed with `rot`/`trans`.
    fn instance(&mut self, file: &str, base_dir: Option<&Path>, rot: DMat3, trans: DVec3) {
        if self.stack.len() >= MAX_DEPTH {
            return;
        }
        let Some(vfs) = self.cache.vfs() else { return };
        let Some(path) = resolve(&*vfs, file, base_dir, self.fallback) else { return };
        if self.stack.contains(&path) {
            return;
        }
        let Some(map) = self.cache.load(&path) else { return };
        self.stack.push(path);
        let dir = self.stack.last().and_then(|p| p.parent()).map(Path::to_path_buf);
        self.map(&map, dir.as_deref(), rot, trans);
        self.stack.pop();
    }

    /// Objects hidden inside the instance file (hidden visgroups, `hidden` blocks) are skipped,
    /// as Hammer does: instance files often keep reference copies of surrounding geometry there.
    fn map(&mut self, map: &Map, dir: Option<&Path>, rot: DMat3, trans: DVec3) {
        for s in map.world.solids.iter().filter(|s| !s.hidden) {
            add_solid(s, rot, trans, &mut self.faces);
        }
        for e in map.entities.iter().filter(|e| !e.hidden) {
            if e.classname().eq_ignore_ascii_case("func_instance") {
                if let Some(f) = e.get("file") {
                    self.instance(f, dir, rot * angles_matrix(e.angles()), rot * e.origin() + trans);
                }
            } else if e.solids.is_empty() {
                if let Some(model) = entity_model(e, self.fgd) {
                    let num = |k: &str| e.get(k).and_then(|v| v.trim().parse::<f64>().ok());
                    self.props.push(InstProp {
                        model,
                        origin: rot * e.origin() + trans,
                        rot: rot * angles_matrix(e.angles()),
                        skin: num("skin").map_or(0, |s| s.max(0.0) as usize),
                        scale: num("modelscale").or_else(|| num("uniformscale")).unwrap_or(1.0),
                    });
                }
            } else {
                for s in e.solids.iter().filter(|s| !s.hidden) {
                    add_solid(s, rot, trans, &mut self.faces);
                }
            }
        }
    }
}

/// Geometry of a single `func_instance` of the document.
///
/// `map_dir` is the directory of the document's VMF (`None` for an unsaved map) and `fallback`
/// the extra directories to search, in order. The bounds cover brush geometry and prop origins;
/// prop hulls are added by the caller once their models are loaded.
pub fn instance_geo(
    cache: &mut InstCache,
    fgd: &Fgd,
    file: &str,
    origin: DVec3,
    angles: DVec3,
    map_dir: Option<&Path>,
    fallback: &[PathBuf],
) -> Option<InstGeo> {
    let mut c = Collector { cache, fallback, fgd, stack: vec![], faces: vec![], props: vec![] };
    c.instance(file, map_dir, angles_matrix(angles), origin);
    let (faces, props) = (c.faces, c.props);
    if faces.is_empty() && props.is_empty() {
        return None;
    }
    let polys: Vec<Vec<DVec3>> = faces.iter().map(InstFace::world_poly).collect();
    let (min, max) = polys
        .iter()
        .flatten()
        .chain(props.iter().map(|p| &p.origin))
        .fold((DVec3::splat(f64::MAX), DVec3::splat(f64::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    Some(InstGeo { faces, props, polys, min, max })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_root_finds_outermost_maps_dir() {
        let d = Path::new("/game/sdk_content/maps/instances/maps/elevator");
        assert_eq!(maps_root(d), Some(PathBuf::from("/game/sdk_content/maps")));
        assert_eq!(maps_root(Path::new("/game/MAPS/sub")), Some(PathBuf::from("/game/MAPS")));
        assert_eq!(maps_root(Path::new("/game/mapsrc")), None);
    }

    #[test]
    fn resolve_follows_vbsp_order() {
        let root = std::env::temp_dir().join(format!("rhammer_inst_{}", std::process::id()));
        let maps = root.join("sdk_content/maps");
        let sub = maps.join("mymaps");
        let other = root.join("other_maps");
        for d in [&sub, &maps.join("instances"), &other.join("instances"), &sub.join("instances/dir.vmf")] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(maps.join("instances/lift.vmf"), "").unwrap();
        std::fs::write(other.join("instances/lift.vmf"), "").unwrap();
        let fallback = vec![other.clone()];
        let canon = |p: PathBuf| std::fs::canonicalize(p).unwrap();

        // maps root wins over the configured fallback, extension is optional, slashes are normalised
        let want = canon(maps.join("instances/lift.vmf"));
        assert_eq!(resolve(&crate::platform::LocalFs, "instances/lift.vmf", Some(&sub), &fallback), Some(want.clone()));
        assert_eq!(resolve(&crate::platform::LocalFs, "instances\\lift", Some(&sub), &fallback), Some(want));
        // unsaved map: only the fallback is searched
        assert_eq!(resolve(&crate::platform::LocalFs, "instances/lift.vmf", None, &fallback), Some(canon(other.join("instances/lift.vmf"))));
        // directories and empty keys never resolve
        assert_eq!(resolve(&crate::platform::LocalFs, "instances/dir.vmf", Some(&sub), &[]), None);
        assert_eq!(resolve(&crate::platform::LocalFs, "  ", Some(&sub), &fallback), None);

        std::fs::remove_dir_all(&root).ok();
    }
}
