//! func_instance support: loads referenced VMFs and flattens their brush geometry into world space.

use crate::doc::angles_matrix;
use crate::geom::{Plane, SolidGeo};
use rhammer_formats::vmf::{Map, Solid, TexAxis};
use glam::{DMat3, DVec3};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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

#[derive(Default, Clone)]
pub struct InstGeo {
    pub faces: Vec<InstFace>,
    /// World-space polygons, parallel to aces.
    pub polys: Vec<Vec<DVec3>>,
    pub min: DVec3,
    pub max: DVec3,
}

#[derive(Default)]
pub struct InstCache {
    pub files: HashMap<PathBuf, Option<Rc<Map>>>,
    /// number of new files that may still be loaded in this pass
    pub budget: i32,
    pub starved: bool,
}

impl InstCache {
    pub fn load(&mut self, path: &Path) -> Option<Rc<Map>> {
        if let Some(m) = self.files.get(path) {
            return m.clone();
        }
        if self.budget <= 0 {
            self.starved = true;
            return None;
        }
        self.budget -= 1;
        let m = Map::load(path).ok().map(Rc::new);
        self.files.insert(path.to_path_buf(), m.clone());
        m
    }
}

fn resolve(file: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let f = file.replace('\\', "/");
    for d in dirs {
        let p = d.join(&f);
        if p.exists() {
            return Some(p);
        }
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

/// Collect faces of all instances inside `map` (one level of `func_instance` entities, recursing
/// into the instances themselves up to `depth`).
pub fn collect_inner(cache: &mut InstCache, map: &Map, dirs: &[PathBuf], rot: DMat3, trans: DVec3, depth: u32, out: &mut Vec<InstFace>) {
    for s in &map.world.solids {
        add_solid(s, rot, trans, out);
    }
    for e in &map.entities {
        if e.classname() == "func_instance" {
            if depth == 0 {
                continue;
            }
            if let Some(f) = e.get("file") {
                if let Some(p) = resolve(f, dirs) {
                    if let Some(m) = cache.load(&p) {
                        let r = angles_matrix(e.angles());
                        let t = rot * e.origin() + trans;
                        let mut d2 = dirs.to_vec();
                        if let Some(parent) = p.parent() {
                            d2.insert(0, parent.to_path_buf());
                        }
                        collect_inner(cache, &m, &d2, rot * r, t, depth - 1, out);
                    }
                }
            }
        } else {
            for s in &e.solids {
                add_solid(s, rot, trans, out);
            }
        }
    }
}

/// Geometry of a single instance entity of the document.
pub fn instance_geo(cache: &mut InstCache, file: &str, origin: DVec3, angles: DVec3, dirs: &[PathBuf]) -> Option<InstGeo> {
    let p = resolve(file, dirs)?;
    let m = cache.load(&p)?;
    let mut d2 = dirs.to_vec();
    if let Some(parent) = p.parent() {
        d2.insert(0, parent.to_path_buf());
    }
    let mut faces = vec![];
    collect_inner(cache, &m, &d2, angles_matrix(angles), origin, 3, &mut faces);
    if faces.is_empty() {
        return None;
    }
    let mut min = DVec3::splat(f64::MAX);
    let mut max = DVec3::splat(f64::MIN);
    for f in &faces {
        for p in f.world_poly() {
            min = min.min(p);
            max = max.max(p);
        }
    }
    let polys = faces.iter().map(|f| f.world_poly()).collect();
    Some(InstGeo { faces, polys, min, max })
}
