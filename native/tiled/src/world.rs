//! Tiled world files (`.world`) and map bounds.
//!
//! A world file places maps in one coordinate space: explicitly (`maps`: a file name and a
//! rect each) or by file name patterns (`patterns`: every map file of the world's directory
//! whose name matches a regular expression, positioned by its captures). The GDScript runtime's
//! `WorldLoader.gd` reads them the same way; this module is its pure-Rust twin.
//!
//! [`World::parse`] reads a world file's own content (explicit maps, the patterns as written).
//! Resolving the patterns needs the directory's file names and the size of every matching map,
//! so [`WorldLoader`] asks for them through the loader's request style: first the world file,
//! then a listing of its directory ([`WorldRequest::ListDir`]), then the matching map files
//! whose size the pattern does not give.
//!
//! ```no_run
//! use tiled_core::world::{WorldLoader, WorldRequest, WorldStatus};
//! let mut loader = WorldLoader::new("res://maps/overworld.world");
//! let world = loop {
//!     match loader.step() {
//!         WorldStatus::NeedFiles => {
//!             for request in loader.needs().to_vec() {
//!                 match request {
//!                     WorldRequest::File(path) => match std::fs::read(&path) {
//!                         Ok(bytes) => loader.supply(&path, bytes),
//!                         Err(_) => loader.missing(&path),
//!                     },
//!                     WorldRequest::ListDir(dir) => {
//!                         let names = std::fs::read_dir(&dir)
//!                             .map(|d| d.flatten().filter_map(|e| e.file_name().into_string().ok()).collect())
//!                             .unwrap_or_default();
//!                         loader.supply_listing(&dir, names);
//!                     }
//!                 }
//!             }
//!         }
//!         WorldStatus::Done(world) => break world,
//!         WorldStatus::Failed(errors) => panic!("{errors:?}"),
//!     }
//! };
//! for m in &world.maps {
//!     println!("{} at {:?}", m.path, m.rect);
//! }
//! ```
//!
//! [`Map::map_rect`] is a map's bounds in pixels, the `map_rect` metadata the GDScript runtime
//! sets on an imported map's root (`TilemapCreator.get_pixel_rect`), with the same formula.

use crate::model::Map;
use crate::{parse, path, Error, ErrorKind};
use regex::Regex;
use serde_json::Value as Json;
use std::collections::HashSet;
use std::io::{Cursor, Read};

/// A map of a world: its path (resolved against the world file's directory) and where the world
/// puts it, `(x, y, width, height)` in pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldMap {
    /// The map file's path.
    pub path: String,
    /// `(x, y, width, height)` in pixels.
    pub rect: (i64, i64, i64, i64),
}

/// A `patterns` entry of a world file, as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldPattern {
    /// `regexp`, matched against file names (unanchored, as Tiled matches it).
    pub regexp: String,
    /// `multiplierX` (default 1): pixels per unit of capture 1.
    pub multiplier_x: i64,
    /// `multiplierY` (default 1): pixels per unit of capture 2.
    pub multiplier_y: i64,
    /// `offsetX` (default 0).
    pub offset_x: i64,
    /// `offsetY` (default 0).
    pub offset_y: i64,
    /// `mapWidth`, when given (else the map's own width in pixels).
    pub map_width: Option<i64>,
    /// `mapHeight`, when given (else the map's own height in pixels).
    pub map_height: Option<i64>,
}

/// A world: its maps, in the order of the file's `maps` entries, then the maps its patterns list
/// (sorted by file name; a file listed in `maps` is not listed again, a file goes with the first
/// pattern it matches).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct World {
    /// The world file's path.
    pub path: String,
    /// The maps and their rects.
    pub maps: Vec<WorldMap>,
    /// The `patterns` entries as written. [`World::parse`] leaves them unresolved;
    /// [`WorldLoader`] adds the maps they list to `maps`.
    pub patterns: Vec<WorldPattern>,
    /// Errors that did not stop the world from loading (a bad regexp, a map that cannot be read).
    pub errors: Vec<Error>,
}

