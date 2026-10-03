//! Vertex tool: pick brush corners (or edges), drag them and merge them. Everything here is
//! view-agnostic: callers pass a projection from world space to screen, so the 2D views and the
//! 3D view share the same drawing, hit-testing and commit code.

use super::*;
use crate::editor::geom::{self, SolidGeo};
use crate::formats::vmf::Solid;
use glam::DVec3;

/// Vertices closer than this are the same corner.
const SAME: f64 = 0.01;
/// Screen-space pick radius of a vertex handle, in points.
const PICK_VERTEX: f32 = 7.0;
/// Screen-space pick radius of an edge midpoint handle, in points.
const PICK_EDGE: f32 = 6.0;

const HANDLE: Color32 = Color32::from_rgb(230, 230, 235);
const HANDLE_SEL: Color32 = Color32::YELLOW;
const EDGE_HANDLE: Color32 = Color32::from_rgb(120, 170, 255);
const PREVIEW_OK: Color32 = Color32::from_rgb(80, 220, 255);
const PREVIEW_BAD: Color32 = Color32::from_rgb(255, 60, 60);

/// World -> screen mapping of one viewport (`None` when the point is not visible).
pub(crate) type Project<'a> = &'a dyn Fn(DVec3) -> Option<Pos2>;

/// Selected corners, keyed by the owning solid id and the corner position.
#[derive(Default)]
pub struct VertexState {
    pub sel: Vec<(u32, DVec3)>,
}

impl VertexState {
    fn contains(&self, solid: u32, p: DVec3) -> bool {
        self.sel.iter().any(|(id, v)| *id == solid && (*v - p).length() < SAME)
    }

    fn moves_for(&self, solid: u32, delta: DVec3) -> Vec<(DVec3, DVec3)> {
        self.sel.iter().filter(|(id, _)| *id == solid).map(|(_, p)| (*p, *p + delta)).collect()
    }
}

impl App {
    /// Solids of the selected objects: the brushes whose corners can be edited.
    fn vertex_solids(&self) -> Vec<&Solid> {
        self.sel.iter().flat_map(|id| self.doc.solids_of(*id)).collect()
    }

    /// Where a selected solid ends up when the selected corners move by `delta`.
    fn moved_solid(&self, s: &Solid, delta: DVec3, next_id: &mut u32) -> Option<Solid> {
        let geo = self.doc.geo.get(&s.id)?;
        geom::move_vertices(s, geo, &self.vtx.moves_for(s.id, delta), next_id)
    }

