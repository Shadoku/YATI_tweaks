//! The loader and its file-request protocol.
//!
//! The loader never opens a file (an exported Godot project's `res://` files live in the .pck,
//! out of `std::fs`'s reach): it says which files it needs, the caller reads them and hands
//! the bytes over, and the loader goes on. A map takes two or three rounds: the map (and the
//! project file), then its external tilesets and templates, then the templates' tilesets.
//!
//! ```no_run
//! use tiled_core::{Loader, Status};
//! let mut loader = Loader::new("res://maps/level.tmx", Some("res://maps/game.tiled-project"));
//! let map = loop {
//!     match loader.step() {
//!         Status::NeedFiles => {
//!             for path in loader.needs().to_vec() {
//!                 match std::fs::read(&path) {
//!                     Ok(bytes) => loader.supply(&path, bytes),
//!                     Err(_) => loader.missing(&path),
//!                 }
//!             }
//!         }
//!         Status::Done(map) => break map,
//!         Status::Failed(errors) => panic!("{errors:?}"),
//!     }
//! };
//! println!("{} layers, {} errors", map.all_layers().len(), map.errors().len());
//! ```
//!
//! Paths are resolved relative to the file that references them (`CommonUtils.cleanup_path`
//! semantics, a `res://`, `user://`, `/` or drive prefix kept as it is). In zip mode
//! ([`Loader::from_zip`]) the loader reads the archive's members itself and never asks; a
//! reference is looked up as written first, then relative to its file, as the GDScript
//! runtime's `DataLoader` does.
//!
//! When the loader is done, external tilesets and templates are in the map
//! ([`Map::tileset_slots`], [`Map::templates`]), a project's custom types are merged into the
//! map, its layers, their objects, the tilesets, their tiles and the tiles' collision objects
//! (`CustomTypes.merge_custom_properties` at every place the GDScript runtime calls it), and
//! the images all of them refer to are listed in [`Map::images`].

use crate::model::map::Loaded;
use crate::model::template::TemplateLoaded;
use crate::model::{Layer, Map, Meta, Project, Template, Tileset, TilesetSlot, TilesetSource};
use crate::{parse, path, Error, ErrorKind};
use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Read};
use std::sync::Arc;

/// What [`Loader::step`] reached.
#[derive(Debug)]
pub enum Status {
    /// Files are needed: read every path in [`Loader::needs`], hand each over with
    /// [`Loader::supply`] or [`Loader::missing`], then step again.
    NeedFiles,
    /// The map, with everything it refers to. Errors that did not stop the load (a missing
    /// tileset, ...) are in [`Map::errors`].
    Done(Box<Map>),
    /// The map could not be loaded (not found, not parseable, ...).
    Failed(Vec<Error>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Map,
    Project,
    Tileset,
    Template,
}

#[derive(Debug)]
enum FileState {
    Requested,
    Bytes(Vec<u8>),
    Missing,
    Consumed,
}

#[derive(Debug)]
struct Request {
    kind: Kind,
    /// The reference as written in the referencing file (for messages).
    reference: String,
    state: FileState,
}

/// Loads a map and the files it refers to, through the file-request protocol.
#[derive(Debug)]
pub struct Loader {
    map_path: String,
    project_path: Option<String>,
    merge_custom_types: bool,
    zip: Option<Result<zip::ZipArchive<Cursor<Vec<u8>>>, String>>,
    requests: HashMap<String, Request>,
    order: Vec<String>,
    needs: Vec<String>,
    started: bool,
    map: Option<Map>,
    project: Option<Arc<Project>>,
    tilesets: HashMap<String, Option<Tileset>>,
    templates: HashMap<String, Option<Template>>,
    errors: Vec<Error>,
    warnings: Vec<String>,
    finished: bool,
    /// Image references found by the parse so far (resolved), for [`Loader::take_discovered_images`].
    discovered: Vec<String>,
    /// How many of `discovered` were taken.
    taken: usize,
}

impl Loader {
    /// A loader for the map at `map_path`, with an optional `.tiled-project` file for custom
    /// types (taken as given, not relative to the map).
    pub fn new(map_path: &str, project_file: Option<&str>) -> Self {
        Loader {
            map_path: map_path.to_string(),
            project_path: project_file.filter(|p| !p.is_empty()).map(str::to_string),
            merge_custom_types: true,
            zip: None,
            requests: HashMap::new(),
            order: Vec::new(),
            needs: Vec::new(),
            started: false,
            map: None,
            project: None,
            tilesets: HashMap::new(),
            templates: HashMap::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            finished: false,
            discovered: Vec::new(),
            taken: 0,
        }
    }

