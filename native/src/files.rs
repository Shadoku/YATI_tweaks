//! Files the builders read on the main thread: textures (imported `res://` textures through
//! `ResourceLoader`, other images through `Image`, members of a zip archive) and resources
//! (materials, scenes, scripts), resolved as the GDScript runtime's `DataLoader` resolves them.
//!
//! Textures are cached per process, by resolved path (`zip::member` for an archive's), for every
//! `TiledTileSets` and every import: [`clear_texture_cache`] forgets them
//! (`TiledTileSets.clear_textures()`), and the extension clears the cache when the engine shuts
//! down. An asynchronous load (`TiledMap.load_async`) starts the `res://` textures of a map
//! through `ResourceLoader.load_threaded_request` while it parses ([`request_threaded`]).

use crate::diag::Diag;
use crate::gd::{cleanup_path, get_extension, path_join};
use godot::classes::{
    FileAccess, Image, ImageTexture, Object, ResourceLoader, Texture2D, ZipReader,
};
use godot::global::Error as GdError;
use godot::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// The per-process texture cache (main thread only: textures are engine objects).
    static TEXTURES: RefCell<HashMap<String, Gd<Texture2D>>> = RefCell::new(HashMap::new());
    /// Threaded texture requests whose map was cancelled or dropped: each must still be
    /// collected with `load_threaded_get` (or the loader keeps the texture), once it is done.
    static ORPHANS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn cached(key: &str) -> Option<Gd<Texture2D>> {
    TEXTURES.with(|t| t.borrow().get(key).cloned())
}

fn cache(key: String, texture: &Gd<Texture2D>) {
    TEXTURES.with(|t| t.borrow_mut().insert(key, texture.clone()));
}

/// Forgets every cached texture (they stay alive while a TileSet or node uses them).
pub fn clear_texture_cache() {
    // Taken out first: dropping a texture may run engine code that reaches the cache.
    let old = TEXTURES.with(|t| std::mem::take(&mut *t.borrow_mut()));
    drop(old);
}

/// The number of cached textures.
pub fn texture_cache_len() -> usize {
    TEXTURES.with(|t| t.borrow().len())
}

/// `ResourceLoader.ThreadLoadStatus`: `THREAD_LOAD_IN_PROGRESS`.
pub const THREAD_LOAD_IN_PROGRESS: i64 = 1;

// godot-rust leaves ResourceLoader's three threaded-load methods out of its API unless the
// `experimental-threads` feature is on (they are its only users of Godot's loader threads). They
// are called here on the main thread like any other method, through the engine's dynamic call;
// the loading threads are the engine's own and run no code of this crate (only images are
// requested), so the feature is not needed.
fn resource_loader() -> Gd<Object> {
    ResourceLoader::singleton().upcast::<Object>()
}

/// Whether `path` is an imported image the engine can load in its threads (a `res://` path
/// `ResourceLoader` knows as an image: what the synchronous path loads with `ResourceLoader`).
pub fn threaded_loadable(path: &str) -> bool {
    path.starts_with("res://")
        && ResourceLoader::singleton()
            .exists_ex(path)
            .type_hint("Image")
            .done()
}

/// `ResourceLoader.load_threaded_request(path)`; true when the request was made.
pub fn request_threaded(path: &str) -> bool {
    let r = resource_loader().call("load_threaded_request", &[path.to_variant()]);
    r.try_to::<i64>().unwrap_or(-1) == GdError::OK.ord() as i64
}

/// `ResourceLoader.load_threaded_get_status(path)` as its enum's value.
pub fn threaded_status(path: &str) -> i64 {
    resource_loader()
        .call("load_threaded_get_status", &[path.to_variant()])
        .try_to::<i64>()
        .unwrap_or(0)
}

/// `ResourceLoader.load_threaded_get(path)`: the loaded resource (waits if it is still loading).
pub fn threaded_get(path: &str) -> Option<Gd<Resource>> {
    resource_loader()
        .call("load_threaded_get", &[path.to_variant()])
        .try_to::<Gd<Resource>>()
        .ok()
}

/// Hands threaded requests nobody will collect to the per-frame drain ([`drain_orphans`]).
pub fn orphan_requests(paths: impl IntoIterator<Item = String>) {
    ORPHANS.with(|o| o.borrow_mut().extend(paths));
}

/// Collects (and drops) the orphaned threaded requests that are done; with `wait`, all of them,
/// waiting for those still loading. Called every frame and at shutdown.
pub fn drain_orphans(wait: bool) {
    let pending = ORPHANS.with(|o| std::mem::take(&mut *o.borrow_mut()));
    if pending.is_empty() {
        return;
    }
    let mut left = Vec::new();
    for p in pending {
        if wait || threaded_status(&p) != THREAD_LOAD_IN_PROGRESS {
            drop(threaded_get(&p));
        } else {
            left.push(p);
        }
    }
    ORPHANS.with(|o| o.borrow_mut().extend(left));
}

/// The number of orphaned threaded requests not collected yet.
pub fn orphan_count() -> usize {
    ORPHANS.with(|o| o.borrow().len())
}

