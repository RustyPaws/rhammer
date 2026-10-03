// Thin wrappers over the File System Access API.
//
// Virtual paths are '/'-separated. Their first component names a picked root folder
// ("Portal 2/portal2/maps/x.vmf"); the empty path lists the roots. Files picked individually
// that lie outside every root live under "@file/<name>".

const roots = new Map(); // name -> FileSystemDirectoryHandle
const externals = new Map(); // "@file/<name>" -> FileSystemFileHandle
let dirCache = new Map();

const VMF_TYPES = [{ description: 'Valve Map', accept: { 'text/plain': ['.vmf'] } }];

export function hasPicker() {
  return typeof window.showDirectoryPicker === 'function' && typeof window.showOpenFilePicker === 'function';
}

export async function pickDir() {
  return await window.showDirectoryPicker({ id: 'rhammer-game', mode: 'readwrite' });
}

export function handleName(h) {
  return h.name;
}

export function addRoot(name, handle) {
  roots.set(name, handle);
  for (const k of [...dirCache.keys()]) {
    if (k === '/' + name || k.startsWith('/' + name + '/')) dirCache.delete(k);
  }
}

// Name of the root that is the same folder as `handle`, or null.
export async function findRoot(handle) {
  for (const [name, r] of roots) {
    if (await r.isSameEntry(handle)) return name;
  }
  return null;
}

function parts(path) {
  return path.split('/').filter((p) => p.length);
}

async function dirHandle(segs) {
  if (!segs.length) return null;
  let h = roots.get(segs[0]);
  if (!h) return null;
  let key = '/' + segs[0];
  for (const s of segs.slice(1)) {
    key += '/' + s;
    let next = dirCache.get(key);
    if (!next) {
      try {
        next = await h.getDirectoryHandle(s);
      } catch (e) {
        return null;
      }
      dirCache.set(key, next);
    }
    h = next;
  }
  return h;
}

async function fileHandle(path, create) {
  if (externals.has(path)) return externals.get(path);
  const segs = parts(path);
  if (segs.length < 2) return null;
  const dir = await dirHandle(segs.slice(0, -1));
  if (!dir) return null;
  try {
    return await dir.getFileHandle(segs[segs.length - 1], { create: !!create });
  } catch (e) {
    return null;
  }
}

// Uint8Array of the whole file, or null if it does not exist.
export async function readFile(path) {
  const h = await fileHandle(path, false);
  if (!h) return null;
  const f = await h.getFile();
  return new Uint8Array(await f.arrayBuffer());
}

export async function readRange(path, off, len) {
  const h = await fileHandle(path, false);
  if (!h) return null;
  const f = await h.getFile();
  if (off + len > f.size) return null;
  return new Uint8Array(await f.slice(off, off + len).arrayBuffer());
}

// Array of entry names; directories carry a trailing '/'. null if the directory is missing.
export async function listDir(path) {
  const segs = parts(path);
  if (!segs.length) return [...roots.keys()].map((n) => n + '/');
  const h = await dirHandle(segs);
  if (!h) return null;
  const out = [];
  for await (const [name, entry] of h.entries()) {
    out.push(entry.kind === 'directory' ? name + '/' : name);
  }
  return out;
}

// 0 = missing, 1 = file, 2 = directory
export async function statPath(path) {
  if (externals.has(path)) return 1;
  const segs = parts(path);
  if (!segs.length) return 2;
  if (segs.length === 1) return roots.has(segs[0]) ? 2 : 0;
  const dir = await dirHandle(segs.slice(0, -1));
  if (!dir) return 0;
  const name = segs[segs.length - 1];
  try {
    await dir.getFileHandle(name);
    return 1;
  } catch (e) {}
  try {
    await dir.getDirectoryHandle(name);
    return 2;
  } catch (e) {}
  return 0;
}

// Creates or overwrites a file. Throws on failure.
export async function writeFile(path, bytes) {
  const h = await fileHandle(path, true);
  if (!h) throw new Error('cannot open ' + path + ' for writing');
  const w = await h.createWritable();
  await w.write(bytes);
  await w.close();
}

// Virtual path of a picked file handle, or null when it is outside every root.
export async function locate(handle) {
  for (const [name, r] of roots) {
    const p = await r.resolve(handle);
    if (p !== null) return name + '/' + p.join('/');
  }
  return null;
}

// Remembers a file that is outside every root; returns its virtual path.
export function registerExternal(handle) {
  let key = '@file/' + handle.name;
  let n = 2;
  while (externals.has(key) && externals.get(key) !== handle) {
    key = '@file/' + n++ + '-' + handle.name;
  }
  externals.set(key, handle);
  return key;
}

export async function pickOpenFile(startPath) {
  const opts = { types: VMF_TYPES, excludeAcceptAllOption: false, multiple: false, id: 'rhammer-map' };
  const dir = await dirHandle(parts(startPath));
  if (dir) opts.startIn = dir;
  const [h] = await window.showOpenFilePicker(opts);
  return h;
}

export async function pickSaveFile(startPath, suggestedName) {
  const opts = { types: VMF_TYPES, suggestedName, id: 'rhammer-map' };
  const dir = await dirHandle(parts(startPath));
  if (dir) opts.startIn = dir;
  return await window.showSaveFilePicker(opts);
}

// ---- remembered folders (IndexedDB) and permissions ------------------------------------

function db() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open('rhammer', 1);
    req.onupgradeneeded = () => req.result.createObjectStore('roots');
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

function done(tx) {
  return new Promise((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error);
  });
}

export async function idbPut(name, handle) {
  const d = await db();
  const tx = d.transaction('roots', 'readwrite');
  tx.objectStore('roots').put(handle, name);
  await done(tx);
}

export async function idbDelete(name) {
  const d = await db();
  const tx = d.transaction('roots', 'readwrite');
  tx.objectStore('roots').delete(name);
  await done(tx);
}

// [[name, handle], ...]
export async function idbAll() {
  const d = await db();
  const store = d.transaction('roots', 'readonly').objectStore('roots');
  const keys = await new Promise((res, rej) => {
    const r = store.getAllKeys();
    r.onsuccess = () => res(r.result);
    r.onerror = () => rej(r.error);
  });
  const vals = await new Promise((res, rej) => {
    const r = store.getAll();
    r.onsuccess = () => res(r.result);
    r.onerror = () => rej(r.error);
  });
  return keys.map((k, i) => [k, vals[i]]);
}

export async function queryPerm(handle) {
  return await handle.queryPermission({ mode: 'readwrite' });
}

export async function requestPerm(handle) {
  return await handle.requestPermission({ mode: 'readwrite' });
}

// ---- settings (localStorage) ------------------------------------------------------------

export function storageGet(key) {
  try {
    return window.localStorage.getItem(key);
  } catch (e) {
    return null;
  }
}

export function storageSet(key, value) {
  try {
    window.localStorage.setItem(key, value);
  } catch (e) {}
}