    /// A loader that reads the map and everything it refers to from a zip archive (deflate or
    /// stored members); every path is a path inside the archive.
    pub fn from_zip(
        zip_bytes: Vec<u8>,
        map_path_in_zip: &str,
        project_in_zip: Option<&str>,
    ) -> Self {
        let mut l = Loader::new(map_path_in_zip, project_in_zip);
        l.zip = Some(zip::ZipArchive::new(Cursor::new(zip_bytes)).map_err(|e| e.to_string()));
        l
    }

    /// Whether to merge the project's custom types into the model (default true). Off, the
    /// project is still loaded and available as [`Map::project`].
    pub fn merge_custom_types(mut self, merge: bool) -> Self {
        self.merge_custom_types = merge;
        self
    }

    /// The paths the loader waits for (after a [`Status::NeedFiles`]).
    pub fn needs(&self) -> &[String] {
        &self.needs
    }

    /// Hands over the bytes of a needed file.
    pub fn supply(&mut self, path: &str, bytes: Vec<u8>) {
        if let Some(r) = self.requests.get_mut(path) {
            if matches!(r.state, FileState::Requested) {
                r.state = FileState::Bytes(bytes);
            }
        }
        self.needs.retain(|p| p != path);
    }

    /// Reports that a needed file does not exist (or cannot be read).
    pub fn missing(&mut self, path: &str) {
        if let Some(r) = self.requests.get_mut(path) {
            if matches!(r.state, FileState::Requested) {
                r.state = FileState::Missing;
            }
        }
        self.needs.retain(|p| p != path);
    }

    /// The images the parse has found since the last call (resolved paths, each listed once over
    /// the loader's life): the embedded tilesets' and image layers' of the map, and those of every
    /// external tileset parsed so far. A caller that loads textures asynchronously starts on
    /// them before the map is done; the complete list is [`Map::images`] once it is. Empty in zip
    /// mode, whose images are members of the archive.
    pub fn take_discovered_images(&mut self) -> Vec<String> {
        let new = self.discovered[self.taken..].to_vec();
        self.taken = self.discovered.len();
        new
    }

    fn discover(&mut self, base: &str, reference: &Option<String>) {
        if self.zip.is_some() {
            return;
        }
        let Some(r) = reference.as_deref() else {
            return;
        };
        let ext = path::extension(r);
        if ext == "tmx" || ext == "tmj" {
            return;
        }
        let resolved = path::resolve(base, r);
        if !self.discovered.contains(&resolved) {
            self.discovered.push(resolved);
        }
    }

    fn discover_tileset(&mut self, base: &str, ts: &Tileset) {
        self.discover(base, &ts.image);
        for tile in ts.tiles.iter().flatten() {
            self.discover(base, &tile.image);
        }
    }

    /// Runs the loader to completion with a file reader (`None`: the file does not exist).
    pub fn run(mut self, mut read: impl FnMut(&str) -> Option<Vec<u8>>) -> Result<Map, Vec<Error>> {
        loop {
            match self.step() {
                Status::NeedFiles => {
                    for p in self.needs.clone() {
                        match read(&p) {
                            Some(b) => self.supply(&p, b),
                            None => self.missing(&p),
                        }
                    }
                }
                Status::Done(m) => return Ok(*m),
                Status::Failed(e) => return Err(e),
            }
        }
    }

    /// Advances as far as the files at hand allow.
    pub fn step(&mut self) -> Status {
        if self.finished {
            return Status::Failed(vec![Error::new(
                ErrorKind::Parse,
                "The loader has already finished.",
            )]);
        }
        if !self.started {
            self.started = true;
            if let Some(Err(e)) = &self.zip {
                self.finished = true;
                return Status::Failed(vec![Error::new(
                    ErrorKind::Zip,
                    format!("ERROR: Unable to open zip file ({e})."),
                )]);
            }
            let map_path = self.map_path.clone();
            self.request(map_path.clone(), Kind::Map, &map_path);
            if let Some(p) = self.project_path.clone() {
                self.request(p.clone(), Kind::Project, &p);
            }
        }
        loop {
            match self.advance() {
                Err(errors) => {
                    self.finished = true;
                    return Status::Failed(errors);
                }
                Ok(false) => break,
                Ok(true) => {}
            }
        }
        self.needs = self
            .order
            .iter()
            .filter(|p| matches!(self.requests[*p].state, FileState::Requested))
            .cloned()
            .collect();
        if !self.needs.is_empty() {
            return Status::NeedFiles;
        }
        self.finished = true;
        Status::Done(Box::new(self.assemble()))
    }

