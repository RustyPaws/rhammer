//! Model viewer window: browse game models in a folder tree, orbit them textured, play
//! sequences, show bones and attachments. Doubles as the model picker for `studio` keys.

use crate::app::App;
use crate::editor::doc::Sel;
use crate::assets::mdl::{self, Model};
use crate::render3d::{self, Batch, Vertex};
use eframe::egui::{self, Color32, Pos2, RichText, Sense, Stroke};
use glam::{DMat4, Mat4, Vec3, Vec4};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

/// A folder of the model tree; `files` index into `ModelViewer::list`.
#[derive(Default)]
pub struct Dir {
    dirs: BTreeMap<String, Dir>,
    files: Vec<usize>,
}

impl Dir {
    fn build(list: &[String]) -> Dir {
        let mut root = Dir::default();
        for (i, p) in list.iter().enumerate() {
            let mut parts: Vec<&str> = p.trim_start_matches("models/").split('/').collect();
            parts.pop();
            let mut d = &mut root;
            for part in parts {
                d = d.dirs.entry(part.to_string()).or_default();
            }
            d.files.push(i);
        }
        root
    }
}

/// Where a picked model goes: the entities and the key to set.
pub struct PickTarget {
    pub ids: Sel,
    pub key: String,
}

pub struct ModelViewer {
    pub list: Vec<String>,
    tree: Option<Dir>,
    pub filter: String,
    pub path: String,
    pub model: Option<Rc<Model>>,
    pub skin: usize,
    pub seq: usize,
    pub seq_filter: String,
    pub time: f64,
    pub playing: bool,
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    /// Camera target offset from the model center (panning).
    pub pan: Vec3,
    pub show_mesh: bool,
    pub show_bones: bool,
    pub show_attach: bool,
    pub show_hull: bool,
    pub load_failed: bool,
    /// The model's files were still arriving (browser): load again next frame.
    pub retry: bool,
    /// Set when opened from an entity's model key: the window becomes a picker.
    pub pick: Option<PickTarget>,
    /// Expand the tree down to the current model on the next frame.
    reveal: bool,
    mesh_key: (String, usize, usize, u64, usize),
}

impl Default for ModelViewer {
    fn default() -> Self {
        ModelViewer {
            list: vec![],
            tree: None,
            filter: String::new(),
            path: String::new(),
            model: None,
            skin: 0,
            seq: 0,
            seq_filter: String::new(),
            time: 0.0,
            playing: true,
            yaw: 0.6,
            pitch: 0.3,
            zoom: 1.0,
            pan: Vec3::ZERO,
            show_mesh: true,
            show_bones: false,
            show_attach: false,
            show_hull: false,
            load_failed: false,
            retry: false,
            pick: None,
            reveal: false,
            mesh_key: Default::default(),
        }
    }
}

struct Cam {
    vp: Mat4,
    rect: egui::Rect,
}

impl Cam {
    fn project(&self, p: Vec3) -> Option<Pos2> {
        let c = self.vp * Vec4::new(p.x, p.y, p.z, 1.0);
        if c.w <= 0.01 {
            return None;
        }
        let (x, y) = (c.x / c.w, c.y / c.w);
        let s = self.rect.size();
        Some(Pos2::new(self.rect.min.x + (x * 0.5 + 0.5) * s.x, self.rect.min.y + (0.5 - y * 0.5) * s.y))
    }
}

fn v3(p: glam::DVec3) -> Vec3 {
    Vec3::new(p.x as f32, p.y as f32, p.z as f32)
}

/// Draw one folder level. `reveal` (path relative to models/) forces its folders open.
fn tree_ui(ui: &mut egui::Ui, dir: &Dir, prefix: &str, list: &[String], cur: &str, reveal: Option<&str>, pick: &mut Option<(String, bool)>) {
    for (name, sub) in &dir.dirs {
        let full = format!("{prefix}{name}/");
        let open = reveal.filter(|r| r.starts_with(&full)).map(|_| true);
        egui::CollapsingHeader::new(name.as_str()).id_salt(("mv_dir", &full)).open(open).show(ui, |ui| {
            tree_ui(ui, sub, &full, list, cur, reveal, pick);
        });
    }
    for &i in &dir.files {
        let p = &list[i];
        let short = p.rsplit('/').next().unwrap_or(p);
        let r = ui.selectable_label(p == cur, short);
        if reveal.is_some() && p == cur {
            r.scroll_to_me(Some(egui::Align::Center));
        }
        if r.clicked() || r.double_clicked() {
            *pick = Some((p.clone(), r.double_clicked()));
        }
    }
}

