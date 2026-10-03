//! Brush geometry. A solid is stored as a set of planes (VMF style); polygons are derived
//! by clipping a huge quad on each plane by all the other planes.

use rhammer_formats::vmf::{Side, Solid, TexAxis};
use glam::DVec3;

pub const EPS: f64 = 0.01;

#[derive(Clone, Copy, Debug)]
pub struct Plane {
    pub n: DVec3,
    pub d: f64,
}

impl Plane {
    /// Source/Hammer convention: points are clockwise looking at the front of the face.
    pub fn from_points(p: &[DVec3; 3]) -> Option<Plane> {
        let n = (p[2] - p[0]).cross(p[1] - p[0]);
        let l = n.length();
        if l < 1e-9 {
            return None;
        }
        let n = n / l;
        Some(Plane { n, d: n.dot(p[0]) })
    }
    pub fn dist(&self, p: DVec3) -> f64 {
        self.n.dot(p) - self.d
    }
    pub fn flipped(&self) -> Plane {
        Plane { n: -self.n, d: -self.d }
    }
}

pub fn basis(n: DVec3) -> (DVec3, DVec3) {
    let helper = if n.z.abs() < 0.9 { DVec3::Z } else { DVec3::X };
    let u = helper.cross(n).normalize();
    let v = n.cross(u);
    // u x v == n
    (u, v)
}

fn clip(poly: &[DVec3], pl: &Plane) -> Vec<DVec3> {
    let mut out = Vec::with_capacity(poly.len() + 1);
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let da = pl.dist(a);
        let db = pl.dist(b);
        let a_in = da <= EPS;
        let b_in = db <= EPS;
        if a_in {
            out.push(a);
        }
        if (da < -EPS && db > EPS) || (da > EPS && db < -EPS) {
            let t = da / (da - db);
            out.push(a + (b - a) * t);
        }
        let _ = b_in;
    }
    out
}

/// Polygon (CCW seen from outside) for each plane; empty when the plane does not contribute.
pub fn polys_from_planes(planes: &[Plane]) -> Vec<Vec<DVec3>> {
    let mut res = Vec::with_capacity(planes.len());
    for (i, p) in planes.iter().enumerate() {
        let (u, v) = basis(p.n);
        let c = p.n * p.d;
        let b = 65536.0;
        let mut poly = vec![c - u * b - v * b, c + u * b - v * b, c + u * b + v * b, c - u * b + v * b];
        for (j, q) in planes.iter().enumerate() {
            if i == j {
                continue;
            }
            // coincident planes: keep only the first
            if (q.n - p.n).length() < 1e-6 && (q.d - p.d).abs() < EPS {
                if j < i {
                    poly.clear();
                    break;
                }
                continue;
            }
            poly = clip(&poly, q);
            if poly.len() < 3 {
                poly.clear();
                break;
            }
        }
        // remove duplicate consecutive points
        let mut cleaned: Vec<DVec3> = Vec::with_capacity(poly.len());
        for v in poly {
            if cleaned.last().map(|l| (*l - v).length() > 1e-4).unwrap_or(true) {
                cleaned.push(v);
            }
        }
        while cleaned.len() > 1 && (cleaned[0] - *cleaned.last().unwrap()).length() <= 1e-4 {
            cleaned.pop();
        }
        if cleaned.len() < 3 {
            cleaned.clear();
        }
        res.push(cleaned);
    }
    res
}

pub fn solid_planes(s: &Solid) -> Vec<Plane> {
    s.sides
        .iter()
        .map(|sd| Plane::from_points(&sd.plane).unwrap_or(Plane { n: DVec3::Z, d: 0.0 }))
        .collect()
}

/// Cached derived geometry of a solid.
#[derive(Clone, Debug, Default)]
pub struct SolidGeo {
    pub polys: Vec<Vec<DVec3>>,
    pub min: DVec3,
    pub max: DVec3,
    pub valid: bool,
}

impl SolidGeo {
    pub fn build(s: &Solid) -> SolidGeo {
        let planes = solid_planes(s);
        let polys = polys_from_planes(&planes);
        let mut min = DVec3::splat(f64::MAX);
        let mut max = DVec3::splat(f64::MIN);
        let mut any = false;
        for p in &polys {
            for v in p {
                min = min.min(*v);
                max = max.max(*v);
                any = true;
            }
        }
        if !any {
            min = DVec3::ZERO;
            max = DVec3::ZERO;
        }
        SolidGeo { polys, min, max, valid: any }
    }
}

