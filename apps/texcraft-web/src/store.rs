//! What the browser keeps between visits, in IndexedDB (database `texcraft`): the projects of the
//! session's library (store `projects`, by name), their review threads (store `reviews`, by
//! `<project>/<thread file>`, each the text a project folder keeps in `.texcraft/review/`), the
//! preferences (store `app`, key `prefs`, as JSON) and the recent projects' thumbnails (store
//! `thumbnails`, by project), and the projects' histories: their versions (store `versions`, by
//! `<project>/<version id>`) and the bytes of the versions' files (store `blobs`, by
//! `<project>/<blob id>`, each kept once however many versions have it).
//!
//! The threads have their own store, not a field of the project's value, so a comment stores one
//! small value at once instead of rewriting the project's files. Version 1 had no `reviews`
//! store: opening it makes the store, and its projects have no threads. Version 2 had no
//! `versions` and `blobs` stores: opening it makes them, and its projects' histories start empty.
//!
//! IndexedDB's requests call back; each is wrapped in a Promise here and awaited. Writes are
//! awaited to their transaction's end, so an error reaches the user.

use std::collections::BTreeMap;

use js_sys::{Array, Object, Promise, Reflect, Uint8Array};
use texcraft_engine::history::store::{Version, Versions};
use texcraft_engine::library::Stored;
use texcraft_ui_egui::Thumbnail;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{IdbDatabase, IdbKeyRange, IdbObjectStore, IdbOpenDbRequest, IdbRequest, IdbTransaction, IdbTransactionMode};

const NAME: &str = "texcraft";
const VERSION: u32 = 3;
const PROJECTS: &str = "projects";
const REVIEWS: &str = "reviews";
const APP: &str = "app";
const THUMBNAILS: &str = "thumbnails";
const VERSIONS: &str = "versions";
const BLOBS: &str = "blobs";

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
            for store in [PROJECTS, REVIEWS, APP, THUMBNAILS, VERSIONS, BLOBS] {
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

    /// The projects, each with its review threads and its history.
    pub async fn projects(&self) -> Result<Vec<(String, Stored)>, String> {
        let mut reviews: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        for (key, text) in self.all(REVIEWS).await? {
            if let (Some((project, file)), Some(text)) = (key.split_once('/'), text.as_string()) {
                reviews.entry(project.to_owned()).or_default().insert(file.to_owned(), text);
            }
        }
        let mut versions: BTreeMap<String, Vec<Version>> = BTreeMap::new();
        for (key, v) in self.all(VERSIONS).await? {
            if let Some((project, id)) = key.split_once('/')
                && let Some(version) = version_from(id, &v)
            {
                versions.entry(project.to_owned()).or_default().push(version);
            }
        }
        let mut blobs: BTreeMap<String, BTreeMap<String, Vec<u8>>> = BTreeMap::new();
        for (key, v) in self.all(BLOBS).await? {
            if let (Some((project, id)), Ok(bytes)) = (key.split_once('/'), v.dyn_into::<Uint8Array>()) {
                blobs.entry(project.to_owned()).or_default().insert(id.to_owned(), bytes.to_vec());
            }
        }
        Ok(self
            .all(PROJECTS)
            .await?
            .into_iter()
            .filter_map(|(name, v)| {
                let mut project = project_from(&v)?;
                project.review = reviews.remove(&name).unwrap_or_default();
                project.history = Versions::from_parts(versions.remove(&name).unwrap_or_default(), blobs.remove(&name).unwrap_or_default());
                Some((name, project))
            })
            .collect())
    }

    /// Adds `versions` and the files' bytes `blobs` (by blob id) to project `name`'s history, in
    /// one transaction.
    pub async fn put_versions(&self, name: &str, versions: &[Version], blobs: &[(String, Vec<u8>)]) -> Result<(), String> {
        let names = Array::of2(&JsValue::from_str(VERSIONS), &JsValue::from_str(BLOBS));
        let tx = self.db.transaction_with_str_sequence_and_mode(&names, IdbTransactionMode::Readwrite).map_err(why)?;
        let version_store = tx.object_store(VERSIONS).map_err(why)?;
        let blob_store = tx.object_store(BLOBS).map_err(why)?;
        for (id, bytes) in blobs {
            blob_store.put_with_key(&Uint8Array::from(&bytes[..]), &JsValue::from_str(&thread_key(name, id))).map_err(why)?;
        }
        for version in versions {
            version_store.put_with_key(&version_to(version), &JsValue::from_str(&thread_key(name, &version.id))).map_err(why)?;
        }
        committed(&tx).await
    }

    /// Both stores of a project, for one transaction.
    fn project_stores(&self) -> Result<(IdbTransaction, IdbObjectStore, IdbObjectStore), String> {
        let names = Array::of2(&JsValue::from_str(PROJECTS), &JsValue::from_str(REVIEWS));
        let tx = self.db.transaction_with_str_sequence_and_mode(&names, IdbTransactionMode::Readwrite).map_err(why)?;
        let projects = tx.object_store(PROJECTS).map_err(why)?;
        let reviews = tx.object_store(REVIEWS).map_err(why)?;
        Ok((tx, projects, reviews))
    }

    /// Replaces the project and all its threads, in one transaction.
    pub async fn put_project(&self, name: &str, project: &Stored) -> Result<(), String> {
        let (tx, projects, reviews) = self.project_stores()?;
        projects.put_with_key(&project_to(project), &JsValue::from_str(name)).map_err(why)?;
        let threads: JsValue = threads_of(name)?.into();
        reviews.delete(&threads).map_err(why)?;
        for (file, text) in &project.review {
            reviews.put_with_key(&JsValue::from_str(text), &JsValue::from_str(&thread_key(name, file))).map_err(why)?;
        }
        committed(&tx).await
    }

    /// Stores thread `file` of project `name` with `text`, or deletes it (`None`).
    pub async fn put_thread(&self, name: &str, file: &str, text: Option<&str>) -> Result<(), String> {
        let (tx, store) = self.store(REVIEWS, IdbTransactionMode::Readwrite)?;
        let key = JsValue::from_str(&thread_key(name, file));
        match text {
            Some(text) => store.put_with_key(&JsValue::from_str(text), &key).map(drop),
            None => store.delete(&key).map(drop),
        }
        .map_err(why)?;
        committed(&tx).await
    }

    /// Deletes the project, its threads and its history.
    pub async fn delete_project(&self, name: &str) -> Result<(), String> {
        let names: Array = [PROJECTS, REVIEWS, VERSIONS, BLOBS].iter().map(|n| JsValue::from_str(n)).collect();
        let tx = self.db.transaction_with_str_sequence_and_mode(&names, IdbTransactionMode::Readwrite).map_err(why)?;
        tx.object_store(PROJECTS).map_err(why)?.delete(&JsValue::from_str(name)).map_err(why)?;
        let keys: JsValue = threads_of(name)?.into();
        for store in [REVIEWS, VERSIONS, BLOBS] {
            tx.object_store(store).map_err(why)?.delete(&keys).map_err(why)?;
        }
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

/// The key of thread `file` of project `name` in the `reviews` store (and of a version or a blob
/// in theirs); a project's name has no `/` (`library::check_name`).
fn thread_key(name: &str, file: &str) -> String {
    format!("{name}/{file}")
}

/// Every key of project `name`'s threads (or versions, or blobs): from `<name>/` up to `<name>0` (`0` follows `/`).
fn threads_of(name: &str) -> Result<IdbKeyRange, String> {
    IdbKeyRange::bound_with_lower_open_and_upper_open(&JsValue::from_str(&format!("{name}/")), &JsValue::from_str(&format!("{name}0")), false, true)
        .map_err(why)
}

/// A project as IndexedDB keeps it: `{main, saved, names, contents, forge}`, the files' paths and
/// their bytes in two arrays, and the GitLab branch it is linked to as JSON (no token). Its
/// threads are in the `reviews` store.
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
        ("forge", project.forge.as_ref().and_then(|l| serde_json::to_string(l).ok()).map_or(JsValue::NULL, |j| JsValue::from_str(&j))),
    ])
    .into()
}

