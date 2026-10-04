//! Developer aid: scripted screenshots driven by RHAMMER_* environment variables.

use super::App;
use crate::editor::doc::Sel;
use crate::ui::layout::Pane;
use eframe::egui;
use glam::DVec3;

impl App {
    /// Developer aid: RHAMMER_SHOT=<file.png> saves a screenshot after a few frames and exits.
    #[cfg(feature = "local")]
    pub(crate) fn debug_screenshot(&mut self, ctx: &egui::Context) {
        let Ok(path) = std::env::var("RHAMMER_SHOT") else { return };
        let frames: u64 = std::env::var("RHAMMER_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
        let n = ctx.cumulative_pass_nr();
        ctx.request_repaint();
        if n == frames / 2 {
            if let Ok(s) = std::env::var("RHAMMER_SELECT_ALL") {
                if s == "1" {
                    let all: Sel = self.doc.all_ids().into_iter().collect();
                    self.set_sel(all);
                }
            }
            if let Ok(t) = std::env::var("RHAMMER_TAB") {
                match t.as_str() {
                    "tex" => self.focus_pane = Some(Pane::Textures),
                    "vis" => self.focus_pane = Some(Pane::VisGroups),
                    _ => self.open_properties(),
                }
            }
            if let Ok(id) = std::env::var("RHAMMER_PICK") {
                if let Ok(id) = id.parse::<u32>() {
                    self.set_sel([id].into_iter().collect());
                }
            }
            if std::env::var("RHAMMER_FRAME").is_ok() {
                self.frame_selection();
            }
            if let Ok(m) = std::env::var("RHAMMER_MAX") {
                self.maximized = m.parse().ok();
            }
            if let Ok(c) = std::env::var("RHAMMER_CAM") {
                let v: Vec<f64> = c.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                if v.len() == 5 {
                    self.cam.pos = DVec3::new(v[0], v[1], v[2]);
                    self.cam.yaw = v[3];
                    self.cam.pitch = v[4];
                }
            }
            if std::env::var("RHAMMER_WIN").ok().as_deref() == Some("cfg") {
                self.win.game_cfg = true;
            }
        }
        if n == frames {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            let [w, h] = img.size;
            let mut buf = Vec::with_capacity(w * h * 4);
            for p in &img.pixels {
                buf.extend_from_slice(&p.to_array());
            }
            let _ = image_save(&path, w as u32, h as u32, &buf);
            std::process::exit(0);
        }
    }
}

/// Minimal uncompressed PNG writer (no extra dependency).
#[cfg(feature = "local")]
fn image_save(path: &str, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = ty.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    }
    let mut raw = Vec::with_capacity((w as usize * 4 + 1) * h as usize);
    for y in 0..h as usize {
        raw.push(0);
        raw.extend_from_slice(&rgba[y * w as usize * 4..(y + 1) * w as usize * 4]);
    }
    // zlib stored blocks
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    while let Some(c) = chunks.next() {
        z.push(if chunks.peek().is_none() { 1 } else { 0 });
        z.extend_from_slice(&(c.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        z.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    std::fs::File::create(path)?.write_all(&out)
}