impl App {
    /// Open the model window as a picker for `key` on `ids`, starting at `current`.
    pub fn pick_model(&mut self, ids: Sel, key: &str, current: &str) {
        let cur = current.to_ascii_lowercase().replace('\\', "/");
        let mut mv = std::mem::take(&mut self.mv);
        if cur.ends_with(".mdl") && cur != mv.path {
            self.mv_load(&mut mv, cur);
        }
        mv.pick = Some(PickTarget { ids, key: key.to_string() });
        mv.reveal = true;
        mv.filter.clear();
        self.mv = mv;
        self.win.model_viewer = true;
    }

    pub fn dlg_model_viewer(&mut self, ctx: &egui::Context) {
        if !self.win.model_viewer {
            return;
        }
        let mut mv = std::mem::take(&mut self.mv);
        if mv.retry {
            let path = mv.path.clone();
            self.mv_load(&mut mv, path);
        }
        if mv.list.is_empty() {
            mv.list = self.mats.fs.list("models/", "mdl");
            mv.tree = None;
        }
        if mv.tree.is_none() {
            mv.tree = Some(Dir::build(&mv.list));
        }
        let mut open = true;
        let mut accept = false;
        let mut cancel = false;
        let title = if mv.pick.is_some() { "Select model" } else { "Model viewer" };
        egui::Window::new(title).id(egui::Id::new("model_viewer")).open(&mut open).default_size([920.0, 620.0]).show(ctx, |ui| {
            if mv.pick.is_some() {
                egui::Panel::bottom("mv_pick_bar").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(if mv.path.is_empty() { "(no model)" } else { mv.path.as_str() }).monospace());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Cancel").clicked() {
                                cancel = true;
                            }
                            if ui.add_enabled(!mv.path.is_empty() && !mv.load_failed, egui::Button::new(RichText::new("OK").strong())).clicked() {
                                accept = true;
                            }
                        });
                    });
                });
            }
            egui::Panel::left("mv_side").resizable(true).default_size(280.0).size_range(180.0..=600.0).show(ui, |ui| {
                if self.mv_list(ui, &mut mv) && mv.pick.is_some() && !mv.load_failed {
                    accept = true;
                }
            });
            egui::CentralPanel::default().show(ui, |ui| self.mv_view(ui, &mut mv));
        });
        if accept {
            if let Some(t) = mv.pick.take() {
                self.doc.checkpoint();
                self.doc.set_prop(&t.ids, &t.key, &mv.path);
                self.doc.touch();
                self.status = format!("{} = {}", t.key, mv.path);
            }
            open = false;
        }
        if !open || cancel {
            self.win.model_viewer = false;
            mv.pick = None;
        }
        self.mv = mv;
    }

    /// Model list (folder tree, or flat while filtering). Returns true on a double-click.
    fn mv_list(&mut self, ui: &mut egui::Ui, mv: &mut ModelViewer) -> bool {
        ui.add(egui::TextEdit::singleline(&mut mv.filter).hint_text("filter models…").desired_width(f32::INFINITY));
        let f = mv.filter.to_ascii_lowercase();
        let mut pick: Option<(String, bool)> = None;
        if f.is_empty() {
            ui.label(RichText::new(format!("{} models", mv.list.len())).small().weak());
            let reveal = if std::mem::take(&mut mv.reveal) { Some(mv.path.trim_start_matches("models/").to_string()) } else { None };
            egui::ScrollArea::both().id_salt("mv_tree").auto_shrink([false, false]).show(ui, |ui| {
                if let Some(tree) = &mv.tree {
                    tree_ui(ui, tree, "", &mv.list, &mv.path, reveal.as_deref(), &mut pick);
                }
            });
        } else {
            let shown: Vec<usize> = (0..mv.list.len()).filter(|&i| mv.list[i].contains(&f)).collect();
            ui.label(RichText::new(format!("{} / {} models", shown.len(), mv.list.len())).small().weak());
            egui::ScrollArea::both().id_salt("mv_list").auto_shrink([false, false]).show_rows(ui, ui.text_style_height(&egui::TextStyle::Body), shown.len(), |ui, range| {
                for k in range {
                    let p = &mv.list[shown[k]];
                    let r = ui.selectable_label(*p == mv.path, p.trim_start_matches("models/"));
                    if r.clicked() || r.double_clicked() {
                        pick = Some((p.clone(), r.double_clicked()));
                    }
                }
            });
        }
        let Some((p, dbl)) = pick else { return false };
        if p != mv.path {
            self.mv_load(mv, p);
        }
        dbl
    }

    pub fn mv_load(&mut self, mv: &mut ModelViewer, path: String) {
        let mark = self.mats.fs.mark();
        let m = mdl::load(&self.mats.fs, &path).map(Rc::new);
        mv.retry = m.is_none() && self.mats.fs.stalled_since(mark);
        if mv.retry {
            self.ctx.request_repaint();
        }
        mv.load_failed = m.is_none() && !mv.retry;
        mv.model = m;
        mv.path = path;
        mv.skin = 0;
        mv.seq = 0;
        mv.time = 0.0;
        mv.zoom = 1.0;
        mv.pan = Vec3::ZERO;
        mv.mesh_key = Default::default();
    }

    /// Rebuild the textured preview mesh when the model, skin, frame or loaded textures changed.
    fn mv_mesh(&mut self, mv: &mut ModelViewer, model: &Model, frame: f64) {
        let key = (mv.path.clone(), mv.skin, mv.seq, frame.to_bits(), self.mats.info.len());
        if key == mv.mesh_key {
            return;
        }
        let posed;
        let lists: Vec<&[mdl::ModelVert]> = if model.sequences.is_empty() || (mv.seq == 0 && frame == 0.0) {
            model.parts.iter().map(|p| &p.verts[..]).collect()
        } else {
            posed = model.posed(mv.seq, frame);
            posed.iter().map(|v| &v[..]).collect()
        };
        let mut batches: HashMap<String, Vec<Vertex>> = HashMap::new();
        for (part, verts) in model.parts.iter().zip(lists) {
            let mat = part.materials.get(mv.skin).or(part.materials.first()).cloned().unwrap_or_default();
            let (col, _) = self.material_color(&mat);
            let out = batches.entry(mat.to_ascii_lowercase()).or_default();
            let conv = |v: &mdl::ModelVert| Vertex { pos: v.pos, nrm: v.nrm, uv: v.uv, col };
            // Source triangles are clockwise; flip to CCW for culling
            for tri in verts.chunks_exact(3) {
                out.push(conv(&tri[0]));
                out.push(conv(&tri[2]));
                out.push(conv(&tri[1]));
            }
        }
        if self.mats.starved {
            // textures are still being decoded: rebuild next frame
            self.ctx.request_repaint();
        } else {
            mv.mesh_key = key;
        }
        if let Ok(mut sh) = self.shared.lock() {
            sh.preview.uploads.append(&mut self.mats.ready);
            sh.preview.model_batches = batches.into_iter().map(|(material, verts)| Batch { material, verts }).collect();
            sh.preview.models_version += 1;
        }
    }

    fn mv_view(&mut self, ui: &mut egui::Ui, mv: &mut ModelViewer) {
        let Some(model) = mv.model.clone() else {
            ui.label(if mv.load_failed { "Failed to load model." } else if mv.retry { "Loading..." } else { "Pick a model from the list." });
            return;
        };
        ui.label(RichText::new(&mv.path).strong());
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut mv.show_mesh, "Mesh");
            ui.checkbox(&mut mv.show_bones, "Bones");
            ui.checkbox(&mut mv.show_attach, "Attachments");
            ui.checkbox(&mut mv.show_hull, "Hull");
            let skins = model.parts.iter().map(|p| p.materials.len()).max().unwrap_or(1).max(1);
            if skins > 1 {
                mv.skin = mv.skin.min(skins - 1);
                ui.separator();
                ui.label("Skin");
                ui.add(egui::DragValue::new(&mut mv.skin).range(0..=skins - 1));
            }
            if ui.button("Reset view").clicked() {
                mv.yaw = 0.6;
                mv.pitch = 0.3;
                mv.zoom = 1.0;
                mv.pan = Vec3::ZERO;
            }
        });

        // animation controls
        if !model.sequences.is_empty() {
            mv.seq = mv.seq.min(model.sequences.len() - 1);
            let seq = &model.sequences[mv.seq];
            ui.horizontal(|ui| {
                ui.label("Sequence");
                egui::ComboBox::from_id_salt("mv_seq").selected_text(format!("{}  ({}f)", seq.name, seq.frames)).width(300.0).height(400.0).show_ui(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut mv.seq_filter).hint_text("filter…"));
                    let fl = mv.seq_filter.to_ascii_lowercase();
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for (i, s) in model.sequences.iter().enumerate() {
                            if !fl.is_empty() && !s.name.to_ascii_lowercase().contains(&fl) && !s.activity.to_ascii_lowercase().contains(&fl) {
                                continue;
                            }
                            if ui.selectable_label(i == mv.seq, format!("{}  ({}f)", s.name, s.frames)).clicked() {
                                mv.seq = i;
                                mv.time = 0.0;
                            }
                        }
                    });
                });
            });
            let seq = &model.sequences[mv.seq];
            let dur = seq.duration();
            ui.horizontal(|ui| {
                if ui.button(if mv.playing { "⏸" } else { "▶" }).clicked() {
                    mv.playing = !mv.playing;
                }
                if ui.button("⏮").clicked() {
                    mv.time = 0.0;
                }
                if dur > 0.0 {
                    if mv.playing {
                        mv.time += ui.input(|i| i.stable_dt) as f64;
                        ui.ctx().request_repaint();
                    }
                    let mut t = mv.time.rem_euclid(dur);
                    if ui.add(egui::Slider::new(&mut t, 0.0..=dur).show_value(false)).changed() {
                        mv.time = t;
                        mv.playing = false;
                    }
                    ui.label(format!("{:.0}/{}", seq.frame_at(mv.time), seq.frames - 1));
                }
                let mut info = format!("{} fps", seq.fps);
                if seq.looping {
                    info.push_str(", looping");
                }
                if !seq.activity.is_empty() {
                    info.push_str(&format!(", {}", seq.activity));
                }
                ui.weak(info);
            });
        } else {
            ui.weak("No animations");
        }

        // 3D view
        let avail = ui.available_size();
        let h = (avail.y - 50.0).max(240.0);
        let (resp, painter) = ui.allocate_painter(egui::vec2(avail.x.max(300.0), h), Sense::click_and_drag());
        if resp.dragged_by(egui::PointerButton::Primary) {
            let d = resp.drag_delta();
            mv.yaw -= d.x * 0.01;
            mv.pitch = (mv.pitch + d.y * 0.01).clamp(-1.5, 1.5);
        }
        if resp.hovered() {
            let sc = ui.input(|i| i.smooth_scroll_delta.y);
            mv.zoom = (mv.zoom * (1.0 - sc * 0.002)).clamp(0.12, 20.0);
        }
        let painter = painter.with_clip_rect(resp.rect.intersect(ui.clip_rect()));

        let (hmin, hmax) = (v3(model.hull_min), v3(model.hull_max));
        let base = (hmin + hmax) * 0.5;
        let radius = ((hmax - hmin).length() * 0.5).max(4.0);
        let dist = radius * 2.6 * mv.zoom;
        // Pan along the view plane with right/middle drag
        let dir = Vec3::new(mv.pitch.cos() * mv.yaw.cos(), mv.pitch.cos() * mv.yaw.sin(), mv.pitch.sin());
        let right = (-dir).cross(Vec3::Z).normalize_or_zero();
        let up = right.cross(-dir).normalize_or_zero();
        let mut mv_pan = |dx: f32, dy: f32| mv.pan += right * dx + up * dy;
        if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
            let d = resp.drag_delta();
            let per_px = dist * 0.97 / resp.rect.height().max(1.0);
            mv_pan(-d.x * per_px, d.y * per_px);
        }
        let center = base + mv.pan;
        let eye = center + dist * Vec3::new(mv.pitch.cos() * mv.yaw.cos(), mv.pitch.cos() * mv.yaw.sin(), mv.pitch.sin());
        let aspect = resp.rect.width() / resp.rect.height().max(1.0);
        let vp = glam::camera::rh::proj::opengl::perspective(0.9, aspect, (dist * 0.002).max(0.001), dist * 100.0 + radius * 4.0) * glam::camera::rh::view::look_at_mat4(eye, center, Vec3::Z);
        let cam = Cam { vp, rect: resp.rect };

        let frame = model.sequences.get(mv.seq).map(|s| s.frame_at(mv.time)).unwrap_or(0.0);
        let pose = if model.sequences.is_empty() { None } else { model.pose(mv.seq, frame) };

        if mv.show_mesh {
            self.mv_mesh(mv, &model, frame);
        } else {
            mv.mesh_key = Default::default();
            if let Ok(mut sh) = self.shared.lock() {
                if !sh.preview.model_batches.is_empty() {
                    sh.preview.model_batches.clear();
                    sh.preview.models_version += 1;
                }
            }
        }
        if let Ok(mut sh) = self.shared.lock() {
            sh.preview.mvp = vp;
            sh.preview.lighting = true;
            sh.preview.cull = true;
            sh.preview.wireframe = false;
            sh.preview.clear = [0.15, 0.15, 0.16];
        }
        painter.add(render3d::paint_callback_for(self.shared.clone(), resp.rect, render3d::Which::Preview));

        if mv.show_hull {
            let c = [hmin, Vec3::new(hmax.x, hmin.y, hmin.z), Vec3::new(hmax.x, hmax.y, hmin.z), Vec3::new(hmin.x, hmax.y, hmin.z)];
            let up = Vec3::new(0.0, 0.0, hmax.z - hmin.z);
            for i in 0..4 {
                for (a, b) in [(c[i], c[(i + 1) % 4]), (c[i] + up, c[(i + 1) % 4] + up), (c[i], c[i] + up)] {
                    if let (Some(a), Some(b)) = (cam.project(a), cam.project(b)) {
                        painter.line_segment([a, b], Stroke::new(1.0, Color32::from_rgb(90, 160, 90)));
                    }
                }
            }
        }

        let to_model: Vec<DMat4> = pose.map(|p| p.0).unwrap_or_else(|| vec![DMat4::IDENTITY; model.bone_count()]);
        if mv.show_bones {
            for i in 0..model.bone_count() {
                let Some(a) = to_model.get(i).map(|m| v3(m.w_axis.truncate())) else { continue };
                let Some(pa) = cam.project(a) else { continue };
                if let Some(par) = model.bone_parent(i) {
                    if let Some(pb) = to_model.get(par).and_then(|m| cam.project(v3(m.w_axis.truncate()))) {
                        painter.line_segment([pa, pb], Stroke::new(1.5, Color32::from_rgb(240, 200, 60)));
                    }
                }
                painter.circle_filled(pa, 2.5, Color32::from_rgb(255, 220, 80));
            }
        }
        if mv.show_attach {
            let axes = radius * 0.12;
            for a in &model.attachments {
                let m = model.attachment_matrix(a, &to_model);
                let o = v3(m.w_axis.truncate());
                let Some(po) = cam.project(o) else { continue };
                for (ax, col) in [(m.x_axis, Color32::RED), (m.y_axis, Color32::GREEN), (m.z_axis, Color32::from_rgb(80, 140, 255))] {
                    if let Some(pe) = cam.project(o + v3(ax.truncate()) * axes) {
                        painter.line_segment([po, pe], Stroke::new(2.0, col));
                    }
                }
                painter.circle_filled(po, 3.0, Color32::WHITE);
                painter.text(po + egui::vec2(5.0, -5.0), egui::Align2::LEFT_BOTTOM, &a.name, egui::FontId::proportional(12.0), Color32::from_rgb(120, 230, 255));
            }
        }
        painter.text(resp.rect.left_bottom() + egui::vec2(6.0, -6.0), egui::Align2::LEFT_BOTTOM, "drag: orbit   right/middle drag: move   wheel: zoom", egui::FontId::proportional(11.0), Color32::from_gray(160));

        // info
        ui.weak(format!(
            "{} parts, {} tris, {} bones, {} attachments, {} sequences",
            model.parts.len(),
            model.parts.iter().map(|p| p.bind.len() / 3).sum::<usize>(),
            model.bone_count(),
            model.attachments.len(),
            model.sequences.len()
        ));
        if !model.attachments.is_empty() {
            egui::CollapsingHeader::new("Attachments").id_salt("mv_att").show(ui, |ui| {
                for a in &model.attachments {
                    ui.label(format!("{}  (bone: {})", a.name, model.bone_name(a.bone)));
                }
            });
        }
    }
}