    /// Corners (or the two ends of an edge) under the screen position `pos`.
    pub(crate) fn vertex_hit(&self, project: Project, pos: Pos2) -> Vec<(u32, DVec3)> {
        let mut corners: Vec<(f32, u32, DVec3)> = Vec::new();
        let mut edges: Vec<(f32, [(u32, DVec3); 2])> = Vec::new();
        for s in self.vertex_solids() {
            let Some(geo) = self.doc.geo.get(&s.id) else { continue };
            let verts = geom::solid_vertices(geo);
            for v in &verts {
                if let Some(sp) = project(*v) {
                    corners.push((sp.distance(pos), s.id, *v));
                }
            }
            for (a, b) in geom::solid_edges(geo, &verts) {
                if let Some(sp) = project((verts[a] + verts[b]) * 0.5) {
                    edges.push((sp.distance(pos), [(s.id, verts[a]), (s.id, verts[b])]));
                }
            }
        }
        let nearest = corners.iter().map(|c| c.0).fold(f32::MAX, f32::min);
        if nearest <= PICK_VERTEX {
            // corners stacked on the same screen spot (e.g. top and bottom in an ortho view) go together
            return corners.iter().filter(|c| c.0 <= nearest + 0.5).map(|c| (c.1, c.2)).collect();
        }
        edges
            .iter()
            .filter(|e| e.0 <= PICK_EDGE)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|e| e.1.to_vec())
            .unwrap_or_default()
    }

    /// Select corners. `add` toggles them in the current selection; without it a corner that is
    /// already part of the selection keeps the whole group (so it can be dragged).
    pub(crate) fn vertex_select(&mut self, hit: Vec<(u32, DVec3)>, add: bool) {
        if add {
            for (id, p) in hit {
                if self.vtx.contains(id, p) {
                    self.vtx.sel.retain(|(i, v)| !(*i == id && (*v - p).length() < SAME));
                } else {
                    self.vtx.sel.push((id, p));
                }
            }
        } else if !hit.iter().any(|(id, p)| self.vtx.contains(*id, *p)) {
            self.vtx.sel = hit;
        }
    }

    /// Select every corner whose projection lies inside `area`.
    pub(crate) fn vertex_box_select(&mut self, project: Project, area: Rect, add: bool) {
        let mut found = Vec::new();
        for s in self.vertex_solids() {
            let Some(geo) = self.doc.geo.get(&s.id) else { continue };
            for v in geom::solid_vertices(geo) {
                if project(v).is_some_and(|sp| area.contains(sp)) {
                    found.push((s.id, v));
                }
            }
        }
        if !add {
            self.vtx.sel.clear();
        }
        for (id, p) in found {
            if !self.vtx.contains(id, p) {
                self.vtx.sel.push((id, p));
            }
        }
    }

    /// Snapped movement of the selection while the mouse is at `cur`. Only the axes in `plane`
    /// move; `grab` (the corner under the mouse) is what lands on the grid.
    pub(crate) fn vertex_delta(&self, grab: DVec3, start: DVec3, cur: DVec3, plane: [usize; 2]) -> DVec3 {
        let mut d = DVec3::ZERO;
        for ax in plane {
            d[ax] = self.snapv(grab[ax] + cur[ax] - start[ax]) - grab[ax];
        }
        d
    }

    /// Draw corner handles, and the result of the drag in progress (red when it is not valid).
    pub(crate) fn vertex_draw(&self, painter: &egui::Painter, project: Project, delta: Option<DVec3>) {
        let delta = delta.filter(|d| *d != DVec3::ZERO);
        let line = |a: DVec3, b: DVec3, color: Color32| {
            if let (Some(p), Some(q)) = (project(a), project(b)) {
                painter.line_segment([p, q], Stroke::new(1.5, color));
            }
        };
        let mut scratch_id = self.doc.next_id;
        for s in self.vertex_solids() {
            let Some(geo) = self.doc.geo.get(&s.id) else { continue };
            let verts = geom::solid_vertices(geo);
            let edges = geom::solid_edges(geo, &verts);
            let shown = |v: DVec3| match delta {
                Some(d) if self.vtx.contains(s.id, v) => v + d,
                _ => v,
            };
            if let Some(d) = delta.filter(|_| self.vtx.sel.iter().any(|(id, _)| *id == s.id)) {
                match self.moved_solid(s, d, &mut scratch_id) {
                    Some(n) => {
                        let ng = SolidGeo::build(&n);
                        let nv = geom::solid_vertices(&ng);
                        for (a, b) in geom::solid_edges(&ng, &nv) {
                            line(nv[a], nv[b], PREVIEW_OK);
                        }
                    }
                    None => {
                        for (a, b) in &edges {
                            line(shown(verts[*a]), shown(verts[*b]), PREVIEW_BAD);
                        }
                    }
                }
            } else {
                for (a, b) in &edges {
                    if let Some(sp) = project((verts[*a] + verts[*b]) * 0.5) {
                        painter.circle_filled(sp, 2.5, EDGE_HANDLE);
                    }
                }
            }
            for v in &verts {
                if let Some(sp) = project(shown(*v)) {
                    let color = if self.vtx.contains(s.id, *v) { HANDLE_SEL } else { HANDLE };
                    painter.rect_filled(Rect::from_center_size(sp, Vec2::splat(7.0)), 0.0, color);
                    painter.rect_stroke(Rect::from_center_size(sp, Vec2::splat(7.0)), 0.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Outside);
                }
            }
        }
    }

    /// Apply the selected corners' movement. Nothing changes unless every affected brush stays valid.
    pub(crate) fn vertex_commit(&mut self, delta: DVec3) {
        if delta == DVec3::ZERO {
            return;
        }
        let mut next_id = self.doc.next_id;
        let moved: Option<Vec<Solid>> = self
            .vertex_solids()
            .into_iter()
            .filter(|s| self.vtx.sel.iter().any(|(id, _)| *id == s.id))
            .map(|s| self.moved_solid(s, delta, &mut next_id))
            .collect();
        let Some(moved) = moved else {
            self.status = "Vertex move would make the brush invalid".into();
            return;
        };
        self.doc.checkpoint();
        self.doc.next_id = next_id;
        self.doc.replace_solids(moved);
        for (_, p) in &mut self.vtx.sel {
            *p += delta;
        }
        self.bump_sel();
    }

    /// Collapse the selected corners of each brush onto its first selected corner (Ctrl+F).
    /// Returns false when there is nothing to merge.
    pub(crate) fn vertex_merge(&mut self) -> bool {
        let solids: Vec<u32> = {
            let mut ids: Vec<u32> = self.vtx.sel.iter().map(|(id, _)| *id).collect();
            ids.dedup();
            ids.into_iter().filter(|id| self.vtx.sel.iter().filter(|(i, _)| i == id).count() >= 2).collect()
        };
        if solids.is_empty() {
            return false;
        }
        let mut next_id = self.doc.next_id;
        let mut merged = Vec::new();
        let mut keep = Vec::new();
        for id in solids {
            let Some(orig) = self.vertex_solids().into_iter().find(|s| s.id == id) else { continue };
            let corners: Vec<DVec3> = self.vtx.sel.iter().filter(|(i, _)| *i == id).map(|(_, p)| *p).collect();
            let mut cur = orig.clone();
            for b in &corners[1..] {
                let geo = SolidGeo::build(&cur);
                match geom::merge_vertices(&cur, &geo, corners[0], *b, &mut next_id) {
                    Some(n) => cur = n,
                    None => {
                        self.status = "Those vertices cannot be merged".into();
                        return true;
                    }
                }
            }
            merged.push(cur);
            keep.push((id, corners[0]));
        }
        self.doc.checkpoint();
        self.doc.next_id = next_id;
        self.doc.replace_solids(merged);
        self.vtx.sel = keep;
        self.bump_sel();
        self.status = "Merged vertices".into();
        true
    }
}