fn project_from(v: &JsValue) -> Option<Stored> {
    let names: Array = get(v, "names").dyn_into().ok()?;
    let contents: Array = get(v, "contents").dyn_into().ok()?;
    let files = names.iter().zip(contents.iter()).filter_map(|(n, c)| Some((n.as_string()?, c.dyn_into::<Uint8Array>().ok()?.to_vec()))).collect();
    Some(Stored {
        files,
        main: get(v, "main").as_string(),
        saved: get(v, "saved").as_f64().unwrap_or(0.0) as i64,
        review: BTreeMap::new(),
        history: Versions::default(),
        forge: get(v, "forge").as_string().and_then(|j| serde_json::from_str(&j).ok()),
    })
}

/// A version as IndexedDB keeps it: `{parent, time, author, message, paths, blobs}`, its files'
/// paths and blob ids in two arrays. Its id is its key.
fn version_to(version: &Version) -> JsValue {
    let paths = Array::new();
    let blobs = Array::new();
    for (path, blob) in &version.files {
        paths.push(&JsValue::from_str(path));
        blobs.push(&JsValue::from_str(blob));
    }
    object(&[
        ("parent", version.parent.as_deref().map_or(JsValue::NULL, JsValue::from_str)),
        ("time", JsValue::from(version.time as f64)),
        ("author", JsValue::from_str(&version.author)),
        ("message", JsValue::from_str(&version.message)),
        ("paths", paths.into()),
        ("blobs", blobs.into()),
    ])
    .into()
}

fn version_from(id: &str, v: &JsValue) -> Option<Version> {
    let paths: Array = get(v, "paths").dyn_into().ok()?;
    let blobs: Array = get(v, "blobs").dyn_into().ok()?;
    let files = paths.iter().zip(blobs.iter()).filter_map(|(p, b)| Some((p.as_string()?, b.as_string()?))).collect();
    Some(Version {
        id: id.to_owned(),
        parent: get(v, "parent").as_string(),
        time: get(v, "time").as_f64()? as i64,
        author: get(v, "author").as_string().unwrap_or_default(),
        message: get(v, "message").as_string().unwrap_or_default(),
        files,
    })
}
