//! Browser backend: game files and maps come from folders picked with the File System Access API.
//!
//! The browser API is asynchronous but [`Vfs`] is not. Reads that miss the cache start a fetch
//! and answer "not there" for now; callers retry next frame (see [`Vfs::pending`] and
//! [`Vfs::stalls`]). Loading a game therefore re-runs its (cheap) setup until a whole pass
//! issues no new fetches.
//!
//! Virtual paths start with the name of a picked folder (`Portal 2/portal2/maps/x.vmf`).
//! Picked folders are remembered in IndexedDB; after a reload the browser may ask for
//! permission again, which needs a click (see [`WebFs::reconnect`]).

mod detect;
mod fsa;

use super::{DirEntry, Vfs};
use crate::config::GameConfig;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::rc::Rc;
use wasm_bindgen::JsValue;

pub use fsa::{storage_get, storage_set};

/// Upper bound for cached file bytes; the oldest entries are dropped first.
const CACHE_BYTES: usize = 384 << 20;

pub fn supported() -> bool {
    fsa::has_picker()
}

/// Outcome of an asynchronous browser action, collected with [`WebFs::take_events`].
pub enum WebEvent {
    /// A game folder was picked: its (unique) name and the game configurations found in it.
    Picked(Result<(String, Vec<GameConfig>), String>),
    /// Remembered folders were looked up. `locked` ones need [`WebFs::reconnect`] (a click).
    Restored { locked: Vec<String> },
    /// A map file was picked and read: virtual path and contents.
    Opened(Result<(String, Vec<u8>), String>),
    /// A map file was written (also after "Save As").
    Saved { path: String, result: Result<(), String> },
}

enum Slot<T> {
    Pending,
    /// `None`: the file or directory does not exist.
    Ready(Option<T>),
}

enum CacheKey {
    File(String),
    Range(String, u64, usize),
}

type Bytes = Rc<Vec<u8>>;
type DirList = Rc<Vec<(String, bool)>>;

#[derive(Default)]
struct Inner {
    roots: RefCell<HashSet<String>>,
    /// remembered folders waiting for the user to grant access again
    locked: RefCell<Vec<(String, JsValue)>>,
    files: RefCell<HashMap<String, Slot<Bytes>>>,
    ranges: RefCell<HashMap<(String, u64, usize), Slot<Bytes>>>,
    dirs: RefCell<HashMap<String, Slot<DirList>>>,
    stats: RefCell<HashMap<String, Slot<u8>>>,
    order: RefCell<VecDeque<CacheKey>>,
    bytes: Cell<usize>,
    pending: Cell<usize>,
    stalls: Cell<usize>,
    ctx: RefCell<Option<eframe::egui::Context>>,
    events: RefCell<VecDeque<WebEvent>>,
}

#[derive(Clone, Default)]
pub struct WebFs {
    inner: Rc<Inner>,
}

/// Normalises to a `/`-separated path without `.`/`..`/empty parts.
fn key(path: &Path) -> String {
    let mut parts: Vec<String> = vec![];
    for c in path.to_string_lossy().replace('\\', "/").split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c.to_string()),
        }
    }
    parts.join("/")
}

fn to_bytes(v: &JsValue) -> Option<Bytes> {
    (!v.is_null() && !v.is_undefined()).then(|| Rc::new(js_sys::Uint8Array::new(v).to_vec()))
}

fn js_prop(e: &JsValue, name: &str) -> String {
    js_sys::Reflect::get(e, &name.into()).ok().and_then(|n| n.as_string()).unwrap_or_default()
}

/// Human-readable message for a rejected JS promise; `AbortError` (dialog cancelled) is quiet.
fn js_err(e: &JsValue) -> String {
    if js_prop(e, "name") == "AbortError" {
        return "Cancelled".to_string();
    }
    let msg = js_prop(e, "message");
    if msg.is_empty() {
        format!("{e:?}")
    } else {
        msg
    }
}

impl WebFs {
    pub fn new() -> WebFs {
        WebFs::default()
    }

    pub fn set_ctx(&self, ctx: eframe::egui::Context) {
        *self.inner.ctx.borrow_mut() = Some(ctx);
    }

    fn repaint(&self) {
        if let Some(c) = &*self.inner.ctx.borrow() {
            c.request_repaint();
        }
    }

    pub fn take_events(&self) -> Vec<WebEvent> {
        self.inner.events.borrow_mut().drain(..).collect()
    }

    fn push(&self, e: WebEvent) {
        self.inner.events.borrow_mut().push_back(e);
        self.repaint();
    }

