//! The open document: map + undo history + derived geometry + editing operations.

use crate::formats::fgd::Fgd;
use crate::editor::geom::{self, Plane, SolidGeo};
use crate::kv::{Node, NodeList, Value};
use crate::formats::vmf::{fmt, fmt_vec3, parse_vec3, Entity, Map, Solid};
pub use crate::editor::direction::{angles_matrix, matrix_angles};
use glam::{DMat3, DQuat, DVec3};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug)]
pub enum Obj {
    WorldSolid(usize),
    Entity(usize),
}

pub type Sel = BTreeSet<u32>;

#[derive(Clone, Debug)]
pub enum Xform {
    Translate(DVec3),
    Scale { origin: DVec3, factor: DVec3 },
    Rotate { center: DVec3, q: DQuat },
    Mirror { center: DVec3, axis: usize },
}

impl Xform {
    pub fn point(&self, p: DVec3) -> DVec3 {
        match self {
            Xform::Translate(t) => p + *t,
            Xform::Scale { origin, factor } => *origin + (p - *origin) * *factor,
            Xform::Rotate { center, q } => *center + *q * (p - *center),
            Xform::Mirror { center, axis } => {
                let mut d = p - *center;
                d[*axis] = -d[*axis];
                *center + d
            }
        }
    }
    /// Linear part applied to a direction.
    pub fn dir(&self, d: DVec3) -> DVec3 {
        self.point(d) - self.point(DVec3::ZERO)
    }
    pub fn rigid(&self) -> bool {
        match self {
            Xform::Scale { .. } => false,
            _ => true,
        }
    }
    pub fn flips(&self) -> bool {
        match self {
            Xform::Mirror { .. } => true,
            Xform::Scale { factor, .. } => (factor.x * factor.y * factor.z) < 0.0,
            _ => false,
        }
    }
}

pub struct Doc {
    pub map: Map,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    undo: Vec<Map>,
    redo: Vec<Map>,
    pub geo: HashMap<u32, SolidGeo>,
    pub index: HashMap<u32, Obj>,
    pub version: u64,
    pub next_id: u32,
    /// Bounds of the geometry inside func_instance entities (filled in by the app).
    pub inst_bounds: HashMap<u32, (DVec3, DVec3)>,
    /// Model hulls keyed by lowercase model path (filled in by the app as models load).
    pub model_bounds: HashMap<String, (DVec3, DVec3)>,
}

/// The model an entity displays: its `model` key, else the FGD class' `studio("...")`.
pub fn entity_model(e: &Entity, fgd: &Fgd) -> Option<String> {
    let from_key = e.get("model").filter(|m| m.to_ascii_lowercase().ends_with(".mdl"));
    let m = match from_key {
        Some(m) => m.to_string(),
        None => fgd.get(e.classname()).and_then(|c| c.model.clone()).filter(|m| m.to_ascii_lowercase().ends_with(".mdl"))?,
    };
    Some(m.to_ascii_lowercase().replace('\\', "/"))
}

/// The material of the editor sprite an entity is drawn with: the FGD class' `iconsprite("...")`,
/// normalized to a material name (no `materials/` prefix or `.vmt` extension).
pub fn entity_sprite(e: &Entity, fgd: &Fgd) -> Option<String> {
    let s = fgd.get(e.classname())?.sprite.as_deref()?.to_ascii_lowercase().replace('\\', "/");
    let s = s.strip_prefix("materials/").unwrap_or(&s);
    let s = s.strip_suffix(".vmt").unwrap_or(s);
    (!s.is_empty()).then(|| s.to_string())
}

impl Doc {
    pub fn new(map: Map, path: Option<PathBuf>) -> Doc {
        let next_id = map.max_id();
        let mut d = Doc {
            map,
            path,
            dirty: false,
            undo: vec![],
            redo: vec![],
            geo: HashMap::new(),
            index: HashMap::new(),
            version: 0,
            next_id,
            inst_bounds: HashMap::new(),
            model_bounds: HashMap::new(),
        };
        d.touch();
        d
    }