/// Pick three well-conditioned vertices (clockwise from the front) as the VMF plane definition.
pub fn plane_points(poly_ccw: &[DVec3]) -> Option<[DVec3; 3]> {
    let cw: Vec<DVec3> = poly_ccw.iter().rev().copied().collect();
    if cw.len() < 3 {
        return None;
    }
    let a = cw[0];
    let mut best: Option<(f64, usize, usize)> = None;
    for i in 1..cw.len() {
        for j in i + 1..cw.len() {
            let area = (cw[i] - a).cross(cw[j] - a).length();
            if best.map(|b| area > b.0 + 1e-6).unwrap_or(true) {
                best = Some((area, i, j));
            }
            // first triple is preferred when it is not degenerate -> matches Hammer output for boxes
            if i == 1 && j == 2 && area > 1e-3 {
                return Some([cw[0], cw[1], cw[2]]);
            }
        }
    }
    let (area, i, j) = best?;
    if area < 1e-6 {
        return None;
    }
    Some([cw[0], cw[i], cw[j]])
}

pub fn snap_near(v: DVec3) -> DVec3 {
    let f = |x: f64| {
        let r = x.round();
        if (x - r).abs() < 1e-4 {
            r
        } else {
            (x * 1000.0).round() / 1000.0
        }
    };
    DVec3::new(f(v.x), f(v.y), f(v.z))
}

/// Default world-aligned texture axes for a face normal (matches Source's TextureAxisFromPlane).
pub fn default_axes(n: DVec3, scale: f64) -> (TexAxis, TexAxis) {
    let a = n.abs();
    let (u, v) = if a.z >= a.x && a.z >= a.y {
        (DVec3::X, DVec3::NEG_Y)
    } else if a.x >= a.y {
        (DVec3::Y, DVec3::NEG_Z)
    } else {
        (DVec3::X, DVec3::NEG_Z)
    };
    (TexAxis { vec: u, shift: 0.0, scale }, TexAxis { vec: v, shift: 0.0, scale })
}

pub fn new_side(poly_ccw: &[DVec3], n: DVec3, material: &str, id: u32, tex_scale: f64, lightmap: i32) -> Option<Side> {
    let plane = plane_points(poly_ccw)?;
    let (u, v) = default_axes(n, tex_scale);
    Some(Side {
        id,
        plane: [snap_near(plane[0]), snap_near(plane[1]), snap_near(plane[2])],
        material: material.to_string(),
        uaxis: u,
        vaxis: v,
        rotation: 0.0,
        lightmap,
        smoothing: 0,
        dispinfo: None,
        extra: vec![],
    })
}

/// Build sides from a convex set of planes. IDs are allocated through `next_id`.
pub fn solid_from_planes(
    planes: &[Plane],
    material: &str,
    next_id: &mut u32,
    tex_scale: f64,
    lightmap: i32,
) -> Option<Solid> {
    let polys = polys_from_planes(planes);
    let mut sides = Vec::new();
    for (p, pl) in polys.iter().zip(planes) {
        if p.is_empty() {
            continue;
        }
        *next_id += 1;
        sides.push(new_side(p, pl.n, material, *next_id, tex_scale, lightmap)?);
    }
    if sides.len() < 4 {
        return None;
    }
    *next_id += 1;
    Some(Solid { id: *next_id, sides, editor: vec![], hidden: false })
}

pub fn box_planes(min: DVec3, max: DVec3) -> Vec<Plane> {
    vec![
        Plane { n: DVec3::X, d: max.x },
        Plane { n: DVec3::NEG_X, d: -min.x },
        Plane { n: DVec3::Y, d: max.y },
        Plane { n: DVec3::NEG_Y, d: -min.y },
        Plane { n: DVec3::Z, d: max.z },
        Plane { n: DVec3::NEG_Z, d: -min.z },
    ]
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Primitive {
    Block,
    Wedge,
    Cylinder,
    Pyramid,
    Cone,
}

impl Primitive {
    pub const ALL: [Primitive; 5] = [Primitive::Block, Primitive::Wedge, Primitive::Cylinder, Primitive::Pyramid, Primitive::Cone];
    pub fn name(&self) -> &'static str {
        match self {
            Primitive::Block => "Block",
            Primitive::Wedge => "Wedge",
            Primitive::Cylinder => "Cylinder",
            Primitive::Pyramid => "Spike (pyramid)",
            Primitive::Cone => "Cone",
        }
    }
}

