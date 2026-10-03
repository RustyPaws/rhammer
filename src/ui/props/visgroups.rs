//! VisGroups tab.

use crate::editor::doc::Sel;
use eframe::egui::{self, RichText};
use super::*;

impl App {
    pub(crate) fn visgroups_tab(&mut self, ui: &mut egui::Ui) {
        let groups = self.doc.visgroups();
        if groups.is_empty() {
            ui.label(RichText::new("No visgroups").weak());
        }
        let mut toggle: Option<(u32, bool)> = None;
        let mut select: Option<u32> = None;
        let mut assign: Option<(u32, bool)> = None;
        let mut delete: Option<u32> = None;
        for g in &groups {
            ui.horizontal(|ui| {
                ui.add_space(g.depth as f32 * 14.0);
                let mut shown = self.doc.visgroup_shown(g.id);
                if ui.checkbox(&mut shown, "").changed() {
                    toggle = Some((g.id, shown));
                }
                ui.label(&g.name);
                if ui.small_button("sel").on_hover_text("select members").clicked() {
                    select = Some(g.id);
                }
                if ui.small_button("+").on_hover_text("add selection to group").clicked() {
                    assign = Some((g.id, true));
                }
                if ui.small_button("−").on_hover_text("remove selection from group").clicked() {
                    assign = Some((g.id, false));
                }
                if ui.small_button("🗑").on_hover_text("delete group (objects are kept)").clicked() {
                    delete = Some(g.id);
                }
            });
        }
        if let Some(id) = delete {
            self.doc.checkpoint();
            self.doc.delete_visgroup(id);
            self.bump_sel();
            return;
        }
        if let Some((id, shown)) = toggle {
            self.doc.checkpoint();
            self.doc.set_visgroup_shown(id, shown);
            self.sel.retain(|i| !self.doc.is_hidden(*i));
            self.bump_sel();
        }
        if let Some(id) = select {
            let m: Sel = self.doc.visgroup_members(id).into_iter().collect();
            self.set_sel(m);
        }
        if let Some((id, add)) = assign {
            self.doc.checkpoint();
            let s = self.sel.clone();
            self.doc.assign_visgroup(&s, id, add);
            self.doc.touch();
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.new_visgroup).hint_text("new group name"));
            if ui.button("Add group").clicked() && !self.new_visgroup.trim().is_empty() {
                self.doc.checkpoint();
                let n = self.new_visgroup.trim().to_string();
                self.doc.add_visgroup(&n);
                self.new_visgroup.clear();
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Show all").clicked() {
                self.doc.checkpoint();
                for g in &groups {
                    self.doc.set_visgroup_shown(g.id, true);
                }
                // also reveal anything hidden outside groups
                self.doc.touch();
            }
        });
    }
}
