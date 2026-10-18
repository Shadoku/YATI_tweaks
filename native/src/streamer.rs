//! `TiledStreamer`: a Node2D that streams a Tiled map's content in and out as a window moves,
//! within a per-frame budget (stage 4 of `docs/native-importer-plan.md`).
//!
//! Every map is a chunk source. Each Tiled tile layer is one persistent `TileMapLayer` (made by
//! the builder's own layer code, configured as `TiledBuilder.build` configures it); its cells are
//! cut into chunks of `chunk_size` (Tiled's 16 × 16 for an infinite map's chunks, a finite
//! layer's cells cut the same way), which enter the layer with `set_cell` and leave it with
//! `erase_cell`: the engine rebuilds only the quadrants a chunk touches (section 1.3). Objects are
//! indexed by the chunk their position lies in and created with it by the builder's object code
//! (`builder::stream`), freed when it leaves (unless `keep_objects_resident`). Payloads are
//! decoded when their chunk is wanted (an infinite map's), or once on a thread of their own (a
//! finite map's layers), so a map costs about its file size in memory until it is shown.
//!
//! The budget: the streamer measures what its work costs (µs per cell, per collision cell, per
//! y-sorted cell, per object, per erased cell, per freed object), starting from the first chunks,
//! and stops a frame's work before the next chunk would exceed `budget_us` (each frame does at
//! least one piece of work, so a budget smaller than one chunk still progresses). Every tile of
//! the map's TileSet is created before the first cell is set: a TileSet changed while layers hold
//! cells makes the engine rebuild every cell of every layer that uses it (measured: 40 to 60 ms
//! with 30 000 cells).
//!
//! World mode (`world`, a `TiledWorld`): every map of the world at its rect, loaded with
//! `TiledMap.load_async` when the window comes within `world_margin` of it, streamed as one map,
//! and unloaded chunk by chunk (then entirely) once the window is `keep_margin` farther away.

use crate::builder::stream::{layer_key, map_of, tileset_gids, Cell, Skeleton};
use crate::builder::{Build, Suspended};
use crate::map::TiledMap;
use crate::options::{options_of, TiledImportOptions};
use crate::tileset::{Built, TiledTileSets};
use crate::world::TiledWorld;
use godot::classes::{INode2D, Node, Node2D, TileMapLayer};
use godot::global::printerr;
use godot::prelude::*;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;
use tiled_core::data::GID_MASK;
use tiled_core::Map;

/// A chunk's coordinates (in chunks).
type Coord = (i32, i32);

/// The coordinates of an object item that stands for all of a layer's objects (`load_all`: they
/// are created in the layer's order, as `TiledBuilder.build` creates them).
const ALL: Coord = (i32::MIN, i32::MIN);

fn div_floor(a: i32, b: i32) -> i32 {
    a.div_euclid(b.max(1))
}

/// A cost the streamer measures: µs as a linear function of a piece of work's counts (the first
/// feature is 1: a fixed cost per piece), fitted by least squares over the samples so far, older
/// samples weighing less (a forgetting factor), and drawn toward the initial guess until samples
/// say otherwise (a ridge prior). The first chunks calibrate it.
#[derive(Clone, Copy, Debug)]
struct Fit<const K: usize> {
    prior: [f64; K],
    xx: [[f64; K]; K],
    xt: [f64; K],
    coef: [f64; K],
    samples: u32,
}

impl<const K: usize> Fit<K> {
    /// How much one sample's weight decays per later sample.
    const FORGET: f64 = 0.97;
    /// The prior's weight (in squared feature units: about one sample of the fixed cost).
    const RIDGE: f64 = 1.0;

    fn new(prior: [f64; K]) -> Self {
        Fit {
            prior,
            xx: [[0.0; K]; K],
            xt: [0.0; K],
            coef: prior,
            samples: 0,
        }
    }

    fn predict(&self, x: [f64; K]) -> f64 {
        x.iter()
            .zip(&self.coef)
            .map(|(a, b)| a * b)
            .sum::<f64>()
            .max(0.0)
    }

    fn add(&mut self, x: [f64; K], t: f64) {
        if !t.is_finite() || t < 0.0 {
            return;
        }
        self.samples += 1;
        for r in 0..K {
            self.xt[r] = self.xt[r] * Self::FORGET + x[r] * t;
            for c in 0..K {
                self.xx[r][c] = self.xx[r][c] * Self::FORGET + x[r] * x[c];
            }
        }
        // (XᵀX + λI) β = Xᵀt + λ prior, by Gaussian elimination with partial pivoting
        let mut a = self.xx;
        let mut b = self.xt;
        for k in 0..K {
            a[k][k] += Self::RIDGE;
            b[k] += Self::RIDGE * self.prior[k];
        }
        for col in 0..K {
            let piv = (col..K)
                .max_by(|&p, &q| a[p][col].abs().total_cmp(&a[q][col].abs()))
                .unwrap_or(col);
            a.swap(col, piv);
            b.swap(col, piv);
            if a[col][col].abs() < 1e-12 {
                return;
            }
            let pivot_row = a[col];
            let pivot_b = b[col];
            for (row, rb) in a.iter_mut().zip(b.iter_mut()).skip(col + 1) {
                let f = row[col] / pivot_row[col];
                for (v, p) in row.iter_mut().zip(pivot_row.iter()).skip(col) {
                    *v -= f * p;
                }
                *rb -= f * pivot_b;
            }
        }
        let mut beta = [0.0; K];
        for r in (0..K).rev() {
            let mut v = b[r];
            for c in r + 1..K {
                v -= a[r][c] * beta[c];
            }
            beta[r] = v / a[r][r];
        }
        // A negative cost is noise: none
        for v in &mut beta {
            *v = v.max(0.0);
        }
        self.coef = beta;
    }
}

/// The measured costs (the initial guesses are this machine's, section 1.3 of the plan).
#[derive(Clone, Copy, Debug)]
struct Costs {
    /// A chunk of a tile layer: [per chunk, per cell, per y-sorted cell, per collision cell].
    tiles: Fit<4>,
    /// Erasing a chunk: [per chunk, per cell, per y-sorted cell].
    erase: Fit<3>,
    /// A chunk's objects: [per chunk, per object].
    spawn: Fit<2>,
    /// Freeing a chunk's objects: [per chunk, per object].
    free: Fit<2>,
}

impl Default for Costs {
    fn default() -> Self {
        Costs {
            tiles: Fit::new([30.0, 1.8, 16.0, 7.0]),
            erase: Fit::new([20.0, 1.0, 7.0]),
            spawn: Fit::new([10.0, 40.0]),
            free: Fit::new([10.0, 15.0]),
        }
    }
}

/// Raw cell value (flags included) -> its resolution, `None` inside for a value no tileset has.
#[derive(Default)]
struct Table {
    dense: Vec<Option<Option<Cell>>>,
    sparse: HashMap<u32, Option<Cell>>,
}

impl Table {
    const DENSE_LIMIT: usize = 1 << 16;

    fn key(raw: u32) -> u32 {
        raw & 0xEFFF_FFFF
    }

    fn index(raw: u32) -> usize {
        ((raw & GID_MASK) as usize) * 8 + ((raw >> 29) & 7) as usize
    }

    fn get(&self, raw: u32) -> Option<Option<Cell>> {
        let i = Self::index(raw);
        if i < Self::DENSE_LIMIT {
            self.dense.get(i).copied().flatten()
        } else {
            self.sparse.get(&Self::key(raw)).copied()
        }
    }

    fn insert(&mut self, raw: u32, c: Option<Cell>) {
        let i = Self::index(raw);
        if i < Self::DENSE_LIMIT {
            if self.dense.len() <= i {
                self.dense.resize(i + 1, None);
            }
            self.dense[i] = Some(c);
        } else {
            self.sparse.insert(Self::key(raw), c);
        }
    }
}

/// Where a tile layer's cells come from: a rect of cells (map coordinates) and its payload, an
/// infinite layer's chunk (`Some(index)`) or a finite layer's `data` (`None`).
struct Src {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    chunk: Option<usize>,
}

/// One Tiled tile layer of a streamed map.
struct TileLayer {
    name: GString,
    node: Gd<TileMapLayer>,
    ysort: bool,
    /// The layer's index in `Map::all_layers()`.
    idx: usize,
    srcs: Vec<Src>,
    /// Chunk -> the sources that have cells in it.
    grid: HashMap<Coord, Vec<u32>>,
    /// Loaded chunks and which of their cells were set (a bit per cell, row by row).
    loaded: HashMap<Coord, Vec<u64>>,
    /// A finite layer's decoded cells (decoded on a thread of their own).
    data: Option<Arc<Vec<u32>>>,
    reported: bool,
}