    /// Whether `k` can be served: it lies in a known folder (or is the folder list / a loose file).
    fn has_root_for(&self, k: &str) -> bool {
        k.is_empty() || k.starts_with("@file/") || self.inner.roots.borrow().contains(k.split('/').next().unwrap_or(""))
    }

    fn register_root(&self, name: &str, handle: &JsValue) {
        fsa::add_root(name, handle);
        self.inner.roots.borrow_mut().insert(name.to_string());
        // anything cached under this name came from an earlier root of the same name
        let prefix = format!("{name}/");
        let i = &self.inner;
        i.files.borrow_mut().retain(|k, _| !k.starts_with(&prefix));
        i.ranges.borrow_mut().retain(|k, _| !k.0.starts_with(&prefix));
        i.dirs.borrow_mut().retain(|k, _| !k.starts_with(&prefix) && k != name && !k.is_empty());
        i.stats.borrow_mut().retain(|k, _| !k.starts_with(&prefix) && k != name && !k.is_empty());
    }

    /// Registers a freshly picked folder (once) and remembers it for the next visit.
    /// Returns the name its paths start with and the folder's own name.
    async fn add_root(&self, handle: &JsValue) -> (String, String) {
        let orig = fsa::handle_name(handle);
        if let Ok(v) = fsa::find_root(handle).await {
            if let Some(existing) = v.as_string() {
                return (existing, orig);
            }
        }
        let mut name = orig.clone();
        let mut n = 2;
        while self.inner.roots.borrow().contains(&name) {
            name = format!("{orig} ({n})");
            n += 1;
        }
        self.register_root(&name, handle);
        let _ = fsa::idb_put(&name, handle).await;
        (name, orig)
    }

    // ---- folders ----------------------------------------------------------------------

