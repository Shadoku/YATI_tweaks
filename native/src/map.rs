//! `TiledMap`: a parsed map, loaded through the core's file-request protocol with the files
//! read on the main thread through Godot (`FileAccess`, so `res://` inside a .pck works).
//!
//! One state machine serves both ways of loading. `load_async` / `from_zip_async` run the
//! core's [`Loader`] on a worker thread ([`tiled_core::worker`]: only `tiled_core` and `std`
//! types cross it) and `poll()` advances the protocol one round per call on the main thread:
//! it reads the files the worker asks for and hands them over, starts the map's `res://`
//! textures through `ResourceLoader.load_threaded_request` as soon as the parse names them, and
//! collects them when they are loaded; `loaded` (or `failed`) is emitted from the `poll()` that
//! finishes. `load` / `from_zip` pump the same machine to the end in one call, with the loader on
//! the main thread and no texture requests (the builder loads them, as before).
//!
//! With `prepare_tile_layers` on, the load also prepares the map's tile layers for a build
//! ([`PreparedCells`]: payloads decoded into blobs that wait only for the TileSet's tile fields),
//! on the worker for an asynchronous load, so `TiledBuilder.build` does not decode them in its
//! frame.

use crate::files::{
    orphan_count, orphan_requests, request_threaded, threaded_get, threaded_loadable,
    threaded_status, THREAD_LOAD_IN_PROGRESS,
};
use crate::gd::value_to_variant;
use godot::classes::FileAccess;
use godot::global::{print_rich, printerr, Error as GdError};
use godot::prelude::*;
use std::collections::HashSet;
use std::sync::Arc;
use tiled_core::prepare::PreparedCells;
use tiled_core::worker::{Event, File, Worker};
use tiled_core::{Loader, Map, Status as LoadStatus};

/// Where a load stands (`poll()`'s value; the class constants `READING` ... `FAILED`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    /// Files are to be read: the next `poll()` reads them.
    Reading,
    /// The worker is parsing.
    Parsing,
    /// The map is parsed; textures are loading in the engine's threads.
    WaitingFiles,
    /// The map is loaded.
    Ready,
    /// The map could not be loaded, the load was cancelled, or none was started.
    #[default]
    Failed,
}

impl State {
    fn code(self) -> i32 {
        match self {
            State::Reading => TiledMap::READING,
            State::Parsing => TiledMap::PARSING,
            State::WaitingFiles => TiledMap::WAITING_FILES,
            State::Ready => TiledMap::READY,
            State::Failed => TiledMap::FAILED,
        }
    }
}

/// Who runs the core's loader.
enum Driver {
    /// `from_zip*` before the archive's bytes are read: the loader needs them to exist.
    Zip {
        zip: String,
        path_in_zip: String,
        project: Option<String>,
    },
    /// The loader on the main thread (a synchronous load, or an asynchronous one before its
    /// first files are read).
    Main(Box<Loader>),
    /// The loader on its worker thread.
    Worker(Worker),
    /// The loader is done.
    Done,
}

/// One load in progress.
struct Job {
    threaded: bool,
    /// Prepare the tile layers (`prepare_tile_layers` when the load started).
    prepare: bool,
    driver: Driver,
    /// The files the next round reads.
    to_read: Vec<String>,
    /// Threaded texture requests not collected yet.
    requested: Vec<String>,
    /// Every image path considered for a request.
    seen: HashSet<String>,
}

impl Drop for Job {
    fn drop(&mut self) {
        // Requests nobody collects keep their texture in the engine's loader: the per-frame
        // drain collects them once they are done (`files::drain_orphans`). Dropping the worker
        // ends its thread without waiting.
        orphan_requests(std::mem::take(&mut self.requested));
    }
}