/// One Tiled object layer of a streamed map.
struct ObjLayer {
    name: GString,
    node: Gd<Node2D>,
    idx: usize,
    /// `draworder="index"` on an orthogonal map: children kept in the layer's object order.
    ordered: bool,
    /// Chunk -> the indices of the objects whose position lies in it.
    grid: HashMap<Coord, Vec<u32>>,
    loaded: HashSet<Coord>,
    /// Object index -> its nodes.
    spawned: BTreeMap<u32, Vec<Gd<Node>>>,
}

/// Finite layers' cells, decoded on a thread: (layer index, cells or the error).
type Decoded = Vec<(usize, Result<Vec<u32>, String>)>;

/// A map whose tree exists: its layers, chunk grids and the suspended build that makes its
/// objects.
struct MapStream {
    map: Arc<Map>,
    root: Gd<Node2D>,
    build: Option<Suspended>,
    tile_size: Vector2i,
    iso: bool,
    tiles: Vec<TileLayer>,
    objs: Vec<ObjLayer>,
    table: Table,
    /// Gids whose tiles are still to be created (popped from the end).
    gids: Vec<u32>,
    decoding: Option<JoinHandle<Decoded>>,
    /// Object ids spawned or despawned in this frame: their references are resolved again.
    changed: HashSet<i64>,
}

impl MapStream {
    fn ready(&self) -> bool {
        self.gids.is_empty() && self.decoding.is_none()
    }

    fn is_empty(&self) -> bool {
        self.tiles.iter().all(|t| t.loaded.is_empty())
            && self.objs.iter().all(|o| o.spawned.is_empty())
    }
}

/// Where a map of the streamer stands.
enum Phase {
    /// Not loaded (a world map out of the window's reach).
    Idle,
    /// Loading: polled once per frame.
    Loading(Gd<TiledMap>),
    /// Loaded; its TileSets are being created, a few tiles per step.
    TileSets(Gd<TiledMap>),
    /// Its tree exists (`Box`: the map's state is large).
    Live(Box<MapStream>),
    /// It could not be loaded.
    Failed,
}

/// A map of the streamer: the one map, or one of the world's.
struct Slot {
    path: GString,
    /// Where the map's root goes (the world's rect origin), in pixels.
    origin: Vector2,
    /// The world's rect for the map, in pixels (`None`: the one map, always wanted).
    rect: Option<Rect2>,
    phase: Phase,
    /// The window left the map: its chunks unload, then the map goes.
    draining: bool,
}

/// What a frame streams in: everything there is, a rect of the map's cells (one map), or a rect
/// of world pixels (world mode).
#[derive(Clone, Copy)]
enum Win {
    All,
    Cells(Rect2i),
    Px(Rect2),
}

/// One piece of work.
#[derive(Clone, Copy)]
struct Item {
    slot: usize,
    tile: bool,
    layer: usize,
    coord: Coord,
    dist: f32,
}

/// What a frame did, for the signals emitted at its end.
enum Event {
    ChunkLoaded(GString, Vector2i),
    ChunkUnloaded(GString, Vector2i),
    Spawned(Gd<Node>, i64),
    Despawned(i64),
    MapLoaded(GString, Gd<Node2D>),
    MapUnloaded(GString),
    Settled,
}

/// Counters for benchmarks (`get_stats`).
#[derive(Default, Clone, Copy)]
struct Stats {
    frames: i64,
    work_frames: i64,
    budget_stops: i64,
    tile_chunks_loaded: i64,
    tile_chunks_unloaded: i64,
    objects_spawned: i64,
    objects_despawned: i64,
    cells_set: i64,
    collision_cells_set: i64,
    last_work_us: f64,
    max_work_us: f64,
    max_item_us: f64,
    max_overrun_us: f64,
}

/// Streams a Tiled map (infinite or finite) or a Tiled world into the scene, chunk by chunk, as a
/// window moves, within a per-frame budget.
///
/// ```gdscript
/// var map := TiledMap.new()
/// map.load("res://maps/overworld.tmx", "")
/// var streamer := TiledStreamer.new()
/// streamer.map = map
/// streamer.follow = $Player          # or: streamer.window = Rect2i(0, 0, 80, 45)
/// streamer.margin = Vector2i(48, 32) # cells around the followed node's chunk
/// add_child(streamer)
/// ```
///
/// Each frame (`_process`, or `step(budget_us)` by hand) loads the chunks newly inside the window
/// (nearest first), unloads those outside the window grown by `keep_margin`, and creates and frees
/// the objects of those chunks, stopping before the next chunk would exceed the budget. Signals
/// tell what happened; `settled` is emitted once whenever the window is fully loaded.
#[derive(GodotClass)]
#[class(base = Node2D)]
pub struct TiledStreamer {
    base: Base<Node2D>,
    /// The map to stream (a loaded `TiledMap`, or one loading asynchronously: it is polled).
    #[var(set = set_map)]
    map: Option<Gd<TiledMap>>,
    /// A world to stream instead of one map: every map placed at its rect.
    #[var(set = set_world)]
    world: Option<Gd<TiledWorld>>,
    /// The TileSet cache (`null`: `TiledTileSets.shared()`, the facade's shared one, set on first
    /// use).
    #[var]
    tilesets: Option<Gd<TiledTileSets>>,
    /// The import options (`null`: the defaults).
    #[var(set = set_options)]
    options: Option<Gd<TiledImportOptions>>,
    /// The project file the world's maps are loaded with ("" for none).
    #[export]
    project_file: GString,
    /// The window to stream, in cells (of the map; in world mode, of `world_cell_size`). Used
    /// when `follow` is not set.
    #[var(set = set_window)]
    window: Rect2i,
    /// A node whose position decides the window: its chunk grown by `margin`, recomputed every
    /// frame.
    #[var(set = set_follow)]
    follow: Option<Gd<Node2D>>,
    /// Follow mode: cells around the followed node's chunk that are streamed in.
    #[export]
    margin: Vector2i,
    /// Cells beyond the window a loaded chunk may be before it is unloaded (hysteresis, so a
    /// window moving back and forth across a chunk border does not reload it every time).
    #[export]
    keep_margin: Vector2i,
    /// World mode: how near (cells) the window must come to a map for it to start loading.
    #[export]
    world_margin: Vector2i,
    /// World mode: the size of a window cell in pixels.
    #[export]
    world_cell_size: Vector2i,
    /// Work per frame in µs (`<= 0`: unbounded).
    #[export]
    budget_us: i64,
    /// Objects are created with their chunk but never freed when it leaves (for objects that
    /// carry game state). A world map that leaves the window still frees its objects.
    #[export]
    keep_objects_resident: bool,
    /// The chunk size in cells (Tiled's 16 × 16 by default; a finite layer is cut the same way).
    #[var(set = set_chunk_size)]
    chunk_size: Vector2i,
    /// Stream the y-sorted tile layers of isometric and staggered maps. Off (the default), such a
    /// map is refused with an error, except by `load_all()`: the engine draws a y-sorted layer's
    /// cells one by one and re-sorts all of them in every frame that changes the layer, a cost
    /// that grows with the layer's live cells and lands after the streamer's work, where no budget
    /// reaches (measured with `test/bench_materialize.gd -- --ysort`: +4 ms over the idle frame for
    /// one 16 × 16 chunk into a layer of 3 500 cells, +8 ms into one of 87 000, on this machine's
    /// software renderer). On, their cells are budgeted at their own measured cost (about 8 times
    /// an ordinary cell's), and frames that change them cost that re-sort on top.
    #[export]
    allow_y_sorted_layers: bool,

    slots: Vec<Slot>,
    all: bool,
    settled: bool,
    costs: Costs,
    stats: Stats,
    events: Vec<Event>,
    /// TileSets this streamer holds, by cache key: a world's maps with the same tilesets share
    /// one (and creating its tiles for a second map changes nothing).
    held: HashMap<String, Weak<Built>>,
    errors: u32,
    warnings: u32,
}

#[godot_api]
impl INode2D for TiledStreamer {
    fn init(base: Base<Node2D>) -> Self {
        TiledStreamer {
            base,
            map: None,
            world: None,
            tilesets: None,
            options: None,
            project_file: GString::new(),
            window: Rect2i::default(),
            follow: None,
            margin: Vector2i::new(48, 32),
            keep_margin: Vector2i::new(16, 16),
            world_margin: Vector2i::new(32, 32),
            world_cell_size: Vector2i::new(16, 16),
            budget_us: 2000,
            keep_objects_resident: false,
            chunk_size: Vector2i::new(16, 16),
            allow_y_sorted_layers: false,
            slots: Vec::new(),
            all: false,
            settled: false,
            costs: Costs::default(),
            stats: Stats::default(),
            events: Vec::new(),
            held: HashMap::new(),
            errors: 0,
            warnings: 0,
        }
    }