    fn request(&mut self, path: String, kind: Kind, reference: &str) {
        if self.requests.contains_key(&path) {
            return;
        }
        let state = match &mut self.zip {
            Some(Ok(z)) => match read_zip(z, reference, &path) {
                Some(b) => FileState::Bytes(b),
                None => FileState::Missing,
            },
            _ => FileState::Requested,
        };
        self.order.push(path.clone());
        self.requests.insert(
            path,
            Request {
                kind,
                reference: reference.to_string(),
                state,
            },
        );
    }

    /// Consumes every file that arrived; true when anything happened.
    fn advance(&mut self) -> Result<bool, Vec<Error>> {
        let ready: Vec<String> = self
            .order
            .iter()
            .filter(|p| {
                matches!(
                    self.requests[*p].state,
                    FileState::Bytes(_) | FileState::Missing
                )
            })
            .cloned()
            .collect();
        if ready.is_empty() {
            return Ok(false);
        }
        for path in ready {
            let r = self.requests.get_mut(&path).expect("requested");
            let state = std::mem::replace(&mut r.state, FileState::Consumed);
            let (kind, reference) = (r.kind, r.reference.clone());
            let bytes = match state {
                FileState::Bytes(b) => Some(b),
                _ => None,
            };
            match kind {
                Kind::Map => self.got_map(&path, bytes)?,
                Kind::Project => self.got_project(&path, bytes)?,
                Kind::Tileset => self.got_tileset(&path, &reference, bytes),
                Kind::Template => self.got_template(&path, bytes),
            }
        }
        Ok(true)
    }

    fn got_map(&mut self, path: &str, bytes: Option<Vec<u8>>) -> Result<(), Vec<Error>> {
        let Some(bytes) = bytes else {
            return Err(vec![Error::new(
                ErrorKind::NotFound,
                format!("ERROR: Tiled map file '{path}' not found."),
            )]);
        };
        let map = parse::map(&bytes, path).map_err(|e| vec![e])?;
        let base = path::base_dir(path);
        for ts in map.tilesets.iter().flatten() {
            if let Some(src) = ts.source.as_deref() {
                if !src.starts_with(":/automap") {
                    self.request(path::resolve(&base, src), Kind::Tileset, src);
                }
            }
        }
        let mut refs = Vec::new();
        for l in map.all_layers() {
            for o in l.objects.iter().flatten() {
                if let Some(t) = o.template.as_deref() {
                    refs.push(t.to_string());
                }
            }
        }
        for t in refs {
            self.request(path::resolve(&base, &t), Kind::Template, &t);
        }
        for ts in map.tilesets.iter().flatten() {
            if ts.source.is_none() {
                self.discover_tileset(&base, ts);
            }
        }
        for l in map.all_layers() {
            if l.kind.as_deref() == Some("imagelayer") {
                self.discover(&base, &l.image);
            }
        }
        self.map = Some(map);
        Ok(())
    }

    fn got_project(&mut self, path: &str, bytes: Option<Vec<u8>>) -> Result<(), Vec<Error>> {
        let Some(bytes) = bytes else {
            let e = Error::new(
                ErrorKind::NotFound,
                format!("ERROR: Tiled project file '{path}' not found."),
            );
            // Fatal outside a zip (Importer.gd checks it first), an error inside one.
            if self.zip.is_none() {
                return Err(vec![e]);
            }
            self.errors.push(e);
            return Ok(());
        };
        match Project::from_json(&bytes) {
            Ok(mut p) => {
                p.path = path.to_string();
                self.project = Some(Arc::new(p));
            }
            Err(_) => self.errors.push(Error::new(
                ErrorKind::Parse,
                format!("ERROR: Tiled project file '{path}' could not be parsed."),
            )),
        }
        Ok(())
    }