/// `DataLoader._resolve_path_in_file_system`: the name as given, else relative to `base`.
pub fn resolve_in_fs(file: &str, base: &str) -> Option<String> {
    if FileAccess::file_exists(file) {
        return Some(file.to_string());
    }
    let joined = path_join(base, file);
    FileAccess::file_exists(joined.as_str()).then_some(joined)
}

/// `DataLoader.load_resource_from_file`.
pub fn load_resource(file: &str, base: &str, diag: &mut Diag) -> Option<Gd<Resource>> {
    match resolve_in_fs(file, base) {
        Some(p) => ResourceLoader::singleton().load(p.as_str()),
        None => {
            diag.error(format!("ERROR: Resource file '{file}' not found."));
            None
        }
    }
}

/// The texture loader of the imports one `TiledTileSets` serves: textures come from the
/// per-process cache; this holds the zip archive an import reads from.
#[derive(Default)]
pub struct Textures {
    zip: Option<(String, Gd<ZipReader>)>,
}

impl Textures {
    /// Closes a zip archive (the textures stay in the per-process cache).
    pub fn clear(&mut self) {
        self.close_zip();
    }

    /// Closes the zip archive opened for an import.
    pub fn close_zip(&mut self) {
        if let Some((_, mut r)) = self.zip.take() {
            r.close();
        }
    }

    /// `DataLoader.load_image`: the image `file` referenced from a file in `base`, from the file
    /// system or (when `zip` is not empty) from that archive.
    pub fn load(
        &mut self,
        file: &str,
        base: &str,
        zip: &str,
        diag: &mut Diag,
    ) -> Option<Gd<Texture2D>> {
        if zip.is_empty() {
            self.load_from_fs(file, base, diag)
        } else {
            self.load_from_zip(file, base, zip, diag)
        }
    }

    fn load_from_fs(&mut self, file: &str, base: &str, diag: &mut Diag) -> Option<Gd<Texture2D>> {
        let Some(checked) = resolve_in_fs(file, base) else {
            diag.error(format!("ERROR: Image file '{file}' not found."));
            return None;
        };
        if let Some(t) = cached(&checked) {
            return Some(t);
        }
        let loader = ResourceLoader::singleton();
        let texture: Gd<Texture2D> = if loader
            .clone()
            .exists_ex(checked.as_str())
            .type_hint("Image")
            .done()
        {
            let res = ResourceLoader::singleton().load(checked.as_str())?;
            match res.try_cast::<Texture2D>() {
                Ok(t) => t,
                Err(_) => {
                    diag.error(format!(
                        "ERROR: Image file '{checked}' could not be loaded."
                    ));
                    return None;
                }
            }
        } else {
            let Some(image) = Image::load_from_file(checked.as_str()) else {
                diag.error(format!(
                    "ERROR: Image file '{checked}' could not be loaded."
                ));
                return None;
            };
            ImageTexture::create_from_image(&image)?.upcast()
        };
        cache(checked, &texture);
        Some(texture)
    }

    fn reader(&mut self, zip: &str, diag: &mut Diag) -> Option<Gd<ZipReader>> {
        if let Some((p, r)) = &self.zip {
            if p == zip {
                return Some(r.clone());
            }
        }
        self.close_zip();
        let mut r = ZipReader::new_gd();
        if r.open(zip) != GdError::OK {
            diag.error(format!("ERROR: Unable to open zip file '{zip}'."));
            return None;
        }
        self.zip = Some((zip.to_string(), r.clone()));
        Some(r)
    }

    fn load_from_zip(
        &mut self,
        file: &str,
        base: &str,
        zip: &str,
        diag: &mut Diag,
    ) -> Option<Gd<Texture2D>> {
        let mut reader = self.reader(zip, diag)?;
        let member = if reader.file_exists(file) {
            file.to_string()
        } else {
            let p = cleanup_path(&path_join(base, file));
            if reader.file_exists(p.as_str()) {
                p
            } else {
                diag.error(format!(
                    "ERROR: Image file '{file}' not found in zip file '{zip}'."
                ));
                return None;
            }
        };
        let key = format!("{zip}::{member}");
        if let Some(t) = cached(&key) {
            return Some(t);
        }
        let bytes = reader.read_file(member.as_str());
        let mut image = Image::new_gd();
        let err = match get_extension(file).to_lowercase().as_str() {
            "png" => image.load_png_from_buffer(&bytes),
            "jpg" | "jpeg" => image.load_jpg_from_buffer(&bytes),
            "bmp" => image.load_bmp_from_buffer(&bytes),
            "webp" => image.load_webp_from_buffer(&bytes),
            "tga" => image.load_tga_from_buffer(&bytes),
            _ => GdError::ERR_FILE_UNRECOGNIZED,
        };
        if err != GdError::OK {
            diag.error(format!(
                "ERROR: Image file '{member}' in zip file '{zip}' could not be loaded."
            ));
            return None;
        }
        let texture: Gd<Texture2D> = ImageTexture::create_from_image(&image)?.upcast();
        cache(key, &texture);
        Some(texture)
    }
}