    fn process(&mut self, _delta: f64) {
        let b = self.budget_us;
        self.step(b);
    }
}

#[godot_api]
impl TiledStreamer {
    /// A chunk of a tile layer entered its `TileMapLayer` (`coords` in chunks of the map).
    #[signal]
    fn chunk_loaded(layer_name: GString, coords: Vector2i);

    /// A chunk of a tile layer left its `TileMapLayer`.
    #[signal]
    fn chunk_unloaded(layer_name: GString, coords: Vector2i);

    /// An object's node was created (`id`: the Tiled object id; the node that stands for it).
    #[signal]
    fn object_spawned(node: Gd<Node>, id: i64);

    /// An object's nodes were freed.
    #[signal]
    fn object_despawned(id: i64);

    /// World mode: a map's tree was created (its layers still empty).
    #[signal]
    fn map_loaded(path: GString, root: Gd<Node2D>);

    /// World mode: a map left the window and its tree was freed.
    #[signal]
    fn map_unloaded(path: GString);

    /// The window is fully loaded: emitted by the frame whose work completed it (once per change
    /// of the window; not by frames with nothing to do).
    #[signal]
    fn settled();

    #[func]
    fn set_map(&mut self, map: Option<Gd<TiledMap>>) {
        self.map = map;
        self.world = None;
        self.reset();
    }

    #[func]
    fn set_world(&mut self, world: Option<Gd<TiledWorld>>) {
        self.world = world;
        self.map = None;
        self.reset();
    }

    #[func]
    fn set_options(&mut self, options: Option<Gd<TiledImportOptions>>) {
        self.options = options;
        self.reset();
    }

    #[func]
    fn set_window(&mut self, window: Rect2i) {
        self.window = window;
        self.all = false;
    }

    #[func]
    fn set_follow(&mut self, follow: Option<Gd<Node2D>>) {
        self.follow = follow;
        self.all = false;
    }

    #[func]
    fn set_chunk_size(&mut self, size: Vector2i) {
        self.chunk_size = Vector2i::new(size.x.max(1), size.y.max(1));
        self.reset();
    }

    /// One frame's work within `budget_us` (`<= 0`: unbounded, which also waits for a finite map's
    /// layers to be decoded, but not for a map still loading), then the signals. What `_process`
    /// calls with `budget_us`; call it by hand with processing off (tests, tools: `set_process(false)`
    /// once the streamer is in the tree, since entering it turns processing on). Returns
    /// `is_settled()`.
    #[func]
    pub fn step(&mut self, budget_us: i64) -> bool {
        let budget = if budget_us <= 0 {
            f64::INFINITY
        } else {
            budget_us as f64
        };
        self.frame(budget, false);
        self.emit_events();
        self.settled
    }

    /// Loads everything (an unbounded window and budget, in this call; a world's maps loaded
    /// synchronously): the map's whole tile and object content, as `TiledBuilder.build` makes
    /// it. The window stays unbounded until `window` or `follow` is set.
    #[func]
    fn load_all(&mut self) {
        self.all = true;
        for _ in 0..64 {
            self.frame(f64::INFINITY, true);
            if self.settled {
                break;
            }
        }
        self.emit_events();
    }

    /// Whether the window is fully loaded (nothing left to load or unload, no map loading).
    #[func]
    fn is_settled(&self) -> bool {
        self.settled
    }

    /// The number of loaded tile layer chunks (every layer, every map).
    #[func]
    fn loaded_chunk_count(&self) -> i64 {
        self.live()
            .map(|m| m.tiles.iter().map(|t| t.loaded.len()).sum::<usize>())
            .sum::<usize>() as i64
    }

    /// The loaded chunks of a tile layer (of the first map that has a layer of that name).
    #[func]
    fn get_loaded_chunks(&self, layer_name: GString) -> Array<Vector2i> {
        let mut out = Array::new();
        if let Some(t) = self.find_layer(&layer_name) {
            let mut v: Vec<Coord> = t.loaded.keys().copied().collect();
            v.sort();
            for (x, y) in v {
                out.push(Vector2i::new(x, y));
            }
        }
        out
    }

    /// The number of objects whose nodes exist.
    #[func]
    fn get_spawned_object_count(&self) -> i64 {
        self.live()
            .map(|m| m.objs.iter().map(|o| o.spawned.len()).sum::<usize>())
            .sum::<usize>() as i64
    }

    /// The `TileMapLayer` of a Tiled tile layer by name (the first map's that has one), or null.
    #[func]
    fn get_layer(&self, name: GString) -> Option<Gd<TileMapLayer>> {
        self.find_layer(&name).map(|t| t.node.clone())
    }

    /// The node of a Tiled object layer by name (the first map's that has one), or null.
    #[func]
    fn get_object_layer(&self, name: GString) -> Option<Gd<Node2D>> {
        self.live()
            .flat_map(|m| m.objs.iter())
            .find(|o| o.name == name)
            .map(|o| o.node.clone())
    }

    /// The root node of a streamed map (by path; "" for the one map), or null when its tree does
    /// not exist (a world map out of reach).
    #[func]
    fn get_map_root(&self, path: GString) -> Option<Gd<Node2D>> {
        self.slots
            .iter()
            .filter(|s| path.is_empty() || s.path == path)
            .find_map(|s| match &s.phase {
                Phase::Live(m) => Some(m.root.clone()),
                _ => None,
            })
    }

    /// The paths of the maps whose trees exist.
    #[func]
    fn get_loaded_maps(&self) -> PackedStringArray {
        self.slots
            .iter()
            .filter(|s| matches!(s.phase, Phase::Live(_)))
            .map(|s| s.path.clone())
            .collect()
    }

    /// The costs the streamer measured (fitted to its work so far), in µs: `chunk` (a tile
    /// chunk's fixed cost), `cell`, `ysort_cell` (a cell of a y-sorted layer, instead of `cell`),
    /// `collision_cell` (the extra of a cell whose tile has a collision polygon), `erase_chunk`,
    /// `erased_cell`, `erased_ysort_cell`, `objects_chunk`, `object`, `free_chunk`, `freed_object`; and the sample
    /// counts `tile_samples`, `erase_samples`, `object_samples`, `free_samples`.
    #[func]
    fn get_costs(&self) -> VarDictionary {
        let mut d = VarDictionary::new();
        let c = &self.costs;
        for (k, v) in [
            ("chunk", c.tiles.coef[0]),
            ("cell", c.tiles.coef[1]),
            ("ysort_cell", c.tiles.coef[2]),
            ("collision_cell", c.tiles.coef[3]),
            ("erase_chunk", c.erase.coef[0]),
            ("erased_cell", c.erase.coef[1]),
            ("erased_ysort_cell", c.erase.coef[2]),
            ("objects_chunk", c.spawn.coef[0]),
            ("object", c.spawn.coef[1]),
            ("free_chunk", c.free.coef[0]),
            ("freed_object", c.free.coef[1]),
        ] {
            d.set(k, v);
        }
        for (k, n) in [
            ("tile_samples", c.tiles.samples),
            ("erase_samples", c.erase.samples),
            ("object_samples", c.spawn.samples),
            ("free_samples", c.free.samples),
        ] {
            d.set(k, n as i64);
        }
        d
    }

    /// Counters since the streamer was made (or `reset_stats`): `frames`, `work_frames`,
    /// `budget_stops` (frames whose work stopped at the budget), `tile_chunks_loaded`,
    /// `tile_chunks_unloaded`, `objects_spawned`, `objects_despawned`, `cells_set`,
    /// `collision_cells_set`, `last_work_us`, `max_work_us`, `max_item_us` (the longest piece of
    /// work), `max_overrun_us` (the most a piece of work took beyond its estimate).
    #[func]
    fn get_stats(&self) -> VarDictionary {
        let s = self.stats;
        let mut d = VarDictionary::new();
        d.set("frames", s.frames);
        d.set("work_frames", s.work_frames);
        d.set("budget_stops", s.budget_stops);
        d.set("tile_chunks_loaded", s.tile_chunks_loaded);
        d.set("tile_chunks_unloaded", s.tile_chunks_unloaded);
        d.set("objects_spawned", s.objects_spawned);
        d.set("objects_despawned", s.objects_despawned);
        d.set("cells_set", s.cells_set);
        d.set("collision_cells_set", s.collision_cells_set);
        d.set("last_work_us", s.last_work_us);
        d.set("max_work_us", s.max_work_us);
        d.set("max_item_us", s.max_item_us);
        d.set("max_overrun_us", s.max_overrun_us);
        d
    }

    #[func]
    fn reset_stats(&mut self) {
        self.stats = Stats::default();
    }