/// A Tiled map (`.tmx` / `.tmj`) with its external tilesets, templates and custom types,
/// parsed by the native core. `load()` or `from_zip()` it, or `load_async()` /
/// `from_zip_async()` and `poll()` it once per frame until `READY` (or `await` its `loaded`
/// signal while something polls it); then hand it to `TiledTileSets.tileset_for` and
/// `TiledBuilder.build`, or read it with the queries.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct TiledMap {
    base: Base<RefCounted>,
    pub(crate) map: Option<Arc<Map>>,
    /// The zip archive the map was read from ("" for the file system).
    pub(crate) zip_file: String,
    errors: Vec<String>,
    warnings: Vec<String>,
    state: State,
    job: Option<Job>,
    /// The textures an asynchronous load collected (kept alive, so the build finds them in the
    /// engine's resource cache).
    textures: Vec<Gd<Resource>>,
    /// Loads also prepare the map's tile layers for `TiledBuilder.build` (decoded into blobs
    /// that wait only for the TileSet's tile fields; on the worker thread for `load_async` and
    /// `from_zip_async`), so the build's frame does not decode them. The map then holds them,
    /// about 12 bytes per non-empty cell, until it is loaded again or freed. Off by default: a
    /// map a `TiledStreamer` reads needs none (it decodes the chunks it shows), and an infinite
    /// map prepared whole costs 12 bytes per cell of every chunk. `Importer.import_async` turns
    /// it on. Read when a load starts.
    #[var]
    prepare_tile_layers: bool,
    /// The prepared tile layers (`prepare_tile_layers`).
    pub(crate) cells: Option<Arc<PreparedCells>>,
}

#[godot_api]
impl TiledMap {
    /// `poll()`: the main thread reads files for the parse on the next `poll()`.
    #[constant]
    pub const READING: i32 = 0;
    /// `poll()`: the worker thread is parsing.
    #[constant]
    pub const PARSING: i32 = 1;
    /// `poll()`: parsed; the map's textures are loading in the engine's threads.
    #[constant]
    pub const WAITING_FILES: i32 = 2;
    /// `poll()`: the map is loaded.
    #[constant]
    pub const READY: i32 = 3;
    /// `poll()`: the load failed (or was cancelled, or none was started).
    #[constant]
    pub const FAILED: i32 = 4;

    /// Emitted by the `poll()` that finishes an asynchronous load.
    #[signal]
    fn loaded();

    /// Emitted by the `poll()` in which an asynchronous load fails (not by `cancel()`).
    #[signal]
    fn failed();

    /// Loads a map from the file system (`res://`, `user://` or an absolute path), with an
    /// optional `.tiled-project` file for custom types. Errors and warnings are printed with the
    /// GDScript runtime's wording and kept (`get_errors`, `get_warnings`). Returns `OK`, or
    /// `FAILED` when the map could not be loaded at all.
    #[func]
    pub(crate) fn load(&mut self, path: GString, project_file: GString) -> GdError {
        self.start_fs(path.to_string(), project_file.to_string(), false);
        self.pump()
    }

    /// Loads a map from a zip archive: `path_in_zip` and `project_in_zip` are paths inside the
    /// archive, which is read for this map only.
    #[func]
    #[allow(clippy::wrong_self_convention)] // the plan's name for it
    fn from_zip(
        &mut self,
        zip_file: GString,
        path_in_zip: GString,
        project_in_zip: GString,
    ) -> GdError {
        self.start_zip(
            zip_file.to_string(),
            path_in_zip.to_string(),
            project_in_zip.to_string(),
            false,
        );
        self.pump()
    }

    /// Starts loading a map as `load` does, the parse on a worker thread: `poll()` it once per
    /// frame. Returns `OK` (failures show in `poll()`). A load in progress is cancelled first.
    #[func]
    pub(crate) fn load_async(&mut self, path: GString, project_file: GString) -> GdError {
        self.start_fs(path.to_string(), project_file.to_string(), true);
        GdError::OK
    }