/// An integer field of a world entry: JSON numbers truncated toward zero, as GDScript's `int()`.
fn int_field(entry: &Json, key: &str) -> Option<i64> {
    match entry.get(key)? {
        Json::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Json::String(s) => crate::godot_str::is_valid_int(s).then(|| crate::godot_str::to_int(s)),
        Json::Bool(b) => Some(i64::from(*b)),
        _ => None,
    }
}

fn string_field(entry: &Json, key: &str) -> String {
    match entry.get(key) {
        Some(Json::String(s)) => s.clone(),
        _ => String::new(),
    }
}

impl World {
    /// Parses a world file (`json`, the content of `world_path`): its explicit `maps`, each
    /// resolved against the world file's directory, and its `patterns` as written.
    pub fn parse(json: &[u8], world_path: &str) -> Result<World, Error> {
        let unreadable = || {
            Error::new(
                ErrorKind::Parse,
                format!("ERROR: Tiled world file '{world_path}' could not be parsed."),
            )
        };
        let doc: Json = serde_json::from_slice(parse::strip_bom(json)).map_err(|_| unreadable())?;
        if !doc.is_object() {
            return Err(unreadable());
        }
        let dir = path::base_dir(world_path);
        let mut world = World {
            path: world_path.to_string(),
            ..World::default()
        };
        if let Some(Json::Array(maps)) = doc.get("maps") {
            for entry in maps.iter().filter(|e| e.is_object()) {
                let file = string_field(entry, "fileName");
                if file.is_empty() {
                    continue;
                }
                let field = |k| int_field(entry, k).unwrap_or(0);
                world.maps.push(WorldMap {
                    path: path::resolve(&dir, &file),
                    rect: (field("x"), field("y"), field("width"), field("height")),
                });
            }
        }
        if let Some(Json::Array(patterns)) = doc.get("patterns") {
            for entry in patterns.iter().filter(|e| e.is_object()) {
                world.patterns.push(WorldPattern {
                    regexp: string_field(entry, "regexp"),
                    multiplier_x: int_field(entry, "multiplierX").unwrap_or(1),
                    multiplier_y: int_field(entry, "multiplierY").unwrap_or(1),
                    offset_x: int_field(entry, "offsetX").unwrap_or(0),
                    offset_y: int_field(entry, "offsetY").unwrap_or(0),
                    map_width: int_field(entry, "mapWidth"),
                    map_height: int_field(entry, "mapHeight"),
                });
            }
        }
        Ok(world)
    }

    /// The directory the world's files are in (what its references are relative to).
    pub fn base_dir(&self) -> String {
        path::base_dir(&self.path)
    }
}

/// What a [`WorldLoader`] needs from its caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldRequest {
    /// The bytes of a file: [`WorldLoader::supply`] or [`WorldLoader::missing`].
    File(String),
    /// The names of the files directly in a directory (no subdirectories):
    /// [`WorldLoader::supply_listing`], or [`WorldLoader::missing`] (an empty listing).
    ListDir(String),
}

impl WorldRequest {
    /// The file's or the directory's path.
    pub fn path(&self) -> &str {
        match self {
            WorldRequest::File(p) | WorldRequest::ListDir(p) => p,
        }
    }
}

/// What [`WorldLoader::step`] reached.
#[derive(Debug)]
pub enum WorldStatus {
    /// Requests are open: answer every one of [`WorldLoader::needs`], then step again.
    NeedFiles,
    /// The world with every map placed. Non-fatal errors are in [`World::errors`].
    Done(World),
    /// The world file could not be read or parsed.
    Failed(Vec<Error>),
}

#[derive(Debug)]
enum Answer {
    Waiting,
    Bytes(Vec<u8>),
    Listing(Vec<String>),
    Missing,
}

/// A map a pattern listed, waiting for its size.
#[derive(Debug)]
struct Pending {
    path: String,
    pos: (i64, i64),
    size: (Option<i64>, Option<i64>),
}