    /// Errors printed so far (loading, TileSets, objects).
    #[func]
    fn get_error_count(&self) -> i64 {
        self.errors as i64
    }

    /// Warnings printed so far.
    #[func]
    fn get_warning_count(&self) -> i64 {
        self.warnings as i64
    }
}

impl TiledStreamer {
    fn tilesets_gd(&mut self) -> Gd<TiledTileSets> {
        if self.tilesets.is_none() {
            self.tilesets = Some(TiledTileSets::shared_instance());
        }
        self.tilesets.clone().expect("set above")
    }

    fn live(&self) -> impl Iterator<Item = &MapStream> {
        self.slots.iter().filter_map(|s| match &s.phase {
            Phase::Live(m) => Some(&**m),
            _ => None,
        })
    }

    fn find_layer(&self, name: &GString) -> Option<&TileLayer> {
        self.live()
            .flat_map(|m| m.tiles.iter())
            .find(|t| &t.name == name)
    }

    /// Forgets every map's tree and state (the map, world, options or chunk size changed).
    fn reset(&mut self) {
        for s in std::mem::take(&mut self.slots) {
            if let Phase::Live(m) = s.phase {
                let root = m.root.clone();
                drop(m);
                if root.is_instance_valid() {
                    root.free();
                }
            } else if let Phase::Loading(mut t) = s.phase {
                t.bind_mut().cancel();
            }
        }
        self.held.clear();
        self.settled = false;
    }

    /// The slots for the map or the world, made on first use.
    fn ensure_slots(&mut self) {
        if !self.slots.is_empty() {
            return;
        }
        if let Some(m) = self.map.clone() {
            let path = m.bind().get_path_str();
            self.slots.push(Slot {
                path,
                origin: Vector2::ZERO,
                rect: None,
                phase: Phase::Loading(m),
                draining: false,
            });
        } else if let Some(w) = self.world.clone() {
            for (path, (x, y, wi, h)) in w.bind().map_rects() {
                self.slots.push(Slot {
                    path: GString::from(path.as_str()),
                    origin: Vector2::new(x as f32, y as f32),
                    rect: Some(Rect2::new(
                        Vector2::new(x as f32, y as f32),
                        Vector2::new(wi as f32, h as f32),
                    )),
                    phase: Phase::Idle,
                    draining: false,
                });
            }
        }
    }

    fn is_world(&self) -> bool {
        self.world.is_some()
    }

    /// The followed node's position in the streamer's coordinates.
    fn follow_position(&self) -> Option<Vector2> {
        let f = self.follow.as_ref().filter(|f| f.is_instance_valid())?;
        let me = self.base();
        if f.is_inside_tree() && me.is_inside_tree() {
            Some(me.to_local(f.get_global_position()))
        } else {
            Some(f.get_position())
        }
    }

    /// A position (streamer coordinates) as a cell of the one map.
    fn map_cell_at(&self, p: Vector2) -> Vector2i {
        if let Some(Phase::Live(m)) = self.slots.first().map(|s| &s.phase) {
            if let Some(t) = m.tiles.first() {
                let n = &t.node;
                let local = if n.is_inside_tree() && self.base().is_inside_tree() {
                    n.to_local(self.base().to_global(p))
                } else {
                    p - n.get_position()
                };
                return n.local_to_map(local);
            }
            let ts = m.tile_size;
            let d = if m.iso { ts.y } else { ts.x };
            return Vector2i::new(
                (p.x / d.max(1) as f32).floor() as i32,
                (p.y / ts.y.max(1) as f32).floor() as i32,
            );
        }
        let ts = self
            .map
            .as_ref()
            .map(|m| m.bind().tile_size())
            .unwrap_or(Vector2i::new(16, 16));
        Vector2i::new(
            (p.x / ts.x.max(1) as f32).floor() as i32,
            (p.y / ts.y.max(1) as f32).floor() as i32,
        )
    }

    /// The cell rect of a chunk grown by `margin` (follow mode).
    fn around(&self, cell: Vector2i) -> Rect2i {
        let cs = self.chunk_size;
        let c = Vector2i::new(
            div_floor(cell.x, cs.x) * cs.x,
            div_floor(cell.y, cs.y) * cs.y,
        );
        let m = Vector2i::new(self.margin.x.max(0), self.margin.y.max(0));
        Rect2i::new(c - m, cs + m * 2)
    }

    fn window_now(&self) -> Win {
        if self.all {
            return Win::All;
        }
        let wcs = self.world_cell_size;
        let wcs_f = Vector2::new(wcs.x.max(1) as f32, wcs.y.max(1) as f32);
        let cells = match self.follow_position() {
            Some(p) if self.is_world() => {
                let cell = Vector2i::new(
                    (p.x / wcs_f.x).floor() as i32,
                    (p.y / wcs_f.y).floor() as i32,
                );
                self.around(cell)
            }
            Some(p) => self.around(self.map_cell_at(p)),
            None => self.window,
        };
        if self.is_world() {
            let pos = Vector2::new(cells.position.x as f32, cells.position.y as f32) * wcs_f;
            let size = Vector2::new(cells.size.x as f32, cells.size.y as f32) * wcs_f;
            Win::Px(Rect2::new(pos, size))
        } else {
            Win::Cells(cells)
        }
    }

    /// A window grown by cells (world mode: cells of `world_cell_size`).
    fn grown(&self, w: Win, by: Vector2i) -> Win {
        let by = Vector2i::new(by.x.max(0), by.y.max(0));
        match w {
            Win::All => Win::All,
            Win::Cells(r) => Win::Cells(Rect2i::new(r.position - by, r.size + by * 2)),
            Win::Px(r) => {
                let d = Vector2::new(
                    (by.x * self.world_cell_size.x) as f32,
                    (by.y * self.world_cell_size.y) as f32,
                );
                Win::Px(Rect2::new(r.position - d, r.size + d * 2.0))
            }
        }
    }

    /// A window as a range of a map's chunks `[x0, x1) × [y0, y1)`; `None` for everything.
    fn chunk_range(&self, w: Win, m: &MapStream, origin: Vector2) -> Option<(i32, i32, i32, i32)> {
        let cells = match w {
            Win::All => return None,
            Win::Cells(r) => r,
            Win::Px(r) => {
                let ts = m.tile_size;
                let tw = if m.iso { ts.y } else { ts.x }.max(1) as f32;
                let th = ts.y.max(1) as f32;
                let p = r.position - origin;
                let x0 = (p.x / tw).floor() as i32;
                let y0 = (p.y / th).floor() as i32;
                let x1 = ((p.x + r.size.x) / tw).ceil() as i32;
                let y1 = ((p.y + r.size.y) / th).ceil() as i32;
                Rect2i::new(Vector2i::new(x0, y0), Vector2i::new(x1 - x0, y1 - y0))
            }
        };
        let cs = self.chunk_size;
        if cells.size.x <= 0 || cells.size.y <= 0 {
            return Some((0, 0, 0, 0));
        }
        let x0 = div_floor(cells.position.x, cs.x);
        let y0 = div_floor(cells.position.y, cs.y);
        let x1 = div_floor(cells.position.x + cells.size.x - 1, cs.x) + 1;
        let y1 = div_floor(cells.position.y + cells.size.y - 1, cs.y) + 1;
        Some((x0, y0, x1, y1))
    }

    /// The window's centre in pixels of the streamer (for nearest-first ordering).
    fn centre_px(&self, w: Win) -> Vector2 {
        match w {
            Win::All => Vector2::ZERO,
            Win::Px(r) => r.position + r.size * 0.5,
            Win::Cells(r) => {
                let ts = self
                    .live()
                    .next()
                    .map(|m| m.tile_size)
                    .unwrap_or(Vector2i::new(16, 16));
                Vector2::new(
                    (r.position.x as f32 + r.size.x as f32 * 0.5) * ts.x as f32,
                    (r.position.y as f32 + r.size.y as f32 * 0.5) * ts.y as f32,
                )
            }
        }
    }

    fn count(&mut self, d: crate::diag::Diag) {
        self.errors += d.errors;
        self.warnings += d.warnings;
    }

    fn error(&mut self, msg: String) {
        printerr(&[msg.to_variant()]);
        self.errors += 1;
    }