    fn got_tileset(&mut self, path: &str, reference: &str, bytes: Option<Vec<u8>>) {
        let parsed = match bytes {
            None => {
                self.errors.push(Error::new(
                    ErrorKind::NotFound,
                    format!("ERROR: Tileset file '{reference}' not found. -> Continuing but result may be unusable"),
                ));
                None
            }
            Some(b) => match parse::tileset(&b, path) {
                Ok(t) => {
                    self.discover_tileset(&path::base_dir(path), &t);
                    Some(t)
                }
                Err(e) => {
                    self.errors.push(e);
                    None
                }
            },
        };
        self.tilesets.insert(path.to_string(), parsed);
    }

    fn got_template(&mut self, path: &str, bytes: Option<Vec<u8>>) {
        let parsed = match bytes {
            // Reported per instance when the map is assembled, as the runtime reports it.
            None => None,
            Some(b) => match parse::template(&b, path) {
                Ok(t) => {
                    let base = path::base_dir(path);
                    for ts in &t.tilesets {
                        if let Some(src) = ts.source.as_deref() {
                            if !src.starts_with(":/automap") {
                                self.request(path::resolve(&base, src), Kind::Tileset, src);
                            }
                        }
                    }
                    Some(t)
                }
                Err(e) => {
                    self.errors.push(e);
                    None
                }
            },
        };
        self.templates.insert(path.to_string(), parsed);
    }

    /// Puts the map together: tileset slots, templates, custom types, images.
    fn assemble(&mut self) -> Map {
        let mut map = self.map.take().expect("map parsed before assembly");
        let base = path::base_dir(&self.map_path);
        let project = self.project.clone();
        let merge = self.merge_custom_types;

        // External tilesets: custom types merged once, then shared.
        let mut shared: HashMap<String, Option<Arc<Tileset>>> = HashMap::new();
        for (p, t) in self.tilesets.drain() {
            let t = t.map(|mut t| {
                if let (true, Some(pr)) = (merge, project.as_deref()) {
                    merge_tileset(pr, &mut t);
                }
                Arc::new(t)
            });
            shared.insert(p, t);
        }

        if let (true, Some(pr)) = (merge, project.as_deref()) {
            let class = class_or_kind(&map.class, &map.kind);
            pr.merge(&mut map.properties, &class, "map");
            for l in map.layers.iter_mut().flatten() {
                merge_layer(pr, l);
            }
            for t in map.tilesets.iter_mut().flatten() {
                if t.source.is_none() {
                    merge_tileset(pr, t);
                }
            }
        }

        let slots = slots_of(map.tilesets.as_deref().unwrap_or(&[]), &base, &shared);

        // Templates, with their own tileset slots.
        let mut templates = BTreeMap::new();
        let mut missing_templates = Vec::new();
        for (p, t) in self.templates.drain() {
            match t {
                Some(mut t) => {
                    let tbase = path::base_dir(&p);
                    let tslots = slots_of(&t.tilesets, &tbase, &shared);
                    t.loaded = Meta(TemplateLoaded {
                        path: p.clone(),
                        slots: tslots,
                    });
                    templates.insert(p, t);
                }
                None => missing_templates.push(p),
            }
        }
        for l in map.all_layers() {
            for o in l.objects.iter().flatten() {
                if let Some(t) = o.template.as_deref() {
                    if missing_templates.contains(&path::resolve(&base, t)) {
                        self.errors.push(Error::new(
                            ErrorKind::NotFound,
                            format!("ERROR: Template file '{t}' not found. -> Continuing but result may be unusable"),
                        ));
                    }
                }
            }
        }

        // Image references.
        let mut images = Images {
            zip: match &self.zip {
                Some(Ok(z)) => Some(z),
                _ => None,
            },
            list: Vec::new(),
        };
        for s in &slots {
            if let Some(ts) = s.tileset(map.tilesets.as_deref().unwrap_or(&[])) {
                images.tileset(ts, &s.base_dir);
            }
        }
        for l in map.all_layers() {
            if l.kind.as_deref() == Some("imagelayer") {
                images.add(&base, &l.image);
            }
        }
        for t in templates.values() {
            for s in t.tileset_slots() {
                if let Some(ts) = s.tileset(&t.tilesets) {
                    images.tileset(ts, &s.base_dir);
                }
            }
        }
        let images = images.list;

        map.loaded = Meta(Loaded {
            path: self.map_path.clone(),
            slots,
            templates,
            project,
            images,
            errors: std::mem::take(&mut self.errors),
            warnings: std::mem::take(&mut self.warnings),
        });
        map
    }
}

/// Image references, resolved and each listed once.
struct Images<'z> {
    zip: Option<&'z zip::ZipArchive<Cursor<Vec<u8>>>>,
    list: Vec<String>,
}