pub fn primitive_planes(kind: Primitive, min: DVec3, max: DVec3, sides: usize) -> Vec<Plane> {
    let c = (min + max) * 0.5;
    let h = (max - min) * 0.5;
    match kind {
        Primitive::Block => box_planes(min, max),
        Primitive::Wedge => {
            // slope from the low end at +x down... high at min.x, low at max.x
            let mut p = box_planes(min, max);
            p.remove(0); // remove +X face
            // slanted plane through (max.x, z=min.z) and (min.x, z=max.z)
            let dir = DVec3::new(max.x - min.x, 0.0, max.z - min.z);
            let n = DVec3::new(dir.z, 0.0, dir.x).normalize();
            p.push(Plane { n, d: n.dot(DVec3::new(max.x, c.y, min.z)) });
            p
        }
        Primitive::Cylinder => {
            let mut p = vec![Plane { n: DVec3::Z, d: max.z }, Plane { n: DVec3::NEG_Z, d: -min.z }];
            let n = sides.max(3);
            for i in 0..n {
                // planes tangent to the polygon inscribed in the ellipse
                let a0 = std::f64::consts::TAU * i as f64 / n as f64;
                let a1 = std::f64::consts::TAU * (i + 1) as f64 / n as f64;
                let p0 = DVec3::new(c.x + h.x * a0.cos(), c.y + h.y * a0.sin(), 0.0);
                let p1 = DVec3::new(c.x + h.x * a1.cos(), c.y + h.y * a1.sin(), 0.0);
                let e = p1 - p0;
                let nn = DVec3::new(e.y, -e.x, 0.0).normalize();
                p.push(Plane { n: nn, d: nn.dot(p0) });
            }
            p
        }
        Primitive::Pyramid | Primitive::Cone => {
            let mut p = vec![Plane { n: DVec3::NEG_Z, d: -min.z }];
            let n = if kind == Primitive::Pyramid { 4 } else { sides.max(3) };
            let apex = DVec3::new(c.x, c.y, max.z);
            for i in 0..n {
                let off = if kind == Primitive::Pyramid { std::f64::consts::FRAC_PI_4 } else { 0.0 };
                let k = if kind == Primitive::Pyramid { 2f64.sqrt() } else { 1.0 };
                let a0 = off + std::f64::consts::TAU * i as f64 / n as f64;
                let a1 = off + std::f64::consts::TAU * (i + 1) as f64 / n as f64;
                let p0 = DVec3::new(c.x + h.x * k * a0.cos(), c.y + h.y * k * a0.sin(), min.z);
                let p1 = DVec3::new(c.x + h.x * k * a1.cos(), c.y + h.y * k * a1.sin(), min.z);
                let nn = (p1 - p0).cross(apex - p0);
                let nn = DVec3::new(nn.x, nn.y, nn.z).normalize();
                let nn = if nn.dot(DVec3::new(p0.x - c.x, p0.y - c.y, 0.0)) < 0.0 { -nn } else { nn };
                p.push(Plane { n: nn, d: nn.dot(apex) });
            }
            p
        }
    }
}