    /// One frame: the window, the maps' phases, then chunk and object work within the budget.
    fn frame(&mut self, budget: f64, sync: bool) {
        let start = Instant::now();
        let spent = |s: &Instant| s.elapsed().as_secs_f64() * 1e6;
        self.stats.frames += 1;
        self.ensure_slots();
        let win = self.window_now();
        let mut ts_gd = self.tilesets_gd();
        let mut ts = ts_gd.bind_mut();

        // World mode: maps the window approaches start loading, maps it left drain
        if self.is_world() {
            let near = self.grown(win, self.world_margin);
            let far = self.grown(win, self.world_margin + self.keep_margin);
            let touches = |w: Win, r: Rect2| match w {
                Win::All => true,
                Win::Px(p) => p.intersects(r),
                Win::Cells(_) => true,
            };
            let project = self.project_file.clone();
            for s in &mut self.slots {
                let Some(rect) = s.rect else { continue };
                let is_near = touches(near, rect);
                let is_far = !touches(far, rect);
                match &mut s.phase {
                    Phase::Idle if is_near => {
                        let mut tm = TiledMap::new_gd();
                        if sync {
                            tm.bind_mut().load(s.path.clone(), project.clone());
                        } else {
                            tm.bind_mut().load_async(s.path.clone(), project.clone());
                        }
                        s.phase = Phase::Loading(tm);
                        s.draining = false;
                    }
                    Phase::Loading(tm) if is_far => {
                        tm.bind_mut().cancel();
                        s.phase = Phase::Idle;
                    }
                    Phase::TileSets(_) if is_far => s.phase = Phase::Idle,
                    Phase::Live(_) => {
                        if is_far {
                            s.draining = true;
                        } else if is_near {
                            s.draining = false;
                        }
                    }
                    _ => {}
                }
            }
        }

        // Phases: loading, TileSets, the tree, tiles created and finite layers decoded
        let before_phases = spent(&start);
        for si in 0..self.slots.len() {
            self.advance(si, &mut ts, budget, &start, sync);
        }
        // Work done by the phases counts as the frame's first piece of work
        let phase_work = spent(&start) - before_phases > 100.0;

        // The work: chunks to load (nearest first) and to unload (farthest first)
        let centre = self.centre_px(win);
        let keep = self.grown(win, self.keep_margin);
        let mut loads: Vec<Item> = Vec::new();
        let mut unloads: Vec<Item> = Vec::new();
        let mut pending_phase = false;
        for (si, s) in self.slots.iter().enumerate() {
            let m = match &s.phase {
                Phase::Live(m) if m.ready() => m,
                Phase::Idle | Phase::Failed => continue,
                _ => {
                    pending_phase = true;
                    continue;
                }
            };
            let cs = self.chunk_size;
            let dist = |c: Coord| -> f32 {
                let ts = m.tile_size;
                let px = s.origin
                    + Vector2::new(
                        ((c.0 as f32 + 0.5) * cs.x as f32) * ts.x as f32,
                        ((c.1 as f32 + 0.5) * cs.y as f32) * ts.y as f32,
                    );
                (px - centre).length()
            };
            let load_range = if s.draining {
                Some((0, 0, 0, 0))
            } else {
                self.chunk_range(win, m, s.origin)
            };
            let keep_range = if s.draining {
                Some((0, 0, 0, 0))
            } else {
                self.chunk_range(keep, m, s.origin)
            };
            let inside = |r: Option<(i32, i32, i32, i32)>, c: Coord| match r {
                None => true,
                Some((x0, y0, x1, y1)) => c.0 >= x0 && c.0 < x1 && c.1 >= y0 && c.1 < y1,
            };
            let wanted = |grid_len: usize, r: Option<(i32, i32, i32, i32)>| -> Option<Vec<Coord>> {
                // Iterate the range when it is smaller than the grid, else the grid
                let (x0, y0, x1, y1) = r?;
                let area = (x1 - x0).max(0) as i64 * (y1 - y0).max(0) as i64;
                (area <= grid_len as i64 * 2).then(|| {
                    let mut v = Vec::with_capacity(area as usize);
                    for y in y0..y1 {
                        for x in x0..x1 {
                            v.push((x, y));
                        }
                    }
                    v
                })
            };
            for (li, t) in m.tiles.iter().enumerate() {
                let add = |c: Coord| {
                    if !t.loaded.contains_key(&c) {
                        loads.push(Item {
                            slot: si,
                            tile: true,
                            layer: li,
                            coord: c,
                            dist: dist(c),
                        });
                    }
                };
                match wanted(t.grid.len(), load_range) {
                    Some(v) => v
                        .into_iter()
                        .filter(|c| t.grid.contains_key(c))
                        .for_each(add),
                    None => t
                        .grid
                        .keys()
                        .copied()
                        .filter(|&c| inside(load_range, c))
                        .for_each(add),
                }
                for &c in t.loaded.keys() {
                    if !inside(keep_range, c) {
                        unloads.push(Item {
                            slot: si,
                            tile: true,
                            layer: li,
                            coord: c,
                            dist: dist(c),
                        });
                    }
                }
            }
            for (li, o) in m.objs.iter().enumerate() {
                if load_range.is_none() {
                    if o.grid.keys().any(|c| !o.loaded.contains(c)) {
                        loads.push(Item {
                            slot: si,
                            tile: false,
                            layer: li,
                            coord: ALL,
                            dist: 0.0,
                        });
                    }
                    continue;
                }
                let add = |c: Coord| {
                    if !o.loaded.contains(&c) {
                        loads.push(Item {
                            slot: si,
                            tile: false,
                            layer: li,
                            coord: c,
                            dist: dist(c),
                        });
                    }
                };
                match wanted(o.grid.len(), load_range) {
                    Some(v) => v
                        .into_iter()
                        .filter(|c| o.grid.contains_key(c))
                        .for_each(add),
                    None => o
                        .grid
                        .keys()
                        .copied()
                        .filter(|&c| inside(load_range, c))
                        .for_each(add),
                }
                if !self.keep_objects_resident || s.draining {
                    for &c in &o.loaded {
                        if !inside(keep_range, c) {
                            unloads.push(Item {
                                slot: si,
                                tile: false,
                                layer: li,
                                coord: c,
                                dist: dist(c),
                            });
                        }
                    }
                }
            }
        }
        if matches!(win, Win::All) {
            // Everything: tile layers, then objects in the map's order (as the one-shot build)
            loads.sort_by_key(|i| (i.slot, !i.tile, i.layer, i.coord.1, i.coord.0));
        } else {
            loads.sort_by(|a, b| a.dist.total_cmp(&b.dist));
        }
        unloads.sort_by(|a, b| b.dist.total_cmp(&a.dist));

        let mut first = !phase_work;
        let mut stopped = false;
        let mut worked = phase_work;
        let mut unloads = unloads.into_iter().peekable();
        // One unload first (so unloading never starves while loads fill the budget), the loads,
        // then the remaining unloads
        let mut queue: Vec<Item> = Vec::with_capacity(loads.len() + 1);
        if let Some(u) = unloads.next() {
            queue.push(u);
        }
        queue.extend(loads);
        queue.extend(unloads);
        for item in queue {
            let used = spent(&start);
            let remaining = if first { f64::INFINITY } else { budget - used };
            let done = self.work(&mut ts, item, remaining);
            if done {
                first = false;
                worked = true;
            } else {
                stopped = true;
                break;
            }
        }

        // References and hooks of the objects that came and went
        for s in &mut self.slots {
            if let Phase::Live(m) = &mut s.phase {
                if m.changed.is_empty() {
                    continue;
                }
                let changed = std::mem::take(&mut m.changed);
                let map = m.map.clone();
                let mut b = Build::resume(&map, &mut ts, m.build.take().expect("build"));
                b.update_refs(&changed);
                b.run_hooks();
                let d = b.take_diag();
                m.build = Some(b.suspend());
                self.errors += d.errors;
                self.warnings += d.warnings;
            }
        }
        drop(ts);

        // Drained world maps go
        for si in 0..self.slots.len() {
            let gone = matches!(&self.slots[si].phase, Phase::Live(m) if self.slots[si].draining && m.is_empty());
            if gone {
                let s = &mut self.slots[si];
                if let Phase::Live(m) = std::mem::replace(&mut s.phase, Phase::Idle) {
                    let root = m.root.clone();
                    drop(m);
                    if root.is_instance_valid() {
                        root.free();
                    }
                }
                s.draining = false;
                let path = s.path.clone();
                self.events.push(Event::MapUnloaded(path));
            }
        }

        let used = spent(&start);
        if worked {
            self.stats.work_frames += 1;
            self.stats.last_work_us = used;
            self.stats.max_work_us = self.stats.max_work_us.max(used);
        } else {
            self.stats.last_work_us = 0.0;
        }
        if stopped {
            self.stats.budget_stops += 1;
        }
        let draining = self
            .slots
            .iter()
            .any(|s| s.draining && matches!(s.phase, Phase::Live(_)));
        let now_settled = !stopped && !pending_phase && !draining;
        // Settled again: after unsettled frames, or after a frame whose work completed the window
        if now_settled && (!self.settled || worked) {
            self.events.push(Event::Settled);
        }
        self.settled = now_settled;
    }

