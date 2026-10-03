//! Bindings to `fsa.js` (File System Access API, IndexedDB, localStorage).

use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/platform/web/fsa.js")]
extern "C" {
    #[wasm_bindgen(js_name = hasPicker)]
    pub fn has_picker() -> bool;
    #[wasm_bindgen(js_name = pickDir, catch)]
    pub async fn pick_dir() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = handleName)]
    pub fn handle_name(handle: &JsValue) -> String;
    #[wasm_bindgen(js_name = findRoot, catch)]
    pub async fn find_root(handle: &JsValue) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = addRoot)]
    pub fn add_root(name: &str, handle: &JsValue);

    #[wasm_bindgen(js_name = readFile, catch)]
    pub async fn read_file(path: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = readRange, catch)]
    pub async fn read_range(path: &str, off: f64, len: f64) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = listDir, catch)]
    pub async fn list_dir(path: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = statPath, catch)]
    pub async fn stat_path(path: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = writeFile, catch)]
    pub async fn write_file(path: &str, bytes: &[u8]) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch)]
    pub async fn locate(handle: &JsValue) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = registerExternal)]
    pub fn register_external(handle: &JsValue) -> String;
    #[wasm_bindgen(js_name = pickOpenFile, catch)]
    pub async fn pick_open_file(start: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = pickSaveFile, catch)]
    pub async fn pick_save_file(start: &str, suggested: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_name = idbPut, catch)]
    pub async fn idb_put(name: &str, handle: &JsValue) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = idbDelete, catch)]
    pub async fn idb_delete(name: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = idbAll, catch)]
    pub async fn idb_all() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = queryPerm, catch)]
    pub async fn query_perm(handle: &JsValue) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = requestPerm, catch)]
    pub async fn request_perm(handle: &JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_name = storageGet)]
    pub fn storage_get(key: &str) -> Option<String>;
    #[wasm_bindgen(js_name = storageSet)]
    pub fn storage_set(key: &str, value: &str);
}