    /// Starts loading a map from a zip archive as `from_zip` does, asynchronously (the archive
    /// is read by the first `poll()`, the whole parse runs on the worker thread).
    #[func]
    #[allow(clippy::wrong_self_convention)] // as from_zip
    fn from_zip_async(
        &mut self,
        zip_file: GString,
        path_in_zip: GString,
        project_in_zip: GString,
    ) -> GdError {
        self.start_zip(
            zip_file.to_string(),
            path_in_zip.to_string(),
            project_in_zip.to_string(),
            true,
        );
        GdError::OK
    }

    /// Advances an asynchronous load by at most one round of the file protocol (reads the
    /// files the parse asked for and hands them over, requests and collects textures) and
    /// returns the status: `READING`, `PARSING`, `WAITING_FILES`, `READY` or `FAILED`. Never
    /// waits for the worker. Emits `loaded` or `failed` when the load ends in this call. Call it
    /// once per frame; after the end it only returns the status.
    #[func]
    pub(crate) fn poll(&mut self) -> i32 {
        let before = self.state;
        self.advance();
        let after = self.state;
        if after != before {
            match after {
                State::Ready => {
                    self.base_mut().emit_signal("loaded", &[]);
                }
                State::Failed => {
                    self.base_mut().emit_signal("failed", &[]);
                }
                _ => {}
            }
        }
        // A handler may have started another load: report what is now.
        self.state.code()
    }

    /// The status `poll()` returned last (`READY` after a successful `load`, `FAILED` before
    /// any load).
    #[func]
    pub(crate) fn get_status(&self) -> i32 {
        self.state.code()
    }

    /// Stops an asynchronous load: its worker thread ends on its own (never waited for),
    /// texture requests already made are collected by the engine's next frames. The map stays
    /// unloaded; `poll()` returns `FAILED` from now on, no signal is emitted.
    #[func]
    pub(crate) fn cancel(&mut self) {
        if matches!(self.state, State::Ready | State::Failed) {
            return;
        }
        self.job = None;
        self.map = None;
        self.cells = None;
        self.textures.clear();
        self.state = State::Failed;
    }

    /// The number of loader threads still running in this process (for tests and tools).
    #[func]
    fn get_running_loaders(&self) -> i64 {
        tiled_core::worker::running() as i64
    }

    /// The number of threaded texture requests of cancelled or freed maps not collected yet
    /// (for tests and tools; the engine's frames collect them).
    #[func]
    fn get_orphaned_texture_requests(&self) -> i64 {
        orphan_count() as i64
    }

    /// True when the loaded map holds its tile layers prepared for a build
    /// (`prepare_tile_layers`).
    #[func]
    fn has_prepared_tile_layers(&self) -> bool {
        self.cells.is_some()
    }

    /// The number of textures an asynchronous load collected and holds.
    #[func]
    fn get_texture_count(&self) -> i64 {
        self.textures.len() as i64
    }

    /// True after a successful `load` or `from_zip`.
    #[func]
    fn is_loaded(&self) -> bool {
        self.map.is_some()
    }

    /// The path the map was loaded from.
    #[func]
    fn get_path(&self) -> GString {
        self.map
            .as_ref()
            .map(|m| m.path().into())
            .unwrap_or_default()
    }

    /// The zip archive the map was read from, "" for the file system.
    #[func]
    fn get_zip_file(&self) -> GString {
        self.zip_file.as_str().into()
    }

    /// The map's size in cells (`width`, `height`).
    #[func]
    fn get_size(&self) -> Vector2i {
        self.map.as_ref().map_or(Vector2i::ZERO, |m| {
            Vector2i::new(m.width.unwrap_or(0) as i32, m.height.unwrap_or(0) as i32)
        })
    }

    /// The map's tile size in pixels.
    #[func]
    fn get_tile_size(&self) -> Vector2i {
        self.map.as_ref().map_or(Vector2i::ZERO, |m| {
            Vector2i::new(
                m.tilewidth.unwrap_or(0) as i32,
                m.tileheight.unwrap_or(0) as i32,
            )
        })
    }