    /// Advances a map's phase as far as the budget allows (`sync`: all the way).
    fn advance(
        &mut self,
        si: usize,
        ts: &mut TiledTileSets,
        budget: f64,
        start: &Instant,
        sync: bool,
    ) {
        let over = |s: &Instant| s.elapsed().as_secs_f64() * 1e6 > budget;
        loop {
            let phase = std::mem::replace(&mut self.slots[si].phase, Phase::Idle);
            let next = match phase {
                Phase::Loading(mut tm) => {
                    if tm.bind().inner().is_some() {
                        Phase::TileSets(tm)
                    } else {
                        let mut status = tm.bind().get_status();
                        if status == TiledMap::FAILED {
                            let path = self.slots[si].path.clone();
                            self.error(format!(
                                "ERROR: TiledStreamer: map '{path}' is not loaded (load it, or load_async it, first). -> Skipped"
                            ));
                            self.slots[si].phase = Phase::Failed;
                            return;
                        }
                        loop {
                            status = tm.bind_mut().poll();
                            if !sync || status == TiledMap::READY || status == TiledMap::FAILED {
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(1));
                        }
                        match status {
                            TiledMap::READY => Phase::TileSets(tm),
                            TiledMap::FAILED => {
                                self.errors += 1;
                                Phase::Failed
                            }
                            _ => {
                                self.slots[si].phase = Phase::Loading(tm);
                                return;
                            }
                        }
                    }
                }
                Phase::TileSets(tm) => {
                    let Some((map, zip)) = map_of(&tm) else {
                        self.slots[si].phase = Phase::Failed;
                        return;
                    };
                    let orientation = map.orientation.as_deref().unwrap_or("orthogonal");
                    let y_sorted = orientation == "isometric" || orientation == "staggered";
                    let has_tiles = map
                        .all_layers()
                        .iter()
                        .any(|l| l.kind_or_tile() == "tilelayer");
                    if y_sorted && has_tiles && !self.allow_y_sorted_layers && !self.all {
                        let path = self.slots[si].path.clone();
                        self.error(format!(
                            "ERROR: TiledStreamer: map '{path}' is {orientation}: its tile layers are y-sorted, and the engine re-sorts every cell of such a layer in each frame that changes it, beyond any budget. -> Not streamed. Set allow_y_sorted_layers to stream it anyway, use load_all(), or import it with TiledBuilder (Importer.import)."
                        ));
                        self.slots[si].phase = Phase::Failed;
                        return;
                    }
                    let opts = options_of(self.options.clone());
                    let key = TiledTileSets::map_key(&map, &zip, &opts);
                    let held = self.held.get(&key).and_then(Weak::upgrade);
                    if held.is_none() {
                        let mut done = false;
                        while !done {
                            done = ts.build_step(tm.clone(), self.options.clone(), 32);
                            if !sync && over(start) {
                                break;
                            }
                        }
                        if !done {
                            self.slots[si].phase = Phase::TileSets(tm);
                            return;
                        }
                    }
                    let ms = self.make_stream(ts, &map, &zip, held, key);
                    let root = ms.root.clone();
                    let s = &self.slots[si];
                    let mut r = root.clone();
                    r.set_position(s.origin);
                    let path = s.path.clone();
                    self.base_mut().add_child(&root);
                    self.events.push(Event::MapLoaded(path, root));
                    Phase::Live(Box::new(ms))
                }
                Phase::Live(mut m) => {
                    if !m.ready() {
                        self.prepare(&mut m, ts, budget, start, sync);
                    }
                    self.slots[si].phase = Phase::Live(m);
                    return;
                }
                other => {
                    self.slots[si].phase = other;
                    return;
                }
            };
            self.slots[si].phase = next;
            if !sync && over(start) {
                return;
            }
        }
    }

    /// The map's tree (layers empty), its chunk and object grids, its finite layers' decoding
    /// started.
    fn make_stream(
        &mut self,
        ts: &mut TiledTileSets,
        map: &Arc<Map>,
        zip: &str,
        held: Option<Rc<Built>>,
        key: String,
    ) -> MapStream {
        let opts = options_of(self.options.clone());
        let (root, skel, mut susp) = Build::for_stream(map, zip, ts, opts).run_skeleton(held);
        let built = {
            let mut b = Build::resume(map, ts, susp);
            let d = b.take_diag();
            self.count(d);
            let built = b.built();
            susp = b.suspend();
            built
        };
        self.held.insert(key, Rc::downgrade(&built));
        let Skeleton {
            tile_layers,
            object_layers,
        } = skel;
        let all = map.all_layers();
        let index_of = |k: usize| all.iter().position(|l| layer_key(l) == k);
        let orientation = map.orientation.as_deref().unwrap_or("orthogonal");
        let iso = orientation == "isometric";
        let sorted = iso || orientation == "staggered";
        let tile_size = Vector2i::new(
            map.tilewidth.unwrap_or(16) as i32,
            map.tileheight.unwrap_or(16) as i32,
        );
        let cs = self.chunk_size;
        let mut tiles = Vec::new();
        let mut finite = Vec::new();
        for (k, node) in tile_layers {
            let Some(idx) = index_of(k) else { continue };
            let l = all[idx];
            let mut srcs = Vec::new();
            if map.is_infinite() && l.chunks.is_some() {
                for (ci, c) in l.chunks.iter().flatten().enumerate() {
                    srcs.push(Src {
                        x: c.x.unwrap_or(0) as i32,
                        y: c.y.unwrap_or(0) as i32,
                        w: c.width.unwrap_or(0).max(0) as i32,
                        h: c.height.unwrap_or(0).max(0) as i32,
                        chunk: Some(ci),
                    });
                }
            } else if l.data.is_some() {
                srcs.push(Src {
                    x: 0,
                    y: 0,
                    w: map.width.unwrap_or(0).max(0) as i32,
                    h: map.height.unwrap_or(0).max(0) as i32,
                    chunk: None,
                });
                finite.push(idx);
            }
            let mut grid: HashMap<Coord, Vec<u32>> = HashMap::new();
            for (i, s) in srcs.iter().enumerate() {
                if s.w <= 0 || s.h <= 0 {
                    continue;
                }
                for cy in div_floor(s.y, cs.y)..=div_floor(s.y + s.h - 1, cs.y) {
                    for cx in div_floor(s.x, cs.x)..=div_floor(s.x + s.w - 1, cs.x) {
                        grid.entry((cx, cy)).or_default().push(i as u32);
                    }
                }
            }
            tiles.push(TileLayer {
                name: GString::from(node.get_name().to_string().as_str()),
                ysort: node.is_y_sort_enabled(),
                node,
                idx,
                srcs,
                grid,
                loaded: HashMap::new(),
                data: None,
                reported: false,
            });
        }
        let mut objs = Vec::new();
        for (k, node) in object_layers {
            let Some(idx) = index_of(k) else { continue };
            let l = all[idx];
            let mut grid: HashMap<Coord, Vec<u32>> = HashMap::new();
            let (tw, th) = (tile_size.x.max(1) as f64, tile_size.y.max(1) as f64);
            for (i, o) in l.objects.iter().flatten().enumerate() {
                let x = o.x.map(|n| n.as_f64()).unwrap_or(0.0);
                let y = o.y.map(|n| n.as_f64()).unwrap_or(0.0);
                let cx = (x / if iso { th } else { tw }).floor() as i32;
                let cy = (y / th).floor() as i32;
                grid.entry((div_floor(cx, cs.x), div_floor(cy, cs.y)))
                    .or_default()
                    .push(i as u32);
            }
            let ordered = !sorted && l.draworder.as_deref() == Some("index");
            objs.push(ObjLayer {
                name: GString::from(node.get_name().to_string().as_str()),
                node,
                idx,
                ordered,
                grid,
                loaded: HashSet::new(),
                spawned: BTreeMap::new(),
            });
        }
        let decoding = (!finite.is_empty()).then(|| {
            let m = map.clone();
            let job = move || {
                let all = m.all_layers();
                finite
                    .into_iter()
                    .map(|i| {
                        let r = match all[i].decode() {
                            Some(Ok(c)) => Ok(c.into_owned()),
                            Some(Err(e)) => Err(e.message),
                            None => Ok(Vec::new()),
                        };
                        (i, r)
                    })
                    .collect::<Decoded>()
            };
            std::thread::Builder::new()
                .name("tiled-decode".into())
                .spawn(job)
                .expect("a decoding thread")
        });
        let mut gids = tileset_gids(map);
        gids.reverse();
        MapStream {
            map: map.clone(),
            root,
            build: Some(susp),
            tile_size,
            iso,
            tiles,
            objs,
            table: Table::default(),
            gids,
            decoding,
            changed: HashSet::new(),
        }
    }

