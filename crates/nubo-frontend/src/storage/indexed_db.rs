//! The browser's IndexedDB, where the browser build keeps what doesn't fit
//! in localStorage's few megabytes: save states, the game library and its
//! pictures, and the ROMs of the games the player keeps in the browser.
//!
//! The database is called `nubo` and has two object stores:
//!
//! - `files` holds entries named like the desktop's files in its data folder,
//!   such as `library.toml`, `states/Tetris.gb/quick.state` and
//!   `thumbnails/Tetris.gb.png`. Each value is an object with the entry's
//!   bytes (`data`, a `Uint8Array`) and when it was written (`modified`, in
//!   milliseconds since 1970).
//! - `roms` holds each kept game's file, keyed by its file name (the path the
//!   browser build knows the game by), as a `Uint8Array`.
//!
//! IndexedDB only answers asynchronously, but the code that reads saves and
//! pictures is synchronous, as it is with files on the desktop. So [`open`]
//! reads all of `files` into memory once, when the app starts; reads come
//! from memory, and writes change memory at once and reach the database in
//! the background, in the order they were made. A write the database
//! refuses (it is full, say) is collected for the app to show ([`take_errors`]).
//! ROMs are too big to hold them all, so they are read when a game starts
//! ([`read_rom`]).
//!
//! Without a browser (the tests, and builds with the `web` feature for
//! another target) the entries live in memory only, for the current thread,
//! so each test starts empty.

use crate::time::SystemTime;
use std::cell::RefCell;
use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;
use std::sync::Arc;

/// An entry in `files`: its bytes and when they were written.
#[derive(Debug, Clone)]
pub struct Record {
    /// The entry's contents.
    pub bytes: Arc<[u8]>,
    /// When the entry was last written.
    pub modified: SystemTime,
}

/// The name of the database, in the page's origin.
#[cfg(any(target_arch = "wasm32", test))]
const DATABASE: &str = "nubo";
/// The object store mirrored in memory.
#[cfg(any(target_arch = "wasm32", test))]
const FILES: &str = "files";
/// The object store of ROM files, read when a game starts.
#[cfg(any(target_arch = "wasm32", test))]
const ROMS: &str = "roms";

thread_local! {
    /// What `files` holds, all of it, once [`open`] read it.
    static FILES_IN_MEMORY: RefCell<BTreeMap<String, Record>> = const { RefCell::new(BTreeMap::new()) };
    /// Without a browser, the ROMs too.
    #[cfg(not(target_arch = "wasm32"))]
    static ROMS_IN_MEMORY: RefCell<HashMap<String, Arc<[u8]>>> = RefCell::new(HashMap::new());
    /// Writes the database refused, waiting for the app to show them.
    static ERRORS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// The open database, in the browser.
    #[cfg(target_arch = "wasm32")]
    static DATABASE_HANDLE: RefCell<Option<web_sys::IdbDatabase>> = const { RefCell::new(None) };
}

/// Opens the database and reads every entry of `files` into memory. The
/// browser build calls this once when it starts, before it reads the
/// library; until then (and if it fails) entries read as missing and writes
/// fail. Without a browser there is nothing to open.
pub async fn open() -> Result<(), String> {
    #[cfg(target_arch = "wasm32")]
    {
        let (database, records) = browser::open()
            .await
            .map_err(|error| format!("The browser's storage couldn't be opened: {error}"))?;
        FILES_IN_MEMORY.with_borrow_mut(|files| files.extend(records));
        DATABASE_HANDLE.set(Some(database));
    }
    Ok(())
}

/// Whether entries can be written: the database is open, or there is no
/// browser.
pub fn is_open() -> bool {
    #[cfg(target_arch = "wasm32")]
    return DATABASE_HANDLE.with_borrow(Option::is_some);
    #[cfg(not(target_arch = "wasm32"))]
    true
}

/// The entry called `key`, if there is one.
pub fn read(key: &str) -> Option<Record> {
    FILES_IN_MEMORY.with_borrow(|files| files.get(key).cloned())
}

/// Replaces the entry called `key` with `bytes`, written now.
pub fn write(key: &str, bytes: &[u8]) -> Result<(), String> {
    check_open()?;
    let record = Record {
        bytes: Arc::from(bytes),
        modified: SystemTime::now(),
    };
    #[cfg(target_arch = "wasm32")]
    browser::put(key, &record);
    FILES_IN_MEMORY.with_borrow_mut(|files| files.insert(key.to_owned(), record));
    Ok(())
}

/// Deletes the entry called `key`. A missing entry is fine.
pub fn remove(key: &str) -> Result<(), String> {
    check_open()?;
    if FILES_IN_MEMORY
        .with_borrow_mut(|files| files.remove(key))
        .is_some()
    {
        #[cfg(target_arch = "wasm32")]
        browser::delete(FILES, key);
    }
    Ok(())
}

/// Deletes every entry whose name starts with `prefix`.
pub fn remove_all(prefix: &str) -> Result<(), String> {
    for key in keys(prefix) {
        remove(&key)?;
    }
    Ok(())
}