#[derive(Debug)]
enum Phase {
    Start,
    World,
    Listing,
    Sizes,
    Finished,
}

/// Loads a world file and resolves its patterns, through the file-request protocol (see the
/// module documentation). Each request is made once.
#[derive(Debug)]
pub struct WorldLoader {
    world_path: String,
    zip: Option<Result<zip::ZipArchive<Cursor<Vec<u8>>>, String>>,
    phase: Phase,
    needs: Vec<WorldRequest>,
    answers: Vec<(WorldRequest, Answer)>,
    world: Option<World>,
    regexes: Vec<(Regex, WorldPattern)>,
    pending: Vec<Pending>,
}

impl WorldLoader {
    /// A loader for the world file at `world_path`.
    pub fn new(world_path: &str) -> Self {
        WorldLoader {
            world_path: world_path.to_string(),
            zip: None,
            phase: Phase::Start,
            needs: Vec::new(),
            answers: Vec::new(),
            world: None,
            regexes: Vec::new(),
            pending: Vec::new(),
        }
    }

    /// A loader that reads the world file, its directory and its maps from a zip archive
    /// (every path is a path inside the archive); it never asks the caller.
    pub fn from_zip(zip_bytes: Vec<u8>, world_path_in_zip: &str) -> Self {
        let mut l = WorldLoader::new(world_path_in_zip);
        l.zip = Some(zip::ZipArchive::new(Cursor::new(zip_bytes)).map_err(|e| e.to_string()));
        l
    }

    /// The open requests (after a [`WorldStatus::NeedFiles`]).
    pub fn needs(&self) -> &[WorldRequest] {
        &self.needs
    }

    /// Hands over the bytes of a requested file.
    pub fn supply(&mut self, path: &str, bytes: Vec<u8>) {
        self.answer(&WorldRequest::File(path.to_string()), Answer::Bytes(bytes));
    }

    /// Hands over the file names (not paths) directly in a requested directory.
    pub fn supply_listing(&mut self, dir: &str, names: Vec<String>) {
        self.answer(
            &WorldRequest::ListDir(dir.to_string()),
            Answer::Listing(names),
        );
    }

    /// Reports that a requested file or directory does not exist (or cannot be read).
    pub fn missing(&mut self, path: &str) {
        let request = self.needs.iter().find(|r| r.path() == path).cloned();
        if let Some(r) = request {
            self.answer(&r, Answer::Missing);
        }
    }

    fn answer(&mut self, request: &WorldRequest, answer: Answer) {
        if let Some(i) = self.needs.iter().position(|r| r == request) {
            self.needs.remove(i);
            if let Some(slot) = self.answers.iter_mut().find(|(r, _)| r == request) {
                slot.1 = answer;
            }
        }
    }

    fn request(&mut self, request: WorldRequest) {
        let answer = match &mut self.zip {
            Some(Ok(z)) => answer_from_zip(z, &request),
            _ => {
                self.needs.push(request.clone());
                Answer::Waiting
            }
        };
        self.answers.push((request, answer));
    }

    fn take(&mut self, request: &WorldRequest) -> Option<Answer> {
        let i = self.answers.iter().position(|(r, _)| r == request)?;
        if matches!(self.answers[i].1, Answer::Waiting) {
            return None;
        }
        Some(self.answers.remove(i).1)
    }

    /// Runs the loader to completion with a file reader and a directory lister (`None`: it does
    /// not exist).
    pub fn run(
        mut self,
        mut read: impl FnMut(&str) -> Option<Vec<u8>>,
        mut list: impl FnMut(&str) -> Option<Vec<String>>,
    ) -> Result<World, Vec<Error>> {
        loop {
            match self.step() {
                WorldStatus::NeedFiles => {
                    for r in self.needs.clone() {
                        match &r {
                            WorldRequest::File(p) => match read(p) {
                                Some(b) => self.supply(p, b),
                                None => self.missing(p),
                            },
                            WorldRequest::ListDir(d) => match list(d) {
                                Some(n) => self.supply_listing(d, n),
                                None => self.missing(d),
                            },
                        }
                    }
                }
                WorldStatus::Done(w) => return Ok(w),
                WorldStatus::Failed(e) => return Err(e),
            }
        }
    }