    /// `orientation`: `orthogonal`, `isometric`, `staggered` or `hexagonal`.
    #[func]
    fn get_orientation(&self) -> GString {
        self.map
            .as_ref()
            .and_then(|m| m.orientation.as_deref().map(GString::from))
            .unwrap_or_default()
    }

    /// True for an infinite map (tile layers in chunks).
    #[func]
    fn is_infinite(&self) -> bool {
        self.map.as_ref().is_some_and(|m| m.is_infinite())
    }

    /// Every layer, depth first in file order, as `{name, type, id, depth}` dictionaries.
    #[func]
    fn get_layers(&self) -> VarArray {
        let mut out = VarArray::new();
        if let Some(m) = &self.map {
            fn walk(layers: &[tiled_core::Layer], depth: i64, out: &mut VarArray) {
                for l in layers {
                    let mut d = VarDictionary::new();
                    d.set("name", l.name.as_deref().unwrap_or(""));
                    d.set("type", l.kind_or_tile());
                    d.set("id", l.id.unwrap_or(0));
                    d.set("depth", depth);
                    out.push(&d.to_variant());
                    walk(l.layers.as_deref().unwrap_or(&[]), depth + 1, out);
                }
            }
            walk(m.layers.as_deref().unwrap_or(&[]), 0, &mut out);
        }
        out
    }

    /// The map as the GDScript runtime's dictionary (`DictionaryBuilder.get_dictionary`), the
    /// project's custom types merged in. Empty when nothing is loaded.
    #[func]
    fn to_dictionary(&self) -> VarDictionary {
        match &self.map {
            Some(m) => value_to_variant(&m.to_value()).to::<VarDictionary>(),
            None => VarDictionary::new(),
        }
    }

    /// The errors of the last load (each as printed).
    #[func]
    fn get_errors(&self) -> PackedStringArray {
        self.errors
            .iter()
            .map(|s| GString::from(s.as_str()))
            .collect()
    }

    /// The warnings of the last load.
    #[func]
    fn get_warnings(&self) -> PackedStringArray {
        self.warnings
            .iter()
            .map(|s| GString::from(s.as_str()))
            .collect()
    }
}

impl TiledMap {
    fn reset(&mut self, zip: &str) {
        self.job = None;
        self.map = None;
        self.cells = None;
        self.textures.clear();
        self.errors.clear();
        self.warnings.clear();
        self.zip_file = zip.to_string();
    }

    fn start_fs(&mut self, path: String, project: String, threaded: bool) {
        self.reset("");
        let mut loader = Loader::new(&path, (!project.is_empty()).then_some(project.as_str()));
        // The first step only asks for the map and the project file.
        let to_read = match loader.step() {
            LoadStatus::NeedFiles => loader.needs().to_vec(),
            LoadStatus::Done(m) => {
                // Not reached: a loader of the file system needs its map.
                self.job = Some(self.job(threaded, Driver::Done, Vec::new()));
                self.got_map(*m, None);
                return;
            }
            LoadStatus::Failed(e) => {
                self.fail(e.into_iter().map(|e| e.message).collect());
                return;
            }
        };
        self.job = Some(self.job(threaded, Driver::Main(Box::new(loader)), to_read));
        self.state = State::Reading;
    }

    fn start_zip(&mut self, zip: String, path_in_zip: String, project: String, threaded: bool) {
        self.reset(&zip);
        let driver = Driver::Zip {
            zip: zip.clone(),
            path_in_zip,
            project: (!project.is_empty()).then_some(project),
        };
        self.job = Some(self.job(threaded, driver, vec![zip]));
        self.state = State::Reading;
    }

    fn job(&self, threaded: bool, driver: Driver, to_read: Vec<String>) -> Job {
        Job {
            threaded,
            prepare: self.prepare_tile_layers,
            driver,
            to_read,
            requested: Vec::new(),
            seen: HashSet::new(),
        }
    }