    /// Asks for a game folder, then works out its game configurations. The outcome arrives as
    /// [`WebEvent::Picked`]. Must run from a click handler.
    pub fn start_pick(&self) {
        let me = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let res = match fsa::pick_dir().await {
                Ok(h) => {
                    let (name, orig) = me.add_root(&h).await;
                    detect::detect(&me, &name, &orig).await.map(|c| (name, c))
                }
                Err(e) => Err(js_err(&e)),
            };
            me.push(WebEvent::Picked(res));
        });
    }

    /// Looks up the folders remembered from earlier visits and re-attaches those the browser
    /// still lets us use. The rest are reported as locked.
    pub fn start_restore(&self) {
        let me = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let mut locked = vec![];
            if let Ok(all) = fsa::idb_all().await {
                for pair in js_sys::Array::from(&all).iter() {
                    let pair = js_sys::Array::from(&pair);
                    let (Some(name), handle) = (pair.get(0).as_string(), pair.get(1)) else { continue };
                    let perm = fsa::query_perm(&handle).await.ok().and_then(|p| p.as_string());
                    if perm.as_deref() == Some("granted") {
                        me.register_root(&name, &handle);
                    } else {
                        locked.push((name, handle));
                    }
                }
            }
            me.finish_restore(locked);
        });
    }

    /// Asks permission for the remembered folders that need it. Must run from a click handler.
    pub fn reconnect(&self) {
        let me = self.clone();
        let todo: Vec<(String, JsValue)> = self.inner.locked.borrow_mut().drain(..).collect();
        wasm_bindgen_futures::spawn_local(async move {
            let mut locked = vec![];
            for (name, handle) in todo {
                let perm = fsa::request_perm(&handle).await.ok().and_then(|p| p.as_string());
                if perm.as_deref() == Some("granted") {
                    me.register_root(&name, &handle);
                } else {
                    locked.push((name, handle));
                }
            }
            me.finish_restore(locked);
        });
    }

    fn finish_restore(&self, locked: Vec<(String, JsValue)>) {
        let names = locked.iter().map(|(n, _)| n.clone()).collect();
        *self.inner.locked.borrow_mut() = locked;
        self.push(WebEvent::Restored { locked: names });
    }

    /// Forgets a remembered folder.
    pub fn forget(&self, name: &str) {
        let name = name.to_string();
        self.inner.roots.borrow_mut().remove(&name);
        wasm_bindgen_futures::spawn_local(async move {
            let _ = fsa::idb_delete(&name).await;
        });
    }

    /// Names of the folders that can currently be read.
    pub fn roots(&self) -> Vec<String> {
        self.inner.roots.borrow().iter().cloned().collect()
    }

    // ---- maps -------------------------------------------------------------------------

    /// Virtual path for a picked file: inside a known folder, or a loose `@file/` entry.
    async fn path_for(&self, handle: &JsValue) -> String {
        match fsa::locate(handle).await {
            Ok(v) if v.as_string().is_some() => v.as_string().unwrap_or_default(),
            _ => fsa::register_external(handle),
        }
    }

    /// Lets the user pick a `.vmf`, starting in `start_dir` (a virtual directory). Arrives as
    /// [`WebEvent::Opened`]. Must run from a click handler.
    pub fn start_open(&self, start_dir: &str) {
        let me = self.clone();
        let start = start_dir.to_string();
        wasm_bindgen_futures::spawn_local(async move {
            let res = match fsa::pick_open_file(&start).await {
                Ok(h) => {
                    let path = me.path_for(&h).await;
                    match fsa::read_file(&path).await {
                        Ok(v) => to_bytes(&v).map(|b| (path, b.to_vec())).ok_or_else(|| "The file could not be read".to_string()),
                        Err(e) => Err(js_err(&e)),
                    }
                }
                Err(e) => Err(js_err(&e)),
            };
            me.push(WebEvent::Opened(res));
        });
    }

    /// Lets the user choose where to save, then writes `data` there ([`WebEvent::Saved`]).
    /// Must run from a click handler.
    pub fn start_save_as(&self, start_dir: &str, suggested: &str, data: Vec<u8>) {
        let me = self.clone();
        let (start, suggested) = (start_dir.to_string(), suggested.to_string());
        wasm_bindgen_futures::spawn_local(async move {
            match fsa::pick_save_file(&start, &suggested).await {
                Ok(h) => {
                    let path = me.path_for(&h).await;
                    me.write(path, data).await;
                }
                Err(e) => me.push(WebEvent::Saved { path: String::new(), result: Err(js_err(&e)) }),
            }
        });
    }

    /// Overwrites a file that was opened or saved before ([`WebEvent::Saved`]).
    pub fn start_write(&self, path: &str, data: Vec<u8>) {
        let me = self.clone();
        let path = path.to_string();
        wasm_bindgen_futures::spawn_local(async move { me.write(path, data).await });
    }

    async fn write(&self, path: String, data: Vec<u8>) {
        let result = fsa::write_file(&path, &data).await.map(|_| ()).map_err(|e| js_err(&e));
        // cached copies of that file are stale now
        let k = key(Path::new(&path));
        self.inner.files.borrow_mut().remove(&k);
        self.inner.stats.borrow_mut().remove(&k);
        self.push(WebEvent::Saved { path, result });
    }

    // ---- cache ------------------------------------------------------------------------

    fn stall(&self) {
        self.inner.stalls.set(self.inner.stalls.get() + 1);
    }

    fn begin(&self) {
        self.inner.pending.set(self.inner.pending.get() + 1);
    }

    fn end(&self) {
        self.inner.pending.set(self.inner.pending.get().saturating_sub(1));
        self.repaint();
    }

    fn account(&self, k: CacheKey, len: usize) {
        let i = &self.inner;
        i.bytes.set(i.bytes.get() + len);
        i.order.borrow_mut().push_back(k);
        while i.bytes.get() > CACHE_BYTES {
            let Some(old) = i.order.borrow_mut().pop_front() else { break };
            let freed = match &old {
                CacheKey::File(p) => match i.files.borrow_mut().remove(p) {
                    Some(Slot::Ready(Some(b))) => b.len(),
                    _ => 0,
                },
                CacheKey::Range(p, o, l) => match i.ranges.borrow_mut().remove(&(p.clone(), *o, *l)) {
                    Some(Slot::Ready(Some(b))) => b.len(),
                    _ => 0,
                },
            };
            i.bytes.set(i.bytes.get().saturating_sub(freed));
        }
    }

    async fn fetch_file(&self, k: &str) -> Option<Bytes> {
        fsa::read_file(k).await.ok().and_then(|v| to_bytes(&v))
    }

    async fn fetch_range(&self, k: &str, off: u64, len: usize) -> Option<Bytes> {
        fsa::read_range(k, off as f64, len as f64).await.ok().and_then(|v| to_bytes(&v))
    }

    async fn fetch_dir(&self, k: &str) -> Option<DirList> {
        let v = fsa::list_dir(k).await.ok()?;
        if v.is_null() {
            return None;
        }
        let list = js_sys::Array::from(&v)
            .iter()
            .filter_map(|n| n.as_string())
            .map(|n| match n.strip_suffix('/') {
                Some(d) => (d.to_string(), true),
                None => (n, false),
            })
            .collect();
        Some(Rc::new(list))
    }

    async fn fetch_stat(&self, k: &str) -> u8 {
        fsa::stat_path(k).await.ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as u8
    }

    fn store_file(&self, k: String, v: Option<Bytes>) {
        let len = v.as_ref().map_or(0, |b| b.len());
        self.inner.files.borrow_mut().insert(k.clone(), Slot::Ready(v));
        self.account(CacheKey::File(k), len);
    }

    /// Whole file, awaiting the fetch (for setup code that can wait).
    pub async fn ensure_file(&self, path: &str) -> Option<Bytes> {
        let k = key(Path::new(path));
        if let Some(Slot::Ready(v)) = self.inner.files.borrow().get(&k) {
            return v.clone();
        }
        let v = self.fetch_file(&k).await;
        self.store_file(k, v.clone());
        v
    }

    pub async fn ensure_stat(&self, path: &str) -> u8 {
        let k = key(Path::new(path));
        if let Some(Slot::Ready(Some(v))) = self.inner.stats.borrow().get(&k) {
            return *v;
        }
        let v = self.fetch_stat(&k).await;
        self.inner.stats.borrow_mut().insert(k, Slot::Ready(Some(v)));
        v
    }

    pub async fn ensure_dir(&self, path: &str) -> Vec<(String, bool)> {
        let k = key(Path::new(path));
        if let Some(Slot::Ready(v)) = self.inner.dirs.borrow().get(&k) {
            return v.as_ref().map(|l| l.to_vec()).unwrap_or_default();
        }
        let v = self.fetch_dir(&k).await;
        self.inner.dirs.borrow_mut().insert(k, Slot::Ready(v.clone()));
        v.map(|l| l.to_vec()).unwrap_or_default()
    }

    fn stat(&self, path: &Path) -> u8 {
        let k = key(path);
        match self.inner.stats.borrow().get(&k) {
            Some(Slot::Ready(v)) => return v.unwrap_or(0),
            Some(Slot::Pending) => {
                self.stall();
                return 0;
            }
            None => {}
        }
        if !self.has_root_for(&k) {
            return 0;
        }
        self.stall();
        self.inner.stats.borrow_mut().insert(k.clone(), Slot::Pending);
        self.begin();
        let me = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let v = me.fetch_stat(&k).await;
            me.inner.stats.borrow_mut().insert(k, Slot::Ready(Some(v)));
            me.end();
        });
        0
    }
}