/// Gives the entry called `from` the name `to`, replacing any entry called
/// that. Returns whether there was an entry to rename.
pub fn rename(from: &str, to: &str) -> Result<bool, String> {
    check_open()?;
    let Some(record) = FILES_IN_MEMORY.with_borrow_mut(|files| files.remove(from)) else {
        return Ok(false);
    };
    #[cfg(target_arch = "wasm32")]
    {
        browser::put(to, &record);
        browser::delete(FILES, from);
    }
    FILES_IN_MEMORY.with_borrow_mut(|files| files.insert(to.to_owned(), record));
    Ok(true)
}

/// The names of the entries that start with `prefix`, in order.
pub fn keys(prefix: &str) -> Vec<String> {
    FILES_IN_MEMORY.with_borrow(|files| {
        files
            .range(prefix.to_owned()..)
            .take_while(|(key, _)| key.starts_with(prefix))
            .map(|(key, _)| key.clone())
            .collect()
    })
}

/// How many bytes the entries starting with `prefix` hold.
pub fn size(prefix: &str) -> u64 {
    FILES_IN_MEMORY.with_borrow(|files| {
        files
            .range(prefix.to_owned()..)
            .take_while(|(key, _)| key.starts_with(prefix))
            .map(|(_, record)| record.bytes.len() as u64)
            .sum()
    })
}

/// Reads the ROM file kept as `name`, or `None` if none is.
pub async fn read_rom(name: &str) -> Result<Option<Arc<[u8]>>, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let database = DATABASE_HANDLE
            .with_borrow(Clone::clone)
            .ok_or_else(not_open)?;
        browser::read_rom(&database, name).await
    }
    #[cfg(not(target_arch = "wasm32"))]
    Ok(ROMS_IN_MEMORY.with_borrow(|roms| roms.get(name).cloned()))
}

/// Keeps `bytes` as the ROM file called `name`, replacing any kept before.
pub fn write_rom(name: &str, bytes: &[u8]) -> Result<(), String> {
    check_open()?;
    #[cfg(target_arch = "wasm32")]
    browser::put_rom(name, bytes);
    #[cfg(not(target_arch = "wasm32"))]
    ROMS_IN_MEMORY.with_borrow_mut(|roms| roms.insert(name.to_owned(), Arc::from(bytes)));
    Ok(())
}

/// Deletes the ROM file called `name`. A missing one is fine.
pub fn remove_rom(name: &str) -> Result<(), String> {
    check_open()?;
    #[cfg(target_arch = "wasm32")]
    browser::delete(ROMS, name);
    #[cfg(not(target_arch = "wasm32"))]
    ROMS_IN_MEMORY.with_borrow_mut(|roms| roms.remove(name));
    Ok(())
}

/// Removes and returns the writes the database refused since the last call,
/// each as a message for the player.
pub fn take_errors() -> Vec<String> {
    ERRORS.with_borrow_mut(std::mem::take)
}

/// Collects a write the database refused, for [`take_errors`].
#[cfg(target_arch = "wasm32")]
fn report(error: String) {
    ERRORS.with_borrow_mut(|errors| errors.push(error));
}

/// Fails unless entries can be written (see [`is_open`]).
fn check_open() -> Result<(), String> {
    if is_open() { Ok(()) } else { Err(not_open()) }
}

/// Why nothing can be kept: the database isn't open.
fn not_open() -> String {
    String::from("the browser's storage isn't available")
}

/// Reaching IndexedDB through `web-sys`, which only works in the browser.
#[cfg(target_arch = "wasm32")]
mod browser {
    use super::{DATABASE, FILES, ROMS, Record};
    use crate::time::{Duration, SystemTime};
    use js_sys::{Array, Object, Reflect, Uint8Array};
    use std::sync::Arc;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{IdbDatabase, IdbObjectStore, IdbOpenDbRequest, IdbRequest, IdbTransactionMode};

    /// The database's version. A newer one that adds an object store makes
    /// it in `upgradeneeded`, as `create_stores` does.
    const VERSION: u32 = 1;

    /// Opens the database, making its object stores the first time, and
    /// reads every entry of `files`.
    pub async fn open() -> Result<(IdbDatabase, Vec<(String, Record)>), String> {
        let factory = web_sys::window()
            .ok_or("there is no window")?
            .indexed_db()
            .map_err(describe)?
            .ok_or("this browser has no IndexedDB")?;
        let request = factory.open_with_u32(DATABASE, VERSION).map_err(describe)?;

        let upgrade = Closure::<dyn FnMut(web_sys::Event)>::new({
            let request = request.clone();
            move |_event| create_stores(&request)
        });
        request.set_onupgradeneeded(Some(upgrade.as_ref().unchecked_ref()));
        let database: IdbDatabase = finished(&request).await?.unchecked_into();
        request.set_onupgradeneeded(None);
        drop(upgrade);

        let store = store(&database, FILES, IdbTransactionMode::Readonly)?;
        let keys = store.get_all_keys().map_err(describe)?;
        let values = store.get_all().map_err(describe)?;
        let keys: Array = finished(&keys).await?.unchecked_into();
        let values: Array = finished(&values).await?.unchecked_into();

        let records = keys
            .iter()
            .zip(values.iter())
            .filter_map(|(key, value)| Some((key.as_string()?, record(&value)?)))
            .collect();
        Ok((database, records))
    }