    /// Runs a synchronous load to its end.
    fn pump(&mut self) -> GdError {
        while matches!(self.state, State::Reading | State::Parsing) {
            self.advance();
        }
        if self.state == State::Ready {
            GdError::OK
        } else {
            GdError::FAILED
        }
    }

    /// One round: what `poll()` does, without the signals.
    fn advance(&mut self) {
        match self.state {
            State::Reading => self.read_round(),
            State::Parsing => {
                let event = match self.job.as_mut().map(|j| &mut j.driver) {
                    Some(Driver::Worker(w)) => w.try_event(),
                    _ => None,
                };
                match event {
                    None => {}
                    Some(Event::Need { paths, images }) => {
                        for i in images {
                            self.request_texture(i);
                        }
                        if let Some(job) = self.job.as_mut() {
                            job.to_read = paths;
                        }
                        self.read_round();
                    }
                    Some(Event::Done(map, cells)) => self.got_map(*map, cells),
                    Some(Event::Failed(errors)) => {
                        self.fail(errors.into_iter().map(|e| e.message).collect())
                    }
                }
                if self.state == State::Parsing {
                    // Collect textures that arrived early.
                    self.collect_textures();
                }
            }
            State::WaitingFiles => {
                self.collect_textures();
                if self.job.as_ref().is_some_and(|j| j.requested.is_empty()) {
                    self.job = None;
                    self.state = State::Ready;
                }
            }
            State::Ready | State::Failed => {}
        }
    }

    /// Reads the files the loader asked for (on the main thread, through `FileAccess`) and
    /// hands them over: to the loader here (synchronous), or to the worker, started with them
    /// if it is not running yet.
    fn read_round(&mut self) {
        let Some(job) = self.job.as_mut() else {
            self.state = State::Failed;
            return;
        };
        let threaded = job.threaded;
        let prepare = job.prepare;
        let files: Vec<File> = std::mem::take(&mut job.to_read)
            .into_iter()
            .map(|p| {
                let b = read_file(&p);
                (p, b)
            })
            .collect();
        let (loader, files) = match std::mem::replace(&mut job.driver, Driver::Done) {
            Driver::Zip {
                zip,
                path_in_zip,
                project,
            } => {
                let bytes = files
                    .into_iter()
                    .next()
                    .and_then(|(_, b)| b)
                    .filter(|b| !b.is_empty());
                let Some(bytes) = bytes else {
                    self.fail(vec![format!("ERROR: Unable to open zip file '{zip}'.")]);
                    return;
                };
                (
                    Box::new(Loader::from_zip(bytes, &path_in_zip, project.as_deref())),
                    Vec::new(),
                )
            }
            Driver::Main(loader) => (loader, files),
            Driver::Worker(w) => {
                w.supply(files);
                job.driver = Driver::Worker(w);
                self.state = State::Parsing;
                return;
            }
            Driver::Done => {
                self.state = State::Failed;
                return;
            }
        };
        if threaded {
            let spawned = if prepare {
                Worker::spawn_preparing(*loader, files)
            } else {
                Worker::spawn(*loader, files)
            };
            match spawned {
                Ok(w) => {
                    job.driver = Driver::Worker(w);
                    self.state = State::Parsing;
                }
                Err(e) => {
                    self.fail(vec![format!(
                        "ERROR: yati_native: could not start the loader thread ({e})."
                    )]);
                }
            }
            return;
        }
        let mut loader = loader;
        for (p, b) in files {
            match b {
                Some(b) => loader.supply(&p, b),
                None => loader.missing(&p),
            }
        }
        match loader.step() {
            LoadStatus::NeedFiles => {
                job.to_read = loader.needs().to_vec();
                job.driver = Driver::Main(loader);
                self.state = State::Reading;
            }
            LoadStatus::Done(map) => self.got_map(*map, None),
            LoadStatus::Failed(errors) => {
                self.fail(errors.into_iter().map(|e| e.message).collect())
            }
        }
    }