    /// Advances as far as the answers at hand allow.
    pub fn step(&mut self) -> WorldStatus {
        loop {
            match self.phase {
                Phase::Start => {
                    if let Some(Err(e)) = &self.zip {
                        self.phase = Phase::Finished;
                        return WorldStatus::Failed(vec![Error::new(
                            ErrorKind::Zip,
                            format!("ERROR: Unable to open zip file ({e})."),
                        )]);
                    }
                    self.request(WorldRequest::File(self.world_path.clone()));
                    self.phase = Phase::World;
                }
                Phase::World => {
                    let request = WorldRequest::File(self.world_path.clone());
                    let Some(answer) = self.take(&request) else {
                        return WorldStatus::NeedFiles;
                    };
                    let Answer::Bytes(bytes) = answer else {
                        self.phase = Phase::Finished;
                        return WorldStatus::Failed(vec![Error::new(
                            ErrorKind::NotFound,
                            format!("ERROR: Tiled world file '{}' not found.", self.world_path),
                        )]);
                    };
                    let mut world = match World::parse(&bytes, &self.world_path) {
                        Ok(w) => w,
                        Err(e) => {
                            self.phase = Phase::Finished;
                            return WorldStatus::Failed(vec![e]);
                        }
                    };
                    for p in &world.patterns {
                        match Regex::new(&p.regexp) {
                            Ok(r) => self.regexes.push((r, p.clone())),
                            Err(_) => world.errors.push(Error::new(
                                ErrorKind::Parse,
                                format!(
                                    "ERROR: Tiled world file '{}': pattern '{}' is not a valid regular expression. -> Skipped",
                                    self.world_path, p.regexp
                                ),
                            )),
                        }
                    }
                    let has_patterns = !world.patterns.is_empty();
                    let dir = world.base_dir();
                    self.world = Some(world);
                    if has_patterns {
                        self.request(WorldRequest::ListDir(dir));
                        self.phase = Phase::Listing;
                    } else {
                        self.phase = Phase::Sizes;
                    }
                }
                Phase::Listing => {
                    let world = self.world.as_ref().expect("world parsed");
                    let request = WorldRequest::ListDir(world.base_dir());
                    let Some(answer) = self.take(&request) else {
                        return WorldStatus::NeedFiles;
                    };
                    let mut names = match answer {
                        Answer::Listing(n) => n,
                        _ => Vec::new(),
                    };
                    names.sort();
                    names.dedup();
                    self.match_patterns(&names);
                    for i in 0..self.pending.len() {
                        if self.pending[i].size.0.is_none() || self.pending[i].size.1.is_none() {
                            let p = self.pending[i].path.clone();
                            self.request(WorldRequest::File(p));
                        }
                    }
                    self.phase = Phase::Sizes;
                }
                Phase::Sizes => {
                    if !self.resolve_sizes() {
                        return WorldStatus::NeedFiles;
                    }
                    self.phase = Phase::Finished;
                    return WorldStatus::Done(self.world.take().expect("world parsed"));
                }
                Phase::Finished => {
                    return WorldStatus::Failed(vec![Error::new(
                        ErrorKind::Parse,
                        "The world loader has already finished.",
                    )]);
                }
            }
        }
    }

    /// Lists the map files the patterns match, in name order, each with its first pattern.
    fn match_patterns(&mut self, names: &[String]) {
        let world = self.world.as_ref().expect("world parsed");
        let dir = world.base_dir();
        let mut listed: HashSet<String> = world.maps.iter().map(|m| m.path.clone()).collect();
        for name in names {
            let ext = path::extension(name);
            if ext != "tmx" && ext != "tmj" {
                continue;
            }
            let file_path = path::resolve(&dir, name);
            if listed.contains(&file_path) {
                continue;
            }
            for (re, p) in &self.regexes {
                let Some(c) = re.captures(name) else {
                    continue;
                };
                let capture = |i| {
                    c.get(i)
                        .map(|m| crate::godot_str::to_int(m.as_str()))
                        .unwrap_or(0)
                };
                self.pending.push(Pending {
                    path: file_path.clone(),
                    pos: (
                        capture(1) * p.multiplier_x + p.offset_x,
                        capture(2) * p.multiplier_y + p.offset_y,
                    ),
                    size: (p.map_width, p.map_height),
                });
                listed.insert(file_path);
                break;
            }
        }
    }