impl Images<'_> {
    /// A reference from a file in `base`: in a zip, a member named as written wins (the
    /// runtime's lookup order); a `.tmx`/`.tmj` "image" (a map placeholder) is not an image.
    fn add(&mut self, base: &str, reference: &Option<String>) {
        let Some(r) = reference.as_deref() else {
            return;
        };
        let ext = path::extension(r);
        if ext == "tmx" || ext == "tmj" {
            return;
        }
        let resolved = match self.zip {
            Some(z) if z.index_for_name(r).is_some() => r.to_string(),
            _ => path::resolve(base, r),
        };
        if !self.list.contains(&resolved) {
            self.list.push(resolved);
        }
    }

    fn tileset(&mut self, ts: &Tileset, base: &str) {
        self.add(base, &ts.image);
        for tile in ts.tiles.iter().flatten() {
            self.add(base, &tile.image);
        }
    }
}

fn read_zip(
    z: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
    reference: &str,
    resolved: &str,
) -> Option<Vec<u8>> {
    for name in [reference, resolved] {
        if let Ok(mut f) = z.by_name(name) {
            if f.is_dir() {
                continue;
            }
            let mut out = Vec::with_capacity(f.size() as usize);
            if f.read_to_end(&mut out).is_ok() {
                return Some(out);
            }
        }
    }
    None
}

fn slots_of(
    tilesets: &[Tileset],
    base: &str,
    shared: &HashMap<String, Option<Arc<Tileset>>>,
) -> Vec<TilesetSlot> {
    tilesets
        .iter()
        .enumerate()
        .map(|(i, ts)| {
            let firstgid = ts.firstgid.unwrap_or(0);
            match ts.source.as_deref() {
                Some(src) if src.starts_with(":/automap") => TilesetSlot {
                    firstgid,
                    source: TilesetSource::Skipped,
                    base_dir: base.to_string(),
                },
                Some(src) => {
                    let p = path::resolve(base, src);
                    TilesetSlot {
                        firstgid,
                        base_dir: path::base_dir(&p),
                        source: TilesetSource::External {
                            tileset: shared.get(&p).cloned().flatten(),
                            path: p,
                        },
                    }
                }
                None => TilesetSlot {
                    firstgid,
                    source: TilesetSource::Embedded(i),
                    base_dir: base.to_string(),
                },
            }
        })
        .collect()
}

/// `class` when set and not empty, else `type` (`CustomTypes.merge_custom_properties`).
fn class_or_kind(class: &Option<String>, kind: &Option<String>) -> String {
    match class.as_deref() {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => kind.clone().unwrap_or_default(),
    }
}

fn merge_layer(p: &Project, l: &mut Layer) {
    let class = class_or_kind(&l.class, &l.kind);
    p.merge(&mut l.properties, &class, "layer");
    for o in l.objects.iter_mut().flatten() {
        let class = o.class_name().to_string();
        p.merge(&mut o.properties, &class, "object");
    }
    for c in l.layers.iter_mut().flatten() {
        merge_layer(p, c);
    }
}

fn merge_tileset(p: &Project, t: &mut Tileset) {
    let class = t.class_name().to_string();
    p.merge(&mut t.properties, &class, "tileset");
    for tile in t.tiles.iter_mut().flatten() {
        let class = tile.class_name().to_string();
        p.merge(&mut tile.properties, &class, "tile");
        if let Some(g) = &mut tile.objectgroup {
            for o in g.objects.iter_mut().flatten() {
                let class = o.class_name().to_string();
                p.merge(&mut o.properties, &class, "object");
            }
        }
    }
}

/// Loads a map from the file system (`std::fs`), relative paths against the working
/// directory: for tools and tests. A Godot project reads through the protocol instead.
pub fn load_from_fs(map_path: &str, project_file: Option<&str>) -> Result<Map, Vec<Error>> {
    Loader::new(map_path, project_file).run(|p| std::fs::read(p).ok())
}