/// Apply a point transform to a solid. Planes are re-derived from the transformed polygons.
/// `lock` adjusts texture axes for rigid transforms (rotation `rot` + translation `t`).
pub fn transform_solid(s: &mut Solid, f: &dyn Fn(DVec3) -> DVec3, rigid: Option<&dyn Fn(DVec3) -> DVec3>, flip: bool) {
    let geo = SolidGeo::build(s);
    let origin_t = f(DVec3::ZERO);
    for (sd, poly) in s.sides.iter_mut().zip(&geo.polys) {
        if poly.len() < 3 {
            // degenerate side: move its raw plane points
            for p in sd.plane.iter_mut() {
                *p = f(*p);
            }
            continue;
        }
        // a displacement's start corner must follow the face
        if let Some(d) = sd.dispinfo.as_mut() {
            for n in d.iter_mut() {
                if n.key.eq_ignore_ascii_case("startposition") {
                    if let rhammer_kv::Value::Str(s) = &mut n.value {
                        if let Some(v) = rhammer_formats::vmf::parse_vec3(s) {
                            *s = format!("[{}]", rhammer_formats::vmf::fmt_vec3(snap_near(f(v))));
                        }
                    }
                }
            }
        }
        let mut mapped: Vec<DVec3> = poly.iter().map(|v| f(*v)).collect();
        if flip {
            mapped.reverse();
        }
        if let Some(pts) = plane_points(&mapped) {
            sd.plane = [snap_near(pts[0]), snap_near(pts[1]), snap_near(pts[2])];
        }
        if let Some(rot) = rigid {
            // rot maps directions (translation excluded): rot(v) = f(v) - f(0)
            let ru = rot(sd.uaxis.vec);
            let rv = rot(sd.vaxis.vec);
            sd.uaxis.shift -= origin_t.dot(ru) / sd.uaxis.scale;
            sd.vaxis.shift -= origin_t.dot(rv) / sd.vaxis.scale;
            sd.uaxis.vec = ru;
            sd.vaxis.vec = rv;
        }
    }
}

/// Convenience: translate a solid keeping the texture lock.
pub fn translate_solid(s: &mut Solid, t: DVec3) {
    transform_solid(s, &|p| p + t, Some(&|d| d), false);
}

/// Split a solid by a plane. Returns (front, back) where the sides may be `None` if empty.
pub fn clip_solid(s: &Solid, cut: &Plane, material: &str, next_id: &mut u32) -> (Option<Solid>, Option<Solid>) {
    let mk = |pl: Plane, next_id: &mut u32| -> Option<Solid> {
        let mut planes = solid_planes(s);
        planes.push(pl);
        let polys = polys_from_planes(&planes);
        let cut_idx = planes.len() - 1;
        if polys[cut_idx].is_empty() {
            return None;
        }
        let mut sides = Vec::new();
        for (i, (sd, poly)) in s.sides.iter().zip(&polys).enumerate() {
            if poly.is_empty() {
                continue;
            }
            let mut n = sd.clone();
            n.id = {
                *next_id += 1;
                *next_id
            };
            n.dispinfo = None;
            let _ = i;
            sides.push(n);
        }
        let mut cutside = new_side(&polys[cut_idx], pl.n, material, {
            *next_id += 1;
            *next_id
        }, 0.25, 16)?;
        // inherit material scale from the first original side
        if let Some(first) = s.sides.first() {
            cutside.material = if material.is_empty() { first.material.clone() } else { material.to_string() };
        }
        sides.push(cutside);
        if sides.len() < 4 {
            return None;
        }
        *next_id += 1;
        Some(Solid { id: *next_id, sides, editor: s.editor.clone(), hidden: false })
    };
    let a = mk(*cut, next_id);
    let b = mk(cut.flipped(), next_id);
    (a, b)
}

/// Möller–Trumbore style ray vs. convex polygon (fan). Returns distance along the ray.
pub fn ray_poly(origin: DVec3, dir: DVec3, poly: &[DVec3]) -> Option<f64> {
    if poly.len() < 3 {
        return None;
    }
    for i in 1..poly.len() - 1 {
        if let Some(t) = ray_tri(origin, dir, poly[0], poly[i], poly[i + 1]) {
            return Some(t);
        }
    }
    None
}

fn ray_tri(o: DVec3, d: DVec3, a: DVec3, b: DVec3, c: DVec3) -> Option<f64> {
    let e1 = b - a;
    let e2 = c - a;
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let tv = o - a;
    let u = tv.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = tv.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    if t > 0.0 {
        Some(t)
    } else {
        None
    }
}

pub fn ray_aabb(o: DVec3, d: DVec3, min: DVec3, max: DVec3) -> Option<f64> {
    let mut tmin = 0.0f64;
    let mut tmax = f64::MAX;
    for i in 0..3 {
        let (oi, di, mn, mx) = (o[i], d[i], min[i], max[i]);
        if di.abs() < 1e-12 {
            if oi < mn || oi > mx {
                return None;
            }
        } else {
            let t1 = (mn - oi) / di;
            let t2 = (mx - oi) / di;
            let (t1, t2) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
            tmin = tmin.max(t1);
            tmax = tmax.min(t2);
            if tmin > tmax {
                return None;
            }
        }
    }
    Some(tmin)
}
