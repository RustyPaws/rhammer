//! Map compilation: vbsp -> vvis -> vrad -> copy bsp -> launch game, run on a worker thread.

use crate::config::{CompileSettings, GameConfig};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

pub struct CompileJob {
    pub rx: Receiver<Msg>,
    pub log: Vec<(Level, String)>,
    pub running: bool,
    pub ok: bool,
    cancel: Arc<AtomicBool>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Step,
    Error,
    Good,
}

pub enum Msg {
    Line(Level, String),
    Done(bool),
}

impl CompileJob {
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(m) = self.rx.try_recv() {
            changed = true;
            match m {
                Msg::Line(l, s) => self.log.push((l, s)),
                Msg::Done(ok) => {
                    self.running = false;
                    self.ok = ok;
                }
            }
        }
        changed
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

fn run_step(name: &str, exe: &str, args: &str, tx: &Sender<Msg>, cancel: &AtomicBool, cwd: Option<&Path>) -> bool {
    let _ = tx.send(Msg::Line(Level::Step, format!("==== {name} ====")));
    let _ = tx.send(Msg::Line(Level::Info, format!("{exe} {args}")));
    if exe.is_empty() || !Path::new(exe).exists() {
        let _ = tx.send(Msg::Line(Level::Error, format!("Executable not found: {exe:?} (check Game Configurations)")));
        return false;
    }
    let mut cmd = Command::new(exe);
    for a in split_args(args) {
        cmd.arg(a);
    }
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(Msg::Line(Level::Error, format!("Failed to start: {e}")));
            return false;
        }
    };
    let out = child.stdout.take();
    let err = child.stderr.take();
    let tx2 = tx.clone();
    let t = std::thread::spawn(move || {
        if let Some(e) = err {
            for l in BufReader::new(e).lines().map_while(Result::ok) {
                let _ = tx2.send(Msg::Line(Level::Error, l));
            }
        }
    });
    if let Some(o) = out {
        let mut reader = BufReader::new(o);
        let mut buf = Vec::new();
        // vbsp/vvis/vrad draw progress bars with '\r'; split on both
        loop {
            buf.clear();
            let mut b = [0u8; 1];
            let mut got_any = false;
            use std::io::Read;
            loop {
                match reader.read(&mut b) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        got_any = true;
                        if b[0] == b'\n' || b[0] == b'\r' {
                            break;
                        }
                        buf.push(b[0]);
                    }
                }
            }
            if !buf.is_empty() {
                let s = String::from_utf8_lossy(&buf).into_owned();
                let level = if s.to_ascii_lowercase().contains("error") { Level::Error } else { Level::Info };
                let _ = tx.send(Msg::Line(level, s));
            }
            if !got_any {
                break;
            }
            if cancel.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = tx.send(Msg::Line(Level::Error, "Cancelled".into()));
                return false;
            }
        }
    }
    let _ = t.join();
    match child.wait() {
        Ok(st) if st.success() => true,
        Ok(st) => {
            let _ = tx.send(Msg::Line(Level::Error, format!("{name} exited with {st}")));
            false
        }
        Err(e) => {
            let _ = tx.send(Msg::Line(Level::Error, format!("{name}: {e}")));
            false
        }
    }
}

/// Split a command line on spaces, honouring double and single quotes.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut q = false;
    let mut sq = false;
    let mut has = false;
    for c in s.chars() {
        match c {
            '"' if !sq => {
                q = !q;
                has = true;
            }
            '\'' if !q => {
                sq = !sq;
                has = true;
            }
            c if c.is_whitespace() && !q && !sq => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn start(game: GameConfig, cs: CompileSettings, vmf: PathBuf, ctx: eframe::egui::Context) -> CompileJob {
    let (tx, rx) = channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let c2 = cancel.clone();
    std::thread::spawn(move || {
        let ok = run(&game, &cs, &vmf, &tx, &c2);
        let _ = tx.send(Msg::Done(ok));
        ctx.request_repaint();
    });
    CompileJob { rx, log: vec![], running: true, ok: false, cancel }
}

fn run(game: &GameConfig, cs: &CompileSettings, vmf: &Path, tx: &Sender<Msg>, cancel: &AtomicBool) -> bool {
    let started = std::time::Instant::now();
    let dir = vmf.parent().unwrap_or(Path::new("."));
    let stem = vmf.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    // Valve's compilers write their log through the game's file system, which only reaches
    // folders inside the game installation (e.g. <game>/sdk_content/maps).
    let root = Path::new(&game.game_dir).parent().map(|p| p.to_path_buf());
    if let Some(root) = root {
        let norm = |p: &Path| p.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
        if !norm(dir).starts_with(&norm(&root)) {
            let _ = tx.send(Msg::Line(
                Level::Error,
                format!("Warning: the map is outside {}. vbsp may fail with \"Can't create LogFile\"; save the map under sdk_content/maps.", root.display()),
            ));
        }
    }
    if cs.run_bsp && !run_step("BSP", &game.bsp_exe, &game.expand(&cs.bsp_params, vmf), tx, cancel, Some(dir)) {
        return false;
    }
    if cs.run_vis && !run_step("VIS", &game.vis_exe, &game.expand(&cs.vis_params, vmf), tx, cancel, Some(dir)) {
        return false;
    }
    if cs.run_light && !run_step("RAD", &game.light_exe, &game.expand(&cs.light_params, vmf), tx, cancel, Some(dir)) {
        return false;
    }
    let bsp_src = dir.join(format!("{stem}.bsp"));
    let bsp_dst_dir = if game.bsp_dir.is_empty() { PathBuf::from(&game.game_dir).join("maps") } else { PathBuf::from(&game.bsp_dir) };
    if cs.copy_to_game {
        let _ = tx.send(Msg::Line(Level::Step, "==== Copy BSP to game ====".into()));
        let dst = bsp_dst_dir.join(format!("{stem}.bsp"));
        match std::fs::create_dir_all(&bsp_dst_dir).and_then(|_| std::fs::copy(&bsp_src, &dst)) {
            Ok(_) => {
                let _ = tx.send(Msg::Line(Level::Info, format!("{} -> {}", bsp_src.display(), dst.display())));
            }
            Err(e) => {
                let _ = tx.send(Msg::Line(Level::Error, format!("Copy failed: {e}")));
                return false;
            }
        }
    }
    let _ = tx.send(Msg::Line(Level::Good, format!("Compile finished in {:.1}s", started.elapsed().as_secs_f32())));
    if cs.launch_game {
        let _ = tx.send(Msg::Line(Level::Step, "==== Launching game ====".into()));
        let args = game.expand(&cs.game_params, vmf);
        let _ = tx.send(Msg::Line(Level::Info, format!("{} {args}", game.game_exe)));
        let mut cmd = Command::new(&game.game_exe);
        for a in split_args(&args) {
            cmd.arg(a);
        }
        if !game.game_exe_dir.is_empty() {
            cmd.current_dir(&game.game_exe_dir);
        }
        match cmd.spawn() {
            Ok(_) => {}
            Err(e) => {
                let _ = tx.send(Msg::Line(Level::Error, format!("Cannot launch game: {e}")));
                return false;
            }
        }
    }
    true
}