    /// Creates the map's tiles (budgeted) and collects its decoded finite layers.
    fn prepare(
        &mut self,
        m: &mut MapStream,
        ts: &mut TiledTileSets,
        budget: f64,
        start: &Instant,
        sync: bool,
    ) {
        if !m.gids.is_empty() {
            let map = m.map.clone();
            let mut b = Build::resume(&map, ts, m.build.take().expect("build"));
            while let Some(gid) = m.gids.pop() {
                let c = b.resolve_value(gid);
                m.table.insert(gid, c);
                if !sync && start.elapsed().as_secs_f64() * 1e6 > budget {
                    break;
                }
            }
            let d = b.take_diag();
            m.build = Some(b.suspend());
            self.count(d);
        }
        // An unbounded step waits for the decoding (a few ms at most), a budgeted one does not
        let wait = sync || budget.is_infinite();
        let finished = m.decoding.as_ref().is_some_and(|h| wait || h.is_finished());
        if finished {
            let h = m.decoding.take().expect("checked");
            match h.join() {
                Ok(decoded) => {
                    for (idx, r) in decoded {
                        let Some(t) = m.tiles.iter_mut().find(|t| t.idx == idx) else {
                            continue;
                        };
                        match r {
                            Ok(cells) => t.data = Some(Arc::new(cells)),
                            Err(e) => self.error(e),
                        }
                    }
                }
                Err(_) => {
                    self.error("ERROR: TiledStreamer: decoding the map's layers failed.".into())
                }
            }
        }
    }

    /// One piece of work if its estimate fits `remaining` µs; false when it does not (nothing
    /// done). The item functions return the estimate of what they did (`None`: not done).
    fn work(&mut self, ts: &mut TiledTileSets, item: Item, remaining: f64) -> bool {
        let cs = self.chunk_size;
        let Phase::Live(m) = &mut self.slots[item.slot].phase else {
            return true;
        };
        let m: &mut MapStream = m;
        let costs = &mut self.costs;
        let stats = &mut self.stats;
        let events = &mut self.events;
        let mut diag = crate::diag::Diag::default();
        let c = item.coord;
        let loading = if item.tile {
            !m.tiles[item.layer].loaded.contains_key(&c)
        } else {
            c == ALL || !m.objs[item.layer].loaded.contains(&c)
        };
        let t0 = Instant::now();
        let predicted = if item.tile && loading {
            load_tiles(
                m, ts, item.layer, c, cs, remaining, costs, stats, events, &mut diag,
            )
        } else if item.tile {
            unload_tiles(m, item.layer, c, cs, remaining, costs, stats, events)
        } else if loading {
            spawn_objects(
                m, ts, item.layer, c, remaining, costs, stats, events, &mut diag,
            )
        } else {
            despawn_objects(
                m, ts, item.layer, c, remaining, costs, stats, events, &mut diag,
            )
        };
        let us = t0.elapsed().as_secs_f64() * 1e6;
        self.count(diag);
        match predicted {
            Some(p) => {
                self.stats.max_item_us = self.stats.max_item_us.max(us);
                self.stats.max_overrun_us = self.stats.max_overrun_us.max(us - p);
                true
            }
            None => false,
        }
    }

    fn emit_events(&mut self) {
        let events = std::mem::take(&mut self.events);
        for e in events {
            match e {
                Event::ChunkLoaded(n, c) => {
                    self.base_mut()
                        .emit_signal("chunk_loaded", &[n.to_variant(), c.to_variant()]);
                }
                Event::ChunkUnloaded(n, c) => {
                    self.base_mut()
                        .emit_signal("chunk_unloaded", &[n.to_variant(), c.to_variant()]);
                }
                Event::Spawned(node, id) => {
                    if node.is_instance_valid() {
                        self.base_mut()
                            .emit_signal("object_spawned", &[node.to_variant(), id.to_variant()]);
                    }
                }
                Event::Despawned(id) => {
                    self.base_mut()
                        .emit_signal("object_despawned", &[id.to_variant()]);
                }
                Event::MapLoaded(p, r) => {
                    self.base_mut()
                        .emit_signal("map_loaded", &[p.to_variant(), r.to_variant()]);
                }
                Event::MapUnloaded(p) => {
                    self.base_mut()
                        .emit_signal("map_unloaded", &[p.to_variant()]);
                }
                Event::Settled => {
                    self.base_mut().emit_signal("settled", &[]);
                }
            }
        }
    }
}

