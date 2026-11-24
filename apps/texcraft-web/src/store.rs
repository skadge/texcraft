//! What the browser keeps between visits, in IndexedDB (database `texcraft`): the projects of the
//! session's library (store `projects`, by name), the preferences (store `app`, key `prefs`, as
//! JSON) and the recent projects' thumbnails (store `thumbnails`, by project).
//!
//! IndexedDB's requests call back; each is wrapped in a Promise here and awaited. Writes are
//! awaited to their transaction's end, so an error reaches the user.

use js_sys::{Array, Object, Promise, Reflect, Uint8Array};
use texcraft_engine::library::Stored;
use texcraft_ui_egui::Thumbnail;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{IdbDatabase, IdbObjectStore, IdbOpenDbRequest, IdbRequest, IdbTransaction, IdbTransactionMode};

const NAME: &str = "texcraft";
const VERSION: u32 = 1;
const PROJECTS: &str = "projects";
const APP: &str = "app";
const THUMBNAILS: &str = "thumbnails";

fn why(e: JsValue) -> String {
    e.dyn_ref::<js_sys::Error>().map(|e| String::from(e.message())).or_else(|| e.as_string()).unwrap_or_else(|| format!("{e:?}"))
}

fn get(obj: &JsValue, key: &str) -> JsValue {
    Reflect::get(obj, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

fn object(fields: &[(&str, JsValue)]) -> Object {
    let obj = Object::new();
    for (k, v) in fields {
        let _ = Reflect::set(&obj, &JsValue::from_str(k), v);
    }
    obj
}

/// The request's result, once it succeeded.
async fn request(req: &IdbRequest) -> Result<JsValue, String> {
    let promise = Promise::new(&mut |resolve, reject| {
        let done = req.clone();
        req.set_onsuccess(Some(
            Closure::once_into_js(move || {
                let _ = resolve.call1(&JsValue::NULL, &done.result().unwrap_or(JsValue::UNDEFINED));
            })
            .unchecked_ref(),
        ));
        let failed = req.clone();
        req.set_onerror(Some(
            Closure::once_into_js(move || {
                let error = failed.error().ok().flatten().map_or_else(|| "unknown error".to_owned(), |e| e.message());
                let _ = reject.call1(&JsValue::NULL, &JsValue::from_str(&error));
            })
            .unchecked_ref(),
        ));
    });
    JsFuture::from(promise).await.map_err(why)
}

/// Waits for `tx` to be written.
async fn committed(tx: &IdbTransaction) -> Result<(), String> {
    let promise = Promise::new(&mut |resolve, reject| {
        tx.set_oncomplete(Some(
            Closure::once_into_js(move || {
                let _ = resolve.call0(&JsValue::NULL);
            })
            .unchecked_ref(),
        ));
        let failed = tx.clone();
        let on_failure = Closure::once_into_js(move || {
            let error = failed.error().map_or_else(|| "the write was cancelled".to_owned(), |e| e.message());
            let _ = reject.call1(&JsValue::NULL, &JsValue::from_str(&error));
        });
        tx.set_onerror(Some(on_failure.unchecked_ref()));
        tx.set_onabort(Some(on_failure.unchecked_ref()));
    });
    JsFuture::from(promise).await.map(|_| ()).map_err(why)
}

#[derive(Clone)]
pub struct Store {
    db: IdbDatabase,
}

impl Store {
    /// Opens the database, making its stores the first time.
    pub async fn open() -> Result<Store, String> {
        let factory =
            web_sys::window().ok_or("no window")?.indexed_db().map_err(why)?.ok_or("this browser keeps no data for web pages (IndexedDB is off)")?;
        let open: IdbOpenDbRequest = factory.open_with_u32(NAME, VERSION).map_err(why)?;
        let upgrading = open.clone();
        let upgrade = Closure::<dyn FnMut()>::new(move || {
            let Ok(db) = upgrading.result().and_then(|r| r.dyn_into::<IdbDatabase>()) else { return };
            let names = db.object_store_names();
            for store in [PROJECTS, APP, THUMBNAILS] {
                if !names.contains(store)
                    && let Err(e) = db.create_object_store(store)
                {
                    log::error!("the store {store} could not be made: {}", why(e));
                }
            }
        });
        open.set_onupgradeneeded(Some(upgrade.as_ref().unchecked_ref()));
        let db = request(&open).await?;
        drop(upgrade);
        Ok(Store { db: db.dyn_into().map_err(why)? })
    }

    fn store(&self, name: &str, mode: IdbTransactionMode) -> Result<(IdbTransaction, IdbObjectStore), String> {
        let tx = self.db.transaction_with_str_and_mode(name, mode).map_err(why)?;
        let store = tx.object_store(name).map_err(why)?;
        Ok((tx, store))
    }

    /// Every value of store `name`, with its key.
    async fn all(&self, name: &str) -> Result<Vec<(String, JsValue)>, String> {
        let (_, store) = self.store(name, IdbTransactionMode::Readonly)?;
        let keys = store.get_all_keys().map_err(why)?;
        let values = store.get_all().map_err(why)?;
        let keys: Array = request(&keys).await?.dyn_into().map_err(why)?;
        let values: Array = request(&values).await?.dyn_into().map_err(why)?;
        Ok(keys.iter().zip(values.iter()).filter_map(|(k, v)| Some((k.as_string()?, v))).collect())
    }

    async fn put(&self, name: &str, key: &str, value: &JsValue) -> Result<(), String> {
        let (tx, store) = self.store(name, IdbTransactionMode::Readwrite)?;
        store.put_with_key(value, &JsValue::from_str(key)).map_err(why)?;
        committed(&tx).await
    }

    pub async fn projects(&self) -> Result<Vec<(String, Stored)>, String> {
        Ok(self.all(PROJECTS).await?.into_iter().filter_map(|(name, v)| Some((name, project_from(&v)?))).collect())
    }

    pub async fn put_project(&self, name: &str, project: &Stored) -> Result<(), String> {
        self.put(PROJECTS, name, &project_to(project)).await
    }

    pub async fn delete_project(&self, name: &str) -> Result<(), String> {
        let (tx, store) = self.store(PROJECTS, IdbTransactionMode::Readwrite)?;
        store.delete(&JsValue::from_str(name)).map_err(why)?;
        committed(&tx).await?;
        let (tx, store) = self.store(THUMBNAILS, IdbTransactionMode::Readwrite)?;
        store.delete(&JsValue::from_str(name)).map_err(why)?;
        committed(&tx).await
    }

    /// The preferences, as JSON.
    pub async fn prefs(&self) -> Result<Option<String>, String> {
        let (_, store) = self.store(APP, IdbTransactionMode::Readonly)?;
        let req = store.get(&JsValue::from_str("prefs")).map_err(why)?;
        Ok(request(&req).await?.as_string())
    }

    pub async fn put_prefs(&self, json: &str) -> Result<(), String> {
        self.put(APP, "prefs", &JsValue::from_str(json)).await
    }

    pub async fn thumbnails(&self) -> Result<Vec<Thumbnail>, String> {
        Ok(self
            .all(THUMBNAILS)
            .await?
            .into_iter()
            .filter_map(|(project, v)| {
                Some(Thumbnail {
                    project,
                    width: get(&v, "width").as_f64()? as u32,
                    height: get(&v, "height").as_f64()? as u32,
                    rgba: get(&v, "rgba").dyn_into::<Uint8Array>().ok()?.to_vec(),
                })
            })
            .collect())
    }

    pub async fn put_thumbnail(&self, thumbnail: &Thumbnail) -> Result<(), String> {
        let value = object(&[
            ("width", JsValue::from(thumbnail.width)),
            ("height", JsValue::from(thumbnail.height)),
            ("rgba", Uint8Array::from(&thumbnail.rgba[..]).into()),
        ]);
        self.put(THUMBNAILS, &thumbnail.project, &value).await
    }
}

/// A project as IndexedDB keeps it: `{main, saved, names, contents}`, the files' paths and their
/// bytes in two arrays.
fn project_to(project: &Stored) -> JsValue {
    let names = Array::new();
    let contents = Array::new();
    for (path, bytes) in &project.files {
        names.push(&JsValue::from_str(path));
        contents.push(&Uint8Array::from(&bytes[..]));
    }
    object(&[
        ("main", project.main.as_deref().map_or(JsValue::NULL, JsValue::from_str)),
        ("saved", JsValue::from(project.saved as f64)),
        ("names", names.into()),
        ("contents", contents.into()),
    ])
    .into()
}

fn project_from(v: &JsValue) -> Option<Stored> {
    let names: Array = get(v, "names").dyn_into().ok()?;
    let contents: Array = get(v, "contents").dyn_into().ok()?;
    let files = names.iter().zip(contents.iter()).filter_map(|(n, c)| Some((n.as_string()?, c.dyn_into::<Uint8Array>().ok()?.to_vec()))).collect();
    Some(Stored { files, main: get(v, "main").as_string(), saved: get(v, "saved").as_f64().unwrap_or(0.0) as i64 })
}