    /// Makes the object stores the database doesn't have yet.
    fn create_stores(request: &IdbOpenDbRequest) {
        let Ok(database) = request.result() else {
            return;
        };
        let database: IdbDatabase = database.unchecked_into();
        let names = database.object_store_names();
        for name in [FILES, ROMS] {
            if !names.contains(name) {
                let _ = database.create_object_store(name);
            }
        }
    }

    /// Reads the ROM kept as `name` from `roms`.
    pub async fn read_rom(database: &IdbDatabase, name: &str) -> Result<Option<Arc<[u8]>>, String> {
        let request = store(database, ROMS, IdbTransactionMode::Readonly)?
            .get(&JsValue::from_str(name))
            .map_err(describe)?;
        let value = finished(&request).await?;
        Ok(value
            .dyn_ref::<Uint8Array>()
            .map(|bytes| Arc::from(bytes.to_vec())))
    }

    /// Writes `record` as `key` in `files`, in the background.
    pub fn put(key: &str, record: &Record) {
        let value = Object::new();
        let milliseconds = record
            .modified
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0.0, |duration| duration.as_secs_f64() * 1000.0);
        let _ = Reflect::set(
            &value,
            &JsValue::from_str("data"),
            &Uint8Array::from(&record.bytes[..]),
        );
        let _ = Reflect::set(
            &value,
            &JsValue::from_str("modified"),
            &JsValue::from_f64(milliseconds),
        );
        in_background(FILES, key, |store| {
            store.put_with_key(&value, &JsValue::from_str(key))
        });
    }

    /// Writes the ROM `bytes` as `name` in `roms`, in the background.
    pub fn put_rom(name: &str, bytes: &[u8]) {
        let value = Uint8Array::from(bytes);
        in_background(ROMS, name, |store| {
            store.put_with_key(&value, &JsValue::from_str(name))
        });
    }

    /// Deletes `key` from the object store `store_name`, in the background.
    pub fn delete(store_name: &'static str, key: &str) {
        in_background(store_name, key, |store| {
            store.delete(&JsValue::from_str(key))
        });
    }

    /// Starts `change` on the object store `store_name` in a transaction of
    /// its own, and collects its error for the app if it fails. Transactions
    /// on the same store run in the order they were made, so the database
    /// ends up as memory is.
    fn in_background(
        store_name: &'static str,
        key: &str,
        change: impl FnOnce(&IdbObjectStore) -> Result<IdbRequest, JsValue>,
    ) {
        let Some(database) = super::DATABASE_HANDLE.with_borrow(Clone::clone) else {
            return;
        };
        let request = store(&database, store_name, IdbTransactionMode::Readwrite)
            .and_then(|store| change(&store).map_err(describe));
        let key = key.to_owned();
        wasm_bindgen_futures::spawn_local(async move {
            let result = match request {
                Ok(request) => finished(&request).await.map(drop),
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                super::report(format!("{key} couldn't be kept in the browser: {error}"));
            }
        });
    }

    /// The object store `name`, in a new transaction.
    fn store(
        database: &IdbDatabase,
        name: &str,
        mode: IdbTransactionMode,
    ) -> Result<IdbObjectStore, String> {
        database
            .transaction_with_str_and_mode(name, mode)
            .and_then(|transaction| transaction.object_store(name))
            .map_err(describe)
    }

    /// Waits for `request` to finish, and returns its result or why it
    /// failed.
    async fn finished(request: &IdbRequest) -> Result<JsValue, String> {
        let promise = js_sys::Promise::new(&mut |resolve, reject| {
            request.set_onsuccess(Some(&resolve));
            request.set_onerror(Some(&reject));
        });
        let outcome = JsFuture::from(promise).await;
        request.set_onsuccess(None);
        request.set_onerror(None);
        match outcome {
            Ok(_) => request.result().map_err(describe),
            Err(_) => Err(request
                .error()
                .ok()
                .flatten()
                .map_or_else(|| String::from("it failed"), |error| error.message())),
        }
    }

    /// The record a value of `files` holds, or `None` if it isn't one.
    fn record(value: &JsValue) -> Option<Record> {
        let data = Reflect::get(value, &JsValue::from_str("data")).ok()?;
        let modified = Reflect::get(value, &JsValue::from_str("modified"))
            .ok()?
            .as_f64()?;
        Some(Record {
            bytes: Arc::from(data.dyn_ref::<Uint8Array>()?.to_vec()),
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs_f64(modified.max(0.0) / 1000.0),
        })
    }

    /// A JavaScript error as a message.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "`map_err` hands over the error"
    )]
    fn describe(error: JsValue) -> String {
        error
            .dyn_ref::<js_sys::Error>()
            .map(|error| String::from(error.message()))
            .or_else(|| {
                error
                    .dyn_ref::<web_sys::DomException>()
                    .map(web_sys::DomException::message)
            })
            .unwrap_or_else(|| format!("{error:?}"))
    }
}

#[cfg(test)]
mod tests;