impl Vfs for WebFs {
    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        let k = key(path);
        match self.inner.files.borrow().get(&k) {
            Some(Slot::Ready(v)) => return v.as_ref().map(|b| b.to_vec()),
            Some(Slot::Pending) => {
                self.stall();
                return None;
            }
            None => {}
        }
        if !self.has_root_for(&k) {
            return None;
        }
        self.stall();
        self.inner.files.borrow_mut().insert(k.clone(), Slot::Pending);
        self.begin();
        let me = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let v = me.fetch_file(&k).await;
            me.store_file(k, v);
            me.end();
        });
        None
    }

    fn read_range(&self, path: &Path, off: u64, len: usize) -> Option<Vec<u8>> {
        let k = (key(path), off, len);
        match self.inner.ranges.borrow().get(&k) {
            Some(Slot::Ready(v)) => return v.as_ref().map(|b| b.to_vec()),
            Some(Slot::Pending) => {
                self.stall();
                return None;
            }
            None => {}
        }
        if !self.has_root_for(&k.0) {
            return None;
        }
        self.stall();
        self.inner.ranges.borrow_mut().insert(k.clone(), Slot::Pending);
        self.begin();
        let me = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let v = me.fetch_range(&k.0, k.1, k.2).await;
            let n = v.as_ref().map_or(0, |b| b.len());
            me.inner.ranges.borrow_mut().insert(k.clone(), Slot::Ready(v));
            me.account(CacheKey::Range(k.0, k.1, k.2), n);
            me.end();
        });
        None
    }

    fn is_file(&self, path: &Path) -> bool {
        self.stat(path) == 1
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.stat(path) == 2
    }

    fn read_dir(&self, path: &Path) -> Vec<DirEntry> {
        let k = key(path);
        match self.inner.dirs.borrow().get(&k) {
            Some(Slot::Ready(v)) => {
                return v
                    .iter()
                    .flat_map(|l| l.iter())
                    .map(|(name, is_dir)| DirEntry { name: name.clone(), is_dir: *is_dir })
                    .collect();
            }
            Some(Slot::Pending) => {
                self.stall();
                return vec![];
            }
            None => {}
        }
        if !self.has_root_for(&k) {
            return vec![];
        }
        self.stall();
        self.inner.dirs.borrow_mut().insert(k.clone(), Slot::Pending);
        self.begin();
        let me = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let v = me.fetch_dir(&k).await;
            me.inner.dirs.borrow_mut().insert(k, Slot::Ready(v));
            me.end();
        });
        vec![]
    }

    fn pending(&self) -> usize {
        self.inner.pending.get()
    }

    fn stalls(&self) -> usize {
        self.inner.stalls.get()
    }
}