    pub fn alloc_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    /// Rebuild derived data after the map changed.
    pub fn touch(&mut self) {
        self.version += 1;
        self.geo.clear();
        self.index.clear();
        for (i, s) in self.map.world.solids.iter().enumerate() {
            self.geo.insert(s.id, SolidGeo::build(s));
            self.index.insert(s.id, Obj::WorldSolid(i));
        }
        for (i, e) in self.map.entities.iter().enumerate() {
            self.index.insert(e.id, Obj::Entity(i));
            for s in &e.solids {
                self.geo.insert(s.id, SolidGeo::build(s));
            }
        }
        self.next_id = self.next_id.max(self.map.max_id());
    }

    pub fn checkpoint(&mut self) {
        self.undo.push(self.map.clone());
        if self.undo.len() > 64 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    pub fn undo(&mut self) -> bool {
        if let Some(m) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.map, m));
            self.dirty = true;
            self.touch();
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(m) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.map, m));
            self.dirty = true;
            self.touch();
            true
        } else {
            false
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn entity(&self, id: u32) -> Option<&Entity> {
        match self.index.get(&id)? {
            Obj::Entity(i) => self.map.entities.get(*i),
            _ => None,
        }
    }

    pub fn entity_mut(&mut self, id: u32) -> Option<&mut Entity> {
        match self.index.get(&id)? {
            Obj::Entity(i) => self.map.entities.get_mut(*i),
            _ => None,
        }
    }

    /// Solids that make up an object (world solid or brush entity).
    pub fn solids_of(&self, id: u32) -> Vec<&Solid> {
        match self.index.get(&id) {
            Some(Obj::WorldSolid(i)) => vec![&self.map.world.solids[*i]],
            Some(Obj::Entity(i)) => self.map.entities[*i].solids.iter().collect(),
            None => vec![],
        }
    }

    pub fn is_hidden(&self, id: u32) -> bool {
        let ed = match self.index.get(&id) {
            Some(Obj::WorldSolid(i)) => &self.map.world.solids[*i].editor,
            Some(Obj::Entity(i)) => &self.map.entities[*i].editor,
            None => return false,
        };
        ed.get_str("visgroupshown") == Some("0") || ed.get_str("visgroupautoshown") == Some("0")
    }

    /// Ids of all objects (world solids + entities), in map order.
    pub fn all_ids(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.map.world.solids.iter().map(|s| s.id).collect();
        v.extend(self.map.entities.iter().map(|e| e.id));
        v
    }

    pub fn ent_bounds(&self, e: &Entity, fgd: &Fgd) -> (DVec3, DVec3) {
        if !e.solids.is_empty() {
            let mut min = DVec3::splat(f64::MAX);
            let mut max = DVec3::splat(f64::MIN);
            for s in &e.solids {
                if let Some(g) = self.geo.get(&s.id) {
                    if g.valid {
                        min = min.min(g.min);
                        max = max.max(g.max);
                    }
                }
            }
            if min.x <= max.x {
                return (min, max);
            }
        }
        if let Some(b) = self.inst_bounds.get(&e.id) {
            return *b;
        }
        if let Some(h) = entity_model(e, fgd).and_then(|m| self.model_bounds.get(&m)) {
            let m = angles_matrix(e.angles());
            let o = e.origin();
            let mut mn = DVec3::splat(f64::MAX);
            let mut mx = DVec3::splat(f64::MIN);
            for i in 0..8 {
                let c = DVec3::new(
                    if i & 1 != 0 { h.1.x } else { h.0.x },
                    if i & 2 != 0 { h.1.y } else { h.0.y },
                    if i & 4 != 0 { h.1.z } else { h.0.z },
                );
                let p = o + m * c;
                mn = mn.min(p);
                mx = mx.max(p);
            }
            return (mn, mx);
        }
        let o = e.origin();
        let (mn, mx) = fgd
            .get(e.classname())
            .and_then(|c| c.size)
            .unwrap_or((DVec3::splat(-8.0), DVec3::splat(8.0)));
        // rotate the box by angles so that rotated entities show correctly in 2D
        (o + mn, o + mx)
    }

    pub fn bounds(&self, id: u32, fgd: &Fgd) -> Option<(DVec3, DVec3)> {
        match self.index.get(&id)? {
            Obj::WorldSolid(i) => {
                let g = self.geo.get(&self.map.world.solids[*i].id)?;
                Some((g.min, g.max))
            }
            Obj::Entity(i) => Some(self.ent_bounds(&self.map.entities[*i], fgd)),
        }
    }

    pub fn sel_bounds(&self, sel: &Sel, fgd: &Fgd) -> Option<(DVec3, DVec3)> {
        let mut min = DVec3::splat(f64::MAX);
        let mut max = DVec3::splat(f64::MIN);
        let mut any = false;
        for id in sel {
            if let Some((a, b)) = self.bounds(*id, fgd) {
                min = min.min(a);
                max = max.max(b);
                any = true;
            }
        }
        any.then_some((min, max))
    }

    // ---- operations ------------------------------------------------------------------------

    /// Swap in edited versions of existing solids (matched by id).
    pub fn replace_solids(&mut self, new: Vec<Solid>) {
        for n in new {
            let slot = self
                .map
                .world
                .solids
                .iter_mut()
                .chain(self.map.entities.iter_mut().flat_map(|e| e.solids.iter_mut()))
                .find(|s| s.id == n.id);
            if let Some(s) = slot {
                *s = n;
            }
        }
        self.touch();
    }

    pub fn transform(&mut self, sel: &Sel, xf: &Xform, texture_lock: bool) {
        let flip = xf.flips();
        let f = |p: DVec3| xf.point(p);
        let rot = |d: DVec3| xf.dir(d);
        let lock = texture_lock && xf.rigid();
        let apply_solid = |s: &mut Solid| {
            geom::transform_solid(s, &f, if lock { Some(&rot) } else { None }, flip);
        };
        for s in &mut self.map.world.solids {
            if sel.contains(&s.id) {
                apply_solid(s);
            }
        }
        for e in &mut self.map.entities {
            if !sel.contains(&e.id) {
                continue;
            }
            for s in &mut e.solids {
                apply_solid(s);
            }
            if e.get("origin").is_some() {
                let o = xf.point(e.origin());
                e.set_origin(geom::snap_near(o));
            }
            for key in ["angles", "movedir"] {
                // point entities always carry angles; brush entities only when they have the key
                if e.get(key).is_none() && !(key == "angles" && e.get("origin").is_some()) {
                    continue;
                }
                let Some(cur) = e.get(key).map_or(Some(DVec3::ZERO), parse_vec3) else { continue };
                let new = match xf {
                    Xform::Rotate { q, .. } => matrix_angles(DMat3::from_quat(*q) * angles_matrix(cur)),
                    Xform::Mirror { axis, .. } => {
                        // mirror the yaw only (good enough for entities)
                        let mut a = cur;
                        match axis {
                            0 => a.y = 180.0 - a.y,
                            1 => a.y = -a.y,
                            _ => a.x = -a.x,
                        }
                        a
                    }
                    _ => continue,
                };
                e.set(key, fmt_vec3(new));
            }
        }
        self.touch();
    }

    /// Sets the `origin` key of one entity (creating it for brush entities) without moving its brushes.
    pub fn set_entity_origin(&mut self, id: u32, origin: DVec3) {
        if let Some(e) = self.map.entities.iter_mut().find(|e| e.id == id) {
            e.set_origin(origin);
            self.touch();
        }
    }

    pub fn delete(&mut self, sel: &Sel) {
        self.map.world.solids.retain(|s| !sel.contains(&s.id));
        self.map.entities.retain(|e| !sel.contains(&e.id));
        self.touch();
    }

    fn renumber_solid(&mut self, s: &mut Solid) {
        s.id = self.alloc_id();
        for sd in &mut s.sides {
            sd.id = self.alloc_id();
        }
    }

    /// Copy objects with new ids. Returns the new object ids.
    pub fn clone_objects(&mut self, sel: &Sel, offset: DVec3) -> Sel {
        let mut new = Sel::new();
        let mut add_solids: Vec<Solid> = vec![];
        let mut add_ents: Vec<Entity> = vec![];
        for s in &self.map.world.solids {
            if sel.contains(&s.id) {
                add_solids.push(s.clone());
            }
        }
        for e in &self.map.entities {
            if sel.contains(&e.id) {
                add_ents.push(e.clone());
            }
        }
        for mut s in add_solids {
            self.renumber_solid(&mut s);
            geom::translate_solid(&mut s, offset);
            new.insert(s.id);
            self.map.world.solids.push(s);
        }
        for mut e in add_ents {
            e.id = self.alloc_id();
            let mut solids = std::mem::take(&mut e.solids);
            for s in &mut solids {
                self.renumber_solid(s);
                geom::translate_solid(s, offset);
            }
            e.solids = solids;
            if e.get("origin").is_some() {
                let o = e.origin() + offset;
                e.set_origin(o);
            }
            new.insert(e.id);
            self.map.entities.push(e);
        }
        self.touch();
        new
    }

    /// Extract objects for the clipboard.
    pub fn copy_objects(&self, sel: &Sel) -> Clipboard {
        Clipboard {
            solids: self.map.world.solids.iter().filter(|s| sel.contains(&s.id)).cloned().collect(),
            ents: self.map.entities.iter().filter(|e| sel.contains(&e.id)).cloned().collect(),
        }
    }

    pub fn paste(&mut self, cb: &Clipboard, offset: DVec3) -> Sel {
        let mut new = Sel::new();
        for s in &cb.solids {
            let mut s = s.clone();
            self.renumber_solid(&mut s);
            geom::translate_solid(&mut s, offset);
            new.insert(s.id);
            self.map.world.solids.push(s);
        }
        for e in &cb.ents {
            let mut e = e.clone();
            e.id = self.alloc_id();
            let mut solids = std::mem::take(&mut e.solids);
            for s in &mut solids {
                self.renumber_solid(s);
                geom::translate_solid(s, offset);
            }
            e.solids = solids;
            if e.get("origin").is_some() {
                let o = e.origin() + offset;
                e.set_origin(o);
            }
            new.insert(e.id);
            self.map.entities.push(e);
        }
        self.touch();
        new
    }

    pub fn create_solid(&mut self, planes: &[Plane], material: &str, tex_scale: f64, lightmap: i32) -> Option<u32> {
        let mut id = self.next_id;
        let s = geom::solid_from_planes(planes, material, &mut id, tex_scale, lightmap)?;
        self.next_id = id;
        let sid = s.id;
        self.map.world.solids.push(s);
        self.touch();
        Some(sid)
    }

    pub fn create_entity(&mut self, class: &str, origin: DVec3, fgd: &Fgd) -> u32 {
        let id = self.alloc_id();
        let mut e = Entity { id, ..Default::default() };
        e.set("classname", class);
        if let Some(c) = fgd.get(class) {
            for p in &c.props {
                if !p.default.is_empty()
                    && !p.name.eq_ignore_ascii_case("classname")
                    && !p.name.eq_ignore_ascii_case("origin")
                    && !p.ty.eq_ignore_ascii_case("void")
                    && p.name != "targetname"
                    && p.ty != "target_destination"
                    && p.ty != "target_source"
                    && !p.name.eq_ignore_ascii_case("angles")
                {
                    e.set(&p.name, p.default.clone());
                }
            }
        }
        e.set("angles", "0 0 0");
        e.set_origin(origin);
        let color = fgd.get(class).and_then(|c| c.color).unwrap_or([220, 30, 220]);
        e.editor = vec![
            Node::str("color", format!("{} {} {}", color[0], color[1], color[2])),
            Node::str("visgroupshown", "1"),
            Node::str("visgroupautoshown", "1"),
        ];
        self.map.entities.push(e);
        self.touch();
        id
    }

    /// Turn the selected world solids into one brush entity.
    pub fn tie_to_entity(&mut self, sel: &Sel, class: &str, fgd: &Fgd) -> Option<u32> {
        let mut solids = vec![];
        self.map.world.solids.retain(|s| {
            if sel.contains(&s.id) {
                solids.push(s.clone());
                false
            } else {
                true
            }
        });
        if solids.is_empty() {
            return None;
        }
        let id = self.alloc_id();
        let mut e = Entity { id, solids, ..Default::default() };
        e.set("classname", class);
        if let Some(c) = fgd.get(class) {
            for p in &c.props {
                if !p.default.is_empty() && p.name != "targetname" && p.ty != "target_destination" && p.ty != "void" {
                    e.set(&p.name, p.default.clone());
                }
            }
        }
        let color = fgd.get(class).and_then(|c| c.color).unwrap_or([0, 100, 250]);
        e.editor = vec![
            Node::str("color", format!("{} {} {}", color[0], color[1], color[2])),
            Node::str("visgroupshown", "1"),
            Node::str("visgroupautoshown", "1"),
        ];
        // origin defaults to the centre of the brushes
        let (mut min, mut max) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN));
        for g in e.solids.iter().filter_map(|s| self.geo.get(&s.id)).filter(|g| g.valid) {
            min = min.min(g.min);
            max = max.max(g.max);
        }
        if min.x <= max.x {
            e.set_origin(geom::snap_near((min + max) * 0.5));
        }
        self.map.entities.push(e);
        self.touch();
        Some(id)
    }

    /// Move solids from brush entities back to the world.
    pub fn move_to_world(&mut self, sel: &Sel) -> Sel {
        let mut out = Sel::new();
        let mut keep = vec![];
        for e in std::mem::take(&mut self.map.entities) {
            if sel.contains(&e.id) && !e.solids.is_empty() {
                for s in e.solids {
                    out.insert(s.id);
                    self.map.world.solids.push(s);
                }
            } else {
                keep.push(e);
            }
        }
        self.map.entities = keep;
        self.touch();
        out
    }

    pub fn hollow(&mut self, sel: &Sel, thickness: f64, tex_scale: f64, lightmap: i32) -> Sel {
        let mut out = Sel::new();
        let ids: Vec<u32> = self.map.world.solids.iter().filter(|s| sel.contains(&s.id)).map(|s| s.id).collect();
        for id in ids {
            let Some(pos) = self.map.world.solids.iter().position(|s| s.id == id) else { continue };
            let src = self.map.world.solids.remove(pos);
            let outer = geom::solid_planes(&src);
            let polys = geom::polys_from_planes(&outer);
            let live: Vec<usize> = (0..outer.len()).filter(|i| !polys[*i].is_empty()).collect();
            let inner: Vec<Plane> = outer.iter().map(|p| Plane { n: p.n, d: p.d - thickness }).collect();
            let mut made = false;
            for (k, &i) in live.iter().enumerate() {
                let mut planes: Vec<Plane> = live.iter().map(|&j| outer[j]).collect();
                planes.push(inner[i].flipped());
                for &j in &live[..k] {
                    planes.push(inner[j]);
                }
                let mut nid = self.next_id;
                if let Some(mut s) = geom::solid_from_planes(&planes, &src.sides[i].material, &mut nid, tex_scale, lightmap) {
                    // keep original texturing on the outer face
                    s.editor = src.editor.clone();
                    self.next_id = nid;
                    out.insert(s.id);
                    self.map.world.solids.push(s);
                    made = true;
                }
            }
            if !made {
                self.map.world.solids.push(src);
            }
        }
        self.touch();
        out
    }

    pub fn clip(&mut self, sel: &Sel, plane: &Plane, keep_front: bool, keep_back: bool, material: &str) -> Sel {
        let mut out = Sel::new();
        let mut next = self.next_id;
        let mut new_world = vec![];
        for s in std::mem::take(&mut self.map.world.solids) {
            if !sel.contains(&s.id) {
                new_world.push(s);
                continue;
            }
            let (a, b) = geom::clip_solid(&s, plane, material, &mut next);
            let mut pieces = vec![];
            if keep_front {
                pieces.extend(a);
            }
            if keep_back {
                pieces.extend(b);
            }
            if pieces.is_empty() && !(keep_front && keep_back) {
                // clip plane misses the brush entirely: keep it untouched
                let g = SolidGeo::build(&s);
                let on_kept_side = g.polys.iter().flatten().all(|v| {
                    let d = plane.dist(*v);
                    (keep_back && d <= geom::EPS) || (keep_front && d >= -geom::EPS)
                });
                if on_kept_side {
                    out.insert(s.id);
                    new_world.push(s);
                }
                continue;
            }
            for p in pieces {
                out.insert(p.id);
                new_world.push(p);
            }
        }
        self.map.world.solids = new_world;
        self.next_id = next;
        self.touch();
        out
    }

    pub fn set_prop(&mut self, ids: &Sel, key: &str, val: &str) {
        for e in &mut self.map.entities {
            if ids.contains(&e.id) {
                if val.is_empty() && !key.eq_ignore_ascii_case("classname") {
                    e.remove(key);
                } else {
                    e.set(key, val);
                }
            }
        }
        // worldspawn is addressed by its own id
        if ids.contains(&self.map.world.id) {
            if val.is_empty() {
                self.map.world.remove(key);
            } else {
                self.map.world.set(key, val);
            }
        }
    }

    pub fn targetnames(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .map
            .entities
            .iter()
            .filter_map(|e| e.get("targetname"))
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        v.sort();
        v.dedup();
        v
    }

    // ---- visgroups -----------------------------------------------------------------------

    pub fn visgroups(&self) -> Vec<VisGroup> {
        let mut out = vec![];
        fn walk(nodes: &[Node], depth: usize, out: &mut Vec<VisGroup>) {
            for n in nodes {
                if n.key.eq_ignore_ascii_case("visgroup") {
                    let c = n.children();
                    out.push(VisGroup {
                        name: c.get_str("name").unwrap_or("").to_string(),
                        id: c.get_str("visgroupid").and_then(|v| v.parse().ok()).unwrap_or(0),
                        depth,
                    });
                    walk(c, depth + 1, out);
                }
            }
        }
        if let Some(v) = self.map.header.get_block("visgroups") {
            walk(v, 0, &mut out);
        }
        out
    }

    fn in_visgroup(editor: &[Node], id: u32) -> bool {
        editor.iter().any(|n| n.key.eq_ignore_ascii_case("visgroupid") && n.as_str().and_then(|s| s.parse::<u32>().ok()) == Some(id))
    }

    pub fn visgroup_members(&self, id: u32) -> Vec<u32> {
        let mut v = vec![];
        for s in &self.map.world.solids {
            if Self::in_visgroup(&s.editor, id) {
                v.push(s.id);
            }
        }
        for e in &self.map.entities {
            if Self::in_visgroup(&e.editor, id) {
                v.push(e.id);
            }
        }
        v
    }

    pub fn set_editor(editor: &mut Vec<Node>, key: &str, val: &str) {
        if let Some(n) = editor.iter_mut().find(|n| n.key.eq_ignore_ascii_case(key)) {
            n.value = Value::Str(val.to_string());
        } else {
            editor.push(Node::str(key, val));
        }
    }

    pub fn set_visgroup_shown(&mut self, id: u32, shown: bool) {
        let v = if shown { "1" } else { "0" };
        for s in &mut self.map.world.solids {
            if Self::in_visgroup(&s.editor, id) {
                Self::set_editor(&mut s.editor, "visgroupshown", v);
            }
        }
        for e in &mut self.map.entities {
            if Self::in_visgroup(&e.editor, id) {
                Self::set_editor(&mut e.editor, "visgroupshown", v);
            }
        }
        self.touch();
    }

    pub fn visgroup_shown(&self, id: u32) -> bool {
        let m = self.visgroup_members(id);
        m.is_empty() || m.iter().any(|i| !self.is_hidden(*i))
    }

    pub fn add_visgroup(&mut self, name: &str) -> u32 {
        let mut maxid = 0;
        for g in self.visgroups() {
            maxid = maxid.max(g.id);
        }
        let id = maxid + 1;
        let node = Node::block(
            "visgroup",
            vec![
                Node::str("name", name),
                Node::str("visgroupid", id.to_string()),
                Node::str("color", "0 192 192"),
            ],
        );
        if let Some(h) = self.map.header.iter_mut().find(|n| n.key.eq_ignore_ascii_case("visgroups")) {
            if let Value::Block(c) = &mut h.value {
                c.push(node);
            }
        } else {
            self.map.header.push(Node::block("visgroups", vec![node]));
        }
        id
    }

    /// Remove a visgroup (and its child groups); members are un-grouped and made visible again.
    pub fn delete_visgroup(&mut self, id: u32) {
        fn collect(nodes: &[Node], id: u32, inside: bool, out: &mut Vec<u32>) -> bool {
            let mut found = false;
            for n in nodes {
                if !n.key.eq_ignore_ascii_case("visgroup") {
                    continue;
                }
                let c = n.children();
                let nid = c.get_str("visgroupid").and_then(|v| v.parse().ok()).unwrap_or(0);
                let hit = inside || nid == id;
                if hit {
                    out.push(nid);
                    found = true;
                }
                found |= collect(c, id, hit, out);
            }
            found
        }
        fn prune(nodes: &mut Vec<Node>, ids: &[u32]) {
            nodes.retain(|n| {
                !(n.key.eq_ignore_ascii_case("visgroup")
                    && n.children().get_str("visgroupid").and_then(|v| v.parse::<u32>().ok()).map_or(false, |i| ids.contains(&i)))
            });
            for n in nodes.iter_mut() {
                if let Value::Block(c) = &mut n.value {
                    prune(c, ids);
                }
            }
        }
        let mut ids = vec![];
        if let Some(v) = self.map.header.get_block("visgroups") {
            collect(v, id, false, &mut ids);
        }
        if ids.is_empty() {
            return;
        }
        let strip = |editor: &mut Vec<Node>| {
            let had = editor.iter().any(|n| {
                n.key.eq_ignore_ascii_case("visgroupid") && n.as_str().and_then(|s| s.parse::<u32>().ok()).map_or(false, |i| ids.contains(&i))
            });
            if had {
                editor.retain(|n| {
                    !(n.key.eq_ignore_ascii_case("visgroupid") && n.as_str().and_then(|s| s.parse::<u32>().ok()).map_or(false, |i| ids.contains(&i)))
                });
                Self::set_editor(editor, "visgroupshown", "1");
            }
        };
        for s in &mut self.map.world.solids {
            strip(&mut s.editor);
        }
        for e in &mut self.map.entities {
            strip(&mut e.editor);
        }
        if let Some(h) = self.map.header.iter_mut().find(|n| n.key.eq_ignore_ascii_case("visgroups")) {
            if let Value::Block(c) = &mut h.value {
                prune(c, &ids);
            }
        }
        self.touch();
    }

    pub fn assign_visgroup(&mut self, sel: &Sel, id: u32, add: bool) {
        let ed = |editor: &mut Vec<Node>| {
            editor.retain(|n| !(n.key.eq_ignore_ascii_case("visgroupid") && n.as_str().and_then(|s| s.parse::<u32>().ok()) == Some(id)));
            if add {
                // visgroupid entries go before the other editor keys in Hammer's output
                editor.insert(0, Node::str("visgroupid", id.to_string()));
            }
        };
        for s in &mut self.map.world.solids {
            if sel.contains(&s.id) {
                ed(&mut s.editor);
            }
        }
        for e in &mut self.map.entities {
            if sel.contains(&e.id) {
                ed(&mut e.editor);
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct VisGroup {
    pub name: String,
    pub id: u32,
    pub depth: usize,
}

#[derive(Clone, Default)]
pub struct Clipboard {
    pub solids: Vec<Solid>,
    pub ents: Vec<Entity>,
}

impl Clipboard {
    pub fn is_empty(&self) -> bool {
        self.solids.is_empty() && self.ents.is_empty()
    }
}

#[allow(unused)]
fn _use(_: &str) -> String {
    fmt(1.0)
}