/// The cells of a chunk of a tile layer: (x, y, raw value) of every non-empty one.
fn chunk_cells(
    m: &MapStream,
    layer: &tiled_core::Layer,
    li: usize,
    c: Coord,
    cs: Vector2i,
    diag: &mut crate::diag::Diag,
    report: &mut bool,
) -> Vec<(i32, i32, u32)> {
    let t = &m.tiles[li];
    let (x0, y0) = (c.0 * cs.x, c.1 * cs.y);
    let mut out = Vec::with_capacity((cs.x * cs.y) as usize);
    for &si in t.grid.get(&c).into_iter().flatten() {
        let s = &t.srcs[si as usize];
        let cells: Cow<[u32]> = match s.chunk {
            Some(ci) => {
                let Some(chunk) = layer.chunks.as_ref().and_then(|v| v.get(ci)) else {
                    continue;
                };
                match chunk.decode(layer) {
                    Some(Ok(cells)) => cells,
                    Some(Err(e)) => {
                        if !*report {
                            diag.error(e.message);
                            *report = true;
                        }
                        continue;
                    }
                    None => continue,
                }
            }
            None => match &t.data {
                Some(d) => Cow::Borrowed(d.as_slice()),
                None => continue,
            },
        };
        let (ax, ay) = (x0.max(s.x), y0.max(s.y));
        let (bx, by) = ((x0 + cs.x).min(s.x + s.w), (y0 + cs.y).min(s.y + s.h));
        for y in ay..by {
            let row = ((y - s.y) * s.w) as usize;
            for x in ax..bx {
                let Some(&raw) = cells.get(row + (x - s.x) as usize) else {
                    continue;
                };
                if raw & GID_MASK != 0 {
                    out.push((x, y, raw));
                }
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn load_tiles(
    m: &mut MapStream,
    ts: &mut TiledTileSets,
    li: usize,
    c: Coord,
    cs: Vector2i,
    remaining: f64,
    costs: &mut Costs,
    stats: &mut Stats,
    events: &mut Vec<Event>,
    diag: &mut crate::diag::Diag,
) -> Option<f64> {
    let map = m.map.clone();
    let all = map.all_layers();
    let layer = all[m.tiles[li].idx];
    let mut report = m.tiles[li].reported;
    let cells = chunk_cells(m, layer, li, c, cs, diag, &mut report);
    m.tiles[li].reported = report;
    // Values seen for the first time are resolved by the build (their tiles exist already)
    let mut fresh: Vec<u32> = cells
        .iter()
        .map(|&(_, _, raw)| raw)
        .filter(|&raw| m.table.get(raw).is_none())
        .collect();
    if !fresh.is_empty() {
        fresh.sort_unstable();
        fresh.dedup();
        let mut b = Build::resume(&map, ts, m.build.take().expect("build"));
        for raw in fresh {
            let r = b.resolve_value(raw);
            m.table.insert(raw, r);
        }
        let d = b.take_diag();
        m.build = Some(b.suspend());
        diag.errors += d.errors;
        diag.warnings += d.warnings;
    }
    let t = &mut m.tiles[li];
    let resolved: Vec<(i32, i32, [i16; 4], bool)> = cells
        .iter()
        .filter_map(|&(x, y, raw)| m.table.get(raw).flatten().map(|(r, col)| (x, y, r, col)))
        .collect();
    let n = resolved.len() as f64;
    let n_col = resolved.iter().filter(|r| r.3).count() as f64;
    let x = if t.ysort {
        [1.0, 0.0, n, n_col]
    } else {
        [1.0, n, 0.0, n_col]
    };
    let predicted = costs.tiles.predict(x);
    if predicted > remaining {
        return None;
    }
    let words = ((cs.x * cs.y) as usize).div_ceil(64);
    let mut mask = vec![0u64; words];
    let t0 = Instant::now();
    for &(x, y, r, _) in &resolved {
        t.node
            .set_cell_ex(Vector2i::new(x, y))
            .source_id(r[0] as i32)
            .atlas_coords(Vector2i::new(r[1] as i32, r[2] as i32))
            .alternative_tile(r[3] as u16 as i32)
            .done();
        let bit = ((y - c.1 * cs.y) * cs.x + (x - c.0 * cs.x)) as usize;
        mask[bit / 64] |= 1 << (bit % 64);
    }
    if !resolved.is_empty() {
        t.node.update_internals();
    }
    if n > 0.0 {
        costs.tiles.add(x, t0.elapsed().as_secs_f64() * 1e6);
    }
    t.loaded.insert(c, mask);
    stats.tile_chunks_loaded += 1;
    stats.cells_set += n as i64;
    stats.collision_cells_set += n_col as i64;
    events.push(Event::ChunkLoaded(t.name.clone(), Vector2i::new(c.0, c.1)));
    Some(predicted)
}

#[allow(clippy::too_many_arguments)]
fn unload_tiles(
    m: &mut MapStream,
    li: usize,
    c: Coord,
    cs: Vector2i,
    remaining: f64,
    costs: &mut Costs,
    stats: &mut Stats,
    events: &mut Vec<Event>,
) -> Option<f64> {
    let t = &mut m.tiles[li];
    let Some(mask) = t.loaded.get(&c) else {
        return Some(0.0);
    };
    let n: u32 = mask.iter().map(|w| w.count_ones()).sum();
    let x = if t.ysort {
        [1.0, 0.0, n as f64]
    } else {
        [1.0, n as f64, 0.0]
    };
    let predicted = costs.erase.predict(x);
    if predicted > remaining {
        return None;
    }
    let mask = t.loaded.remove(&c).expect("checked");
    let t0 = Instant::now();
    for (wi, w) in mask.iter().enumerate() {
        let mut w = *w;
        while w != 0 {
            let b = w.trailing_zeros() as usize;
            w &= w - 1;
            let bit = (wi * 64 + b) as i32;
            let x = c.0 * cs.x + bit % cs.x;
            let y = c.1 * cs.y + bit / cs.x;
            t.node.erase_cell(Vector2i::new(x, y));
        }
    }
    if n > 0 {
        t.node.update_internals();
        costs.erase.add(x, t0.elapsed().as_secs_f64() * 1e6);
    }
    stats.tile_chunks_unloaded += 1;
    events.push(Event::ChunkUnloaded(
        t.name.clone(),
        Vector2i::new(c.0, c.1),
    ));
    Some(predicted)
}

#[allow(clippy::too_many_arguments)]
fn spawn_objects(
    m: &mut MapStream,
    ts: &mut TiledTileSets,
    li: usize,
    c: Coord,
    remaining: f64,
    costs: &mut Costs,
    stats: &mut Stats,
    events: &mut Vec<Event>,
    diag: &mut crate::diag::Diag,
) -> Option<f64> {
    let idxs: Vec<u32> = if c == ALL {
        // Every object of chunks not loaded yet, in the layer's order
        let o = &m.objs[li];
        let mut v: Vec<u32> = o
            .grid
            .iter()
            .filter(|(k, _)| !o.loaded.contains(*k))
            .flat_map(|(_, v)| v.iter().copied())
            .collect();
        v.sort_unstable();
        v
    } else {
        m.objs[li].grid.get(&c).cloned().unwrap_or_default()
    };
    let predicted = costs.spawn.predict([1.0, idxs.len() as f64]);
    if predicted > remaining {
        return None;
    }
    let map = m.map.clone();
    let all = map.all_layers();
    let layer = all[m.objs[li].idx];
    let objects = layer.objects.as_deref().unwrap_or(&[]);
    let mut b = Build::resume(&map, ts, m.build.take().expect("build"));
    let o = &mut m.objs[li];
    let parent: Gd<Node> = o.node.clone().upcast();
    let t0 = Instant::now();
    for &i in &idxs {
        let Some(obj) = objects.get(i as usize) else {
            continue;
        };
        let nodes = b.spawn_object(obj, &parent);
        if o.ordered {
            // Before the nodes of the next object of the layer that exists
            if let Some(next) = o
                .spawned
                .range(i + 1..)
                .find_map(|(_, v)| v.iter().find(|n| n.is_instance_valid()))
            {
                let mut p = parent.clone();
                for (at, n) in (next.get_index()..).zip(nodes.iter()) {
                    p.move_child(n, at);
                }
            }
        }
        let id = obj.id.unwrap_or(0);
        if let Some(first) = nodes.first() {
            events.push(Event::Spawned(first.clone(), id));
        }
        m.changed.insert(id);
        o.spawned.insert(i, nodes);
        stats.objects_spawned += 1;
    }
    if !idxs.is_empty() {
        costs
            .spawn
            .add([1.0, idxs.len() as f64], t0.elapsed().as_secs_f64() * 1e6);
    }
    if c == ALL {
        let keys: Vec<Coord> = o.grid.keys().copied().collect();
        o.loaded.extend(keys);
    } else {
        o.loaded.insert(c);
    }
    let d = b.take_diag();
    m.build = Some(b.suspend());
    diag.errors += d.errors;
    diag.warnings += d.warnings;
    Some(predicted)
}

#[allow(clippy::too_many_arguments)]
fn despawn_objects(
    m: &mut MapStream,
    ts: &mut TiledTileSets,
    li: usize,
    c: Coord,
    remaining: f64,
    costs: &mut Costs,
    stats: &mut Stats,
    events: &mut Vec<Event>,
    diag: &mut crate::diag::Diag,
) -> Option<f64> {
    let idxs: Vec<u32> = m.objs[li].grid.get(&c).cloned().unwrap_or_default();
    let live = idxs
        .iter()
        .filter(|i| m.objs[li].spawned.contains_key(i))
        .count();
    let predicted = costs.free.predict([1.0, live as f64]);
    if predicted > remaining {
        return None;
    }
    let map = m.map.clone();
    let all = map.all_layers();
    let layer = all[m.objs[li].idx];
    let objects = layer.objects.as_deref().unwrap_or(&[]);
    let mut b = Build::resume(&map, ts, m.build.take().expect("build"));
    let o = &mut m.objs[li];
    let t0 = Instant::now();
    for i in idxs {
        let Some(nodes) = o.spawned.remove(&i) else {
            continue;
        };
        let id = objects.get(i as usize).and_then(|o| o.id).unwrap_or(0);
        b.forget_object(id, &nodes);
        for n in nodes {
            if n.is_instance_valid() {
                n.free();
            }
        }
        m.changed.insert(id);
        events.push(Event::Despawned(id));
        stats.objects_despawned += 1;
    }
    if live > 0 {
        costs
            .free
            .add([1.0, live as f64], t0.elapsed().as_secs_f64() * 1e6);
    }
    o.loaded.remove(&c);
    let d = b.take_diag();
    m.build = Some(b.suspend());
    diag.errors += d.errors;
    diag.warnings += d.warnings;
    Some(predicted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_recovers_a_linear_cost() {
        // t = 50 + 2 n + 6 c, over chunks of varied sizes
        let mut f = Fit::new([30.0, 1.8, 16.0, 7.0]);
        for i in 0..40 {
            let n = (16 + (i * 37) % 240) as f64;
            let c = (i % 5) as f64 * n / 8.0;
            f.add([1.0, n, 0.0, c], 50.0 + 2.0 * n + 6.0 * c);
        }
        assert!((f.coef[0] - 50.0).abs() < 5.0, "{:?}", f.coef);
        assert!((f.coef[1] - 2.0).abs() < 0.05, "{:?}", f.coef);
        assert!((f.coef[3] - 6.0).abs() < 0.1, "{:?}", f.coef);
        // No sample with y-sorted cells: the prior stands
        assert_eq!(f.coef[2], 16.0);
        assert!((f.predict([1.0, 256.0, 0.0, 0.0]) - 562.0).abs() < 15.0);
    }

    #[test]
    fn fit_ignores_bad_samples_and_never_goes_negative() {
        let mut f = Fit::new([10.0, 1.0]);
        f.add([1.0, 10.0], f64::NAN);
        f.add([1.0, 10.0], -5.0);
        assert_eq!(f.samples, 0);
        f.add([1.0, 100.0], 0.0);
        assert!(f.coef.iter().all(|&c| c >= 0.0));
    }

    #[test]
    fn table_dense_and_sparse() {
        let mut t = Table::default();
        t.insert(5, Some(([0, 1, 2, 0], false)));
        t.insert(5 | 0x8000_0000, None);
        t.insert(1_000_000, Some(([1, 0, 0, 0], true)));
        assert_eq!(t.get(5), Some(Some(([0, 1, 2, 0], false))));
        assert_eq!(t.get(5 | 0x8000_0000), Some(None));
        assert_eq!(t.get(6), None);
        // The hexagonal 120° flag is not part of the key
        assert_eq!(
            t.get(1_000_000 | 0x1000_0000),
            Some(Some(([1, 0, 0, 0], true)))
        );
    }
}