    /// Fills in the sizes of the pending maps from their files; false while one is missing.
    fn resolve_sizes(&mut self) -> bool {
        let waiting = self.pending.iter().any(|p| {
            self.answers
                .iter()
                .any(|(r, a)| matches!(a, Answer::Waiting) && r.path() == p.path)
        });
        if waiting {
            return false;
        }
        let pending = std::mem::take(&mut self.pending);
        let world_path = self.world_path.clone();
        for p in pending {
            let (mut w, mut h) = p.size;
            if w.is_none() || h.is_none() {
                let size = match self.take(&WorldRequest::File(p.path.clone())) {
                    Some(Answer::Bytes(b)) => parse::map(&b, &p.path).ok().map(|m| {
                        let (_, _, mw, mh) = m.map_rect();
                        (mw.round() as i64, mh.round() as i64)
                    }),
                    _ => None,
                };
                let world = self.world.as_mut().expect("world parsed");
                let Some((mw, mh)) = size else {
                    world.errors.push(Error::new(
                        ErrorKind::NotFound,
                        format!(
                            "ERROR: Map file '{}' of world '{world_path}' could not be read. -> Skipped",
                            p.path
                        ),
                    ));
                    continue;
                };
                w = w.or(Some(mw));
                h = h.or(Some(mh));
            }
            let world = self.world.as_mut().expect("world parsed");
            world.maps.push(WorldMap {
                path: p.path,
                rect: (p.pos.0, p.pos.1, w.unwrap_or(0), h.unwrap_or(0)),
            });
        }
        true
    }
}

/// A request answered from a zip archive: a member, or the members directly in a directory.
fn answer_from_zip(z: &mut zip::ZipArchive<Cursor<Vec<u8>>>, request: &WorldRequest) -> Answer {
    match request {
        WorldRequest::File(p) => match z.by_name(p) {
            Ok(mut f) if !f.is_dir() => {
                let mut out = Vec::with_capacity(f.size() as usize);
                match f.read_to_end(&mut out) {
                    Ok(_) => Answer::Bytes(out),
                    Err(_) => Answer::Missing,
                }
            }
            _ => Answer::Missing,
        },
        WorldRequest::ListDir(d) => {
            let dir = path::cleanup(d);
            let names = z
                .file_names()
                .filter(|n| !n.ends_with('/') && path::base_dir(n) == dir)
                .map(|n| n.rsplit('/').next().unwrap_or(n).to_string())
                .collect();
            Answer::Listing(names)
        }
    }
}

/// Loads a world from the file system (`std::fs`): for tools and tests.
pub fn load_world_from_fs(world_path: &str) -> Result<World, Vec<Error>> {
    WorldLoader::new(world_path).run(
        |p| std::fs::read(p).ok(),
        |d| {
            let dir = if d.is_empty() { "." } else { d };
            std::fs::read_dir(dir).ok().map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
        },
    )
}