    /// The parsed map (and its tile layers, when the worker prepared them): its messages
    /// printed and kept; a synchronous load that prepares tile layers prepares them here; an
    /// asynchronous load requests the textures it has not requested yet and waits for them.
    fn got_map(&mut self, map: Map, cells: Option<PreparedCells>) {
        let prepare = self.job.as_ref().is_some_and(|j| j.prepare);
        self.cells = match cells {
            Some(c) => Some(Arc::new(c)),
            None if prepare => Some(Arc::new(PreparedCells::prepare(&map))),
            None => None,
        };
        self.errors = map.errors().iter().map(|e| e.message.clone()).collect();
        self.warnings = map.warnings().to_vec();
        self.print_messages();
        let images = map.images().to_vec();
        self.map = Some(Arc::new(map));
        let threaded = self.job.as_ref().is_some_and(|j| j.threaded);
        if let Some(job) = self.job.as_mut() {
            job.driver = Driver::Done;
        }
        if threaded {
            for i in images {
                self.request_texture(i);
            }
            self.collect_textures();
        }
        if self.job.as_ref().is_some_and(|j| !j.requested.is_empty()) {
            self.state = State::WaitingFiles;
        } else {
            self.job = None;
            self.state = State::Ready;
        }
    }

    fn fail(&mut self, errors: Vec<String>) {
        self.job = None;
        self.map = None;
        self.cells = None;
        self.textures.clear();
        self.errors = errors;
        self.warnings.clear();
        self.print_messages();
        self.state = State::Failed;
    }

    /// Starts an imported `res://` texture loading in the engine's threads (each path once).
    fn request_texture(&mut self, path: String) {
        let Some(job) = self.job.as_mut() else { return };
        if !job.threaded || !job.seen.insert(path.clone()) {
            return;
        }
        if threaded_loadable(&path) && request_threaded(&path) {
            job.requested.push(path);
        }
    }

    /// Collects the requested textures the engine has finished (loaded or failed: a failure
    /// is reported by the builder's own load).
    fn collect_textures(&mut self) {
        let Some(job) = self.job.as_mut() else { return };
        let mut got = Vec::new();
        job.requested.retain(|p| {
            if threaded_status(p) == THREAD_LOAD_IN_PROGRESS {
                return true;
            }
            if let Some(r) = threaded_get(p) {
                got.push(r);
            }
            false
        });
        self.textures.extend(got);
    }

    fn print_messages(&self) {
        for e in &self.errors {
            printerr(&[e.to_variant()]);
        }
        for w in &self.warnings {
            print_rich(
                &[format!("[color={}]{w}[/color]", crate::diag::WARNING_COLOR).to_variant()],
            );
        }
    }

    /// The parsed map.
    pub fn inner(&self) -> Option<Arc<Map>> {
        self.map.clone()
    }

    /// The map's prepared tile layers (`prepare_tile_layers`).
    pub(crate) fn prepared(&self) -> Option<Arc<PreparedCells>> {
        self.cells.clone()
    }

    /// The map's path ("" before a load).
    pub(crate) fn get_path_str(&self) -> GString {
        self.get_path()
    }

    /// The map's tile size (16 × 16 before a load).
    pub(crate) fn tile_size(&self) -> Vector2i {
        match &self.map {
            Some(m) => Vector2i::new(
                m.tilewidth.unwrap_or(16) as i32,
                m.tileheight.unwrap_or(16) as i32,
            ),
            None => Vector2i::new(16, 16),
        }
    }
}

/// The main thread's file reader: `FileAccess`, so `res://` inside an exported .pck works.
fn read_file(path: &str) -> Option<Vec<u8>> {
    if !FileAccess::file_exists(path) {
        return None;
    }
    Some(FileAccess::get_file_as_bytes(path).to_vec())
}