impl Map {
    /// The map's extent in tiles, `(x, y, width, height)`: `(0, 0, width, height)` for a finite
    /// map; for an infinite map the union of the chunks of every tile layer (groups included),
    /// `(0, 0, 0, 0)` without chunks (`TilemapCreator.get_map_tile_rect`).
    pub fn tile_rect(&self) -> (i64, i64, i64, i64) {
        if !self.is_infinite() {
            return (0, 0, self.width.unwrap_or(0), self.height.unwrap_or(0));
        }
        let mut acc: Option<(i64, i64, i64, i64)> = None;
        for l in self.all_layers() {
            if l.kind_or_tile() != "tilelayer" {
                continue;
            }
            for c in l.chunks.iter().flatten() {
                let (x, y) = (c.x.unwrap_or(0), c.y.unwrap_or(0));
                let (x1, y1) = (x + c.width.unwrap_or(0), y + c.height.unwrap_or(0));
                acc = Some(match acc {
                    None => (x, y, x1, y1),
                    Some((ax, ay, ax1, ay1)) => (ax.min(x), ay.min(y), ax1.max(x1), ay1.max(y1)),
                });
            }
        }
        match acc {
            Some((x, y, x1, y1)) => (x, y, x1 - x, y1 - y),
            None => (0, 0, 0, 0),
        }
    }

    /// The map's bounds in pixels, `(x, y, width, height)`: [`Map::pixel_rect`] of
    /// [`Map::tile_rect`]. The GDScript runtime stores it as metadata `map_rect` on the root.
    pub fn map_rect(&self) -> (f64, f64, f64, f64) {
        self.pixel_rect(self.tile_rect())
    }

    /// A rect of tiles `(x0, y0, w, h)` of this map in pixels, the map's origin where the
    /// importer puts it (`TilemapCreator.get_pixel_rect`):
    /// - orthogonal: `(x0 * tw, y0 * th, w * tw, h * th)`;
    /// - isometric: the diamond's bounding box, `((x0 - y0 - h + H) * tw / 2, (x0 + y0) * th / 2,
    ///   (w + h) * tw / 2, (w + h) * th / 2)`, `H` the map's height in tiles;
    /// - staggered and hexagonal: Tiled's map size of `w` x `h` tiles (`HexagonalRenderer::mapSize`),
    ///   from the position of tile `(x0, y0)`.
    pub fn pixel_rect(&self, rect: (i64, i64, i64, i64)) -> (f64, f64, f64, f64) {
        let tw = self.tilewidth.unwrap_or(0);
        let th = self.tileheight.unwrap_or(0);
        let (rx, ry, rw, rh) = rect;
        let (x0, y0, w, h) = (rx as f64, ry as f64, rw as f64, rh as f64);
        let (twf, thf) = (tw as f64, th as f64);
        match self.orientation.as_deref().unwrap_or("orthogonal") {
            "isometric" => {
                let map_h = self.height.unwrap_or(0) as f64;
                (
                    (x0 - y0 - h + map_h) * twf / 2.0,
                    (x0 + y0) * thf / 2.0,
                    (w + h) * twf / 2.0,
                    (w + h) * thf / 2.0,
                )
            }
            o @ ("staggered" | "hexagonal") => {
                let stagger_x = self.staggeraxis.as_deref().unwrap_or("y") == "x";
                let side = if o == "hexagonal" {
                    self.hexsidelength.unwrap_or(0)
                } else {
                    0
                };
                let (tile_w, tile_h) = (tw & !1, th & !1);
                let side_x = if stagger_x { side } else { 0 };
                let side_y = if stagger_x { 0 } else { side };
                // GDScript's integer division truncates toward zero, as Rust's does
                let side_offset_x = (tile_w - side_x) / 2;
                let side_offset_y = (tile_h - side_y) / 2;
                let column_w = (side_offset_x + side_x) as f64;
                let row_h = (side_offset_y + side_y) as f64;
                if stagger_x {
                    let extra = if rw > 1 { row_h } else { 0.0 };
                    (
                        x0 * column_w,
                        y0 * (tile_h + side_y) as f64,
                        w * column_w + side_offset_x as f64,
                        h * (tile_h + side_y) as f64 + extra,
                    )
                } else {
                    let extra = if rh > 1 { column_w } else { 0.0 };
                    (
                        x0 * (tile_w + side_x) as f64,
                        y0 * row_h,
                        w * (tile_w + side_x) as f64 + extra,
                        h * row_h + side_offset_y as f64,
                    )
                }
            }
            _ => (x0 * twf, y0 * thf, w * twf, h * thf),
        }
    }
}
