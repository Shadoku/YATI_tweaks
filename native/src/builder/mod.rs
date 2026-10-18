//! `TiledBuilder`: the one-shot materializer. `build(map, tilesets, options)` makes the whole
//! node tree of a loaded map in one call on the main thread, exactly the tree the GDScript
//! runtime's `TilemapCreator` makes (ported here: names and suffixes, owners, metadata, groups,
//! materials, parallax nodes, the single-child parentless return), with the tile layers filled
//! through one `tile_map_data` assignment each.

mod assign;
mod cells;
mod nodeprops;
mod objects;
mod refs;
pub(crate) mod stream;

use crate::diag::Diag;
use crate::dynobj::{get_vec, set};
use crate::gd::{get_basename, get_file, tiled_color};
use crate::map::TiledMap;
use crate::options::{options_of, Options, TiledImportOptions};
use crate::props::{get_property, godot_node_type_property, gprops, Enums, GProp};
use crate::tileset::{Built, TiledTileSets};
use godot::classes::canvas_item::TextureFilter;
use godot::classes::canvas_item_material::BlendMode;
use godot::classes::texture_rect::StretchMode;
use godot::classes::{
    CanvasItem, CanvasItemMaterial, ColorRect, Node, Node2D, ParallaxBackground, ParallaxLayer,
    ResourceLoader, ResourceSaver, TextureRect, TileMapLayer, TileSet,
};
use godot::global::Error as GdError;
use godot::prelude::*;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use tiled_core::{Format, Layer, Map, Object};

/// The node types the runtime maps Tiled classes to (`_godot_type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GType {
    Empty,
    Body,
    CBody,
    RBody,
    ABody,
    Area,
    Navigation,
    Occluder,
    Line,
    Path,
    Polygon,
    Instance,
    Parallax,
    Node2D,
    Unknown,
}

impl GType {
    /// `get_godot_type`: a class name (or `godot_node_type` value) to its node type.
    pub fn of(s: &str) -> GType {
        match s.to_lowercase().as_str() {
            "" => GType::Empty,
            "collision" | "staticbody" => GType::Body,
            "characterbody" => GType::CBody,
            "rigidbody" => GType::RBody,
            "animatablebody" => GType::ABody,
            "area" => GType::Area,
            "navigation" => GType::Navigation,
            "occluder" => GType::Occluder,
            "line" => GType::Line,
            "path" => GType::Path,
            "polygon" => GType::Polygon,
            "instance" => GType::Instance,
            "parallax" => GType::Parallax,
            _ => GType::Unknown,
        }
    }

    pub fn is_collision_object(self) -> bool {
        matches!(
            self,
            GType::Body | GType::ABody | GType::Area | GType::CBody | GType::RBody
        )
    }
}

/// The class of a layer (`get_class_string(layer, true)`: never its `type`, the layer kind).
fn layer_class(l: &Layer) -> String {
    l.class.clone().unwrap_or_default()
}

/// Declares [`Build`] and [`Suspended`] with one field list: a build borrows its map and
/// TileSet cache for one call, everything else it owns, so a streamer can suspend a build
/// between frames (its owned state moved into a `Suspended`) and resume it on the same map.
macro_rules! build_state {
    ($($(#[$m:meta])* $v:vis $f:ident : $t:ty,)*) => {
        /// One build: the port of `TilemapCreator.create` and the state it keeps.
        pub(crate) struct Build<'m> {
            map: &'m Map,
            tilesets: &'m mut TiledTileSets,
            objects_by_id: Option<HashMap<i64, &'m Object>>,
            $($(#[$m])* $v $f: $t,)*
        }

        /// A [`Build`]'s owned state between two calls (`Build::suspend`, `Build::resume`).
        pub(crate) struct Suspended {
            $($f: $t,)*
        }

        impl<'m> Build<'m> {
            /// The build's owned state, the borrows released.
            pub(crate) fn suspend(self) -> Suspended {
                Suspended { $($f: self.$f,)* }
            }

            /// A suspended build resumed on its map (the same one) and a TileSet cache.
            pub(crate) fn resume(map: &'m Map, tilesets: &'m mut TiledTileSets, s: Suspended) -> Self {
                Build { map, tilesets, objects_by_id: None, $($f: s.$f,)* }
            }
        }
    };
}

build_state! {
    json: bool,
    zip: String,
    pub diag: Diag,
    opts: Options,
    parallax_mode: String,
    unknown_type: GType,
    unknown_fallback: String,
    reported_unknown: HashSet<String>,
    orientation: String,
    map_w: i64,
    map_h: i64,
    tile_w: i64,
    tile_h: i64,
    parallax_origin: Vector2,
    base_path: String,
    base_name: String,
    built: Rc<Built>,
    first_gids: Vec<i64>,
    base_node: Gd<Node2D>,
    base_gd: Gd<Node>,
    names: HashMap<String, StringName>,
    meta_class: StringName,
    meta_id: StringName,
    parallax_background: Option<Gd<ParallaxBackground>>,
    background: Option<Gd<ColorRect>>,
    parallax_layer_existing: bool,
    parallax_scale: Vector2,
    iso: Option<(f64, f64, Vector2)>,
    cells: cells::Cells,
    /// Where the tile layers' main-thread time went (`TiledBuilder.get_timings`).
    timing: cells::TileTiming,
    atlas_index_cache: HashMap<u32, i32>,
    cell_cache: HashMap<u32, Option<[i16; 4]>>,
    /// `object_refs`, validated: `nodepath`, `geometry` or `both` (F2).
    object_refs: String,
    /// The project file's enums, for mask properties given as value names (F5).
    enums: Enums,
    /// Object id -> the node that stands for it (the first registered; F2).
    nodes_by_id: HashMap<i64, Gd<Node>>,
    /// Object properties waiting for every node to exist (`resolve_object_refs`).
    pending_refs: Vec<refs::PendingRef>,
    /// Object nodes with a script, and their objects: `_tiled_import` candidates (F1).
    hooks: Vec<(Gd<Node>, Obj)>,
    /// Scripts attached by `godot_script` properties so far (a node gets a script only so,
    /// or from its scene).
    scripts_set: u64,
    /// `get_assignable_properties`' cache of scripted nodes: class -> script id -> properties
    /// by name.
    prop_info: HashMap<String, HashMap<i64, Rc<assign::PropInfos>>>,
    /// A streamer's build: tile layers are left empty and object layers without objects, both
    /// recorded here (`stream::Skeleton`); `None` for a one-shot build.
    skel: Option<stream::Skeleton>,
    /// Object references of a streamer's objects: kept and resolved again whenever their
    /// targets come or go; a missing target is an empty path without a warning.
    stream_refs: Vec<refs::PendingRef>,
}

impl<'m> Build<'m> {
    fn new(map: &'m Map, zip: &str, tilesets: &'m mut TiledTileSets, opts: Options) -> Self {
        let mut diag = Diag::default();
        let mut parallax_mode = opts.parallax_mode.to_lowercase();
        if parallax_mode != "parallax2d" && parallax_mode != "legacy" {
            diag.warn(format!(
                " -- Option parallax_mode '{}' is not supported. -> Using 'parallax2d'",
                opts.parallax_mode
            ));
            parallax_mode = "parallax2d".into();
        }
        let mut unknown_fallback = opts.unknown_class_fallback.to_lowercase();
        let unknown_type = if unknown_fallback == "node2d" {
            GType::Node2D
        } else {
            let t = GType::of(&unknown_fallback);
            if matches!(t, GType::Unknown | GType::Instance | GType::Parallax) {
                diag.warn(format!(
                    " -- Option unknown_class_fallback '{}' is not supported. -> Using 'staticbody'",
                    opts.unknown_class_fallback
                ));
                unknown_fallback = "staticbody".into();
                GType::Body
            } else {
                t
            }
        };
        let mut object_refs = opts.object_refs.to_lowercase();
        if object_refs != "nodepath" && object_refs != "geometry" && object_refs != "both" {
            diag.warn(format!(
                " -- Option object_refs '{}' is not supported. -> Using 'nodepath'",
                opts.object_refs
            ));
            object_refs = "nodepath".into();
        }
        let base_path = map.base_dir();
        let base_name = get_basename(get_file(map.path())).to_string();
        let base_node = Node2D::new_alloc();
        Build {
            map,
            json: map.format() == Format::Json,
            zip: zip.to_string(),
            tilesets,
            diag,
            parallax_mode,
            unknown_type,
            unknown_fallback,
            reported_unknown: HashSet::new(),
            orientation: map
                .orientation
                .clone()
                .unwrap_or_else(|| "othogonal".into()),
            map_w: map.width.unwrap_or(0),
            map_h: map.height.unwrap_or(0),
            tile_w: map.tilewidth.unwrap_or(0),
            tile_h: map.tileheight.unwrap_or(0),
            parallax_origin: Vector2::new(
                map.parallaxoriginx.map(|n| n.as_f64()).unwrap_or(0.0) as f32,
                map.parallaxoriginy.map(|n| n.as_f64()).unwrap_or(0.0) as f32,
            ),
            base_path,
            base_name,
            built: Rc::new(Built {
                tileset: TileSet::new_gd(),
                sources: Vec::new(),
                object_groups: HashMap::new(),
                internal: HashMap::new(),
            }),
            first_gids: Vec::new(),
            base_node: base_node.clone(),
            base_gd: base_node.upcast(),
            names: HashMap::new(),
            meta_class: StringName::from("class"),
            meta_id: StringName::from("id"),
            parallax_background: None,
            background: None,
            parallax_layer_existing: false,
            parallax_scale: Vector2::ONE,
            iso: None,
            cells: cells::Cells::default(),
            timing: cells::TileTiming::default(),
            objects_by_id: None,
            atlas_index_cache: HashMap::new(),
            cell_cache: HashMap::new(),
            object_refs,
            enums: Enums::of(map.project()),
            nodes_by_id: HashMap::new(),
            pending_refs: Vec::new(),
            hooks: Vec::new(),
            scripts_set: 0,
            prop_info: HashMap::new(),
            skel: None,
            stream_refs: Vec::new(),
            opts,
        }
    }

    fn own(&self, n: &mut Gd<impl Inherits<Node>>) {
        n.upcast_mut::<Node>().set_owner(&self.base_gd);
    }

    /// A StringName, made once per build for each distinct string (metadata keys, names).
    pub(super) fn sn(&mut self, s: &str) -> StringName {
        if let Some(n) = self.names.get(s) {
            return n.clone();
        }
        let n = StringName::from(s);
        self.names.insert(s.to_string(), n.clone());
        n
    }

    /// `create`.
    fn run(mut self) -> (Option<Gd<Node>>, Diag, cells::TileTiming) {
        let map = self.map;
        for t in map.tilesets.iter().flatten() {
            self.first_gids.push(t.firstgid.unwrap_or(0));
        }
        let zip = self.zip.clone();
        let opts = self.opts.clone();
        self.built = self
            .tilesets
            .built_for_map(map, &zip, &opts, &mut self.diag);
        let t = std::time::Instant::now();
        self.cells.prepare(map);
        self.timing.prepare_us += t.elapsed().as_micros() as u64;

        let mut base = self.base_node.clone();
        base.set_name(self.base_name.as_str());
        if self.parallax_mode == "legacy" {
            let mut pbg = ParallaxBackground::new_alloc();
            base.add_child(&pbg);
            pbg.set_name(format!("{} (PBG)", self.base_name).as_str());
            self.own(&mut pbg);
            self.parallax_background = Some(pbg);
        }
        if let Some(bg) = map.backgroundcolor.as_deref().filter(|c| !c.is_empty()) {
            let mut rect = ColorRect::new_alloc();
            rect.set_color(tiled_color(bg, Color::WHITE));
            rect.set_size(Vector2::new(
                (self.map_w * self.tile_w) as f32,
                (self.map_h * self.tile_h) as f32,
            ));
            base.add_child(&rect);
            rect.set_name("Background Color");
            self.own(&mut rect);
            self.background = Some(rect);
        }
        let parent: Gd<Node> = base.clone().upcast();
        for l in map.layers.iter().flatten() {
            self.handle_layer(l, &parent);
        }

        let save_path = self.opts.save_tileset_to.clone();
        if !save_path.is_empty() {
            let ts = self.built.tileset.clone();
            let ret = ResourceSaver::singleton()
                .save_ex(&ts)
                .path(save_path.as_str())
                .done();
            if ret == GdError::OK {
                self.diag
                    .info(format!("Successfully saved tileset to '{save_path}'"));
                for n in base
                    .find_children_ex("*")
                    .type_("TileMapLayer")
                    .done()
                    .iter_shared()
                {
                    if let Ok(mut l) = n.try_cast::<TileMapLayer>() {
                        let unsaved = l.get_tile_set().is_none_or(|t| t.get_path().is_empty());
                        if unsaved {
                            let loaded = ResourceLoader::singleton()
                                .load(save_path.as_str())
                                .and_then(|r| r.try_cast::<TileSet>().ok());
                            l.set_tile_set(loaded.as_ref());
                        }
                    }
                }
            } else {
                self.diag
                    .error(format!("Saving tileset returned error {}", ret.ord()));
            }
        }

        let map_props = gprops(&map.properties, self.json);
        if let Some(p) = &map_props {
            let mut n: Gd<Node> = base.clone().upcast();
            self.handle_properties(&mut n, p, "");
        }
        // Every object's node exists now: object properties become NodePaths (F2)
        self.resolve_object_refs();

        if let Some(pbg) = self.parallax_background.take() {
            if pbg.get_child_count() == 0 {
                base.remove_child(&pbg);
                pbg.free();
            }
        }

        let single = (base.get_child_count() == 1 && !self.opts.always_wrap_root)
            .then(|| base.get_child(0))
            .flatten();
        let Some(mut ret) = single else {
            let mut root: Gd<Node> = base.upcast();
            self.finish_import(&mut root);
            return (Some(root), self.diag, self.timing);
        };
        // A single child: returned parentless, the wrapper freed
        base.remove_child(&ret);
        let r = ret.clone();
        change_owner_recursively(&mut ret, &r);
        if let Some(p) = &map_props {
            self.handle_properties(&mut ret, p, "");
            self.resolve_object_refs();
        }
        ret.set_name(self.base_name.as_str());
        self.finish_import(&mut ret);
        base.free();
        (Some(ret), self.diag, self.timing)
    }

    /// `finish_import`: the root gets the map's bounds as metadata (F3); every object node
    /// that has a method `_tiled_import` gets it called (F1), once the tree is complete and
    /// object references are resolved; then the per-import node maps go.
    fn finish_import(&mut self, root: &mut Gd<Node>) {
        self.root_metadata(root);
        self.call_import_hooks();
        self.nodes_by_id.clear();
        self.prop_info.clear();
    }

    /// The map's bounds and tile geometry as metadata of the root (F3).
    fn root_metadata(&mut self, root: &mut Gd<Node>) {
        let map = self.map;
        let (tx, ty, tw, th) = map.tile_rect();
        let (x, y, w, h) = map.pixel_rect((tx, ty, tw, th));
        root.set_meta(
            "map_rect",
            &Rect2::new(
                Vector2::new(x as f32, y as f32),
                Vector2::new(w as f32, h as f32),
            )
            .to_variant(),
        );
        root.set_meta(
            "map_size_tiles",
            &Vector2i::new(tw as i32, th as i32).to_variant(),
        );
        root.set_meta(
            "tile_size",
            &Vector2i::new(self.tile_w as i32, self.tile_h as i32).to_variant(),
        );
        root.set_meta("orientation", &self.orientation.to_variant());
    }

    fn layer_props(&self, l: &Layer) -> Option<Vec<GProp>> {
        gprops(&l.properties, self.json)
    }

    fn texture_filter(&self, n: &mut Gd<impl Inherits<CanvasItem>>) {
        if !self.opts.use_default_filter {
            n.clone()
                .upcast::<CanvasItem>()
                .set_texture_filter(TextureFilter::NEAREST);
        }
    }

    fn set_layer_class_meta(&self, n: &mut Gd<impl Inherits<Node>>, l: &Layer) {
        let class = layer_class(l);
        if self.opts.add_class_as_metadata && !class.is_empty() {
            n.clone()
                .upcast::<Node>()
                .set_meta("class", &class.to_variant());
        }
    }

    fn handle_layer(&mut self, l: &'m Layer, parent: &Gd<Node>) {
        let mut offset_x = l.offsetx.map(|n| n.as_f64()).unwrap_or(0.0);
        let offset_y = l.offsety.map(|n| n.as_f64()).unwrap_or(0.0);
        let opacity = l.opacity.map(|n| n.as_f64()).unwrap_or(1.0);
        let visible = l.visible.unwrap_or(true);
        let kind = l.kind_or_tile();
        let tint = l.tintcolor.clone().unwrap_or_else(|| "#ffffff".into());
        let props = self.layer_props(l);
        if get_property(props.as_deref(), "no_import", "bool") == "true" {
            return;
        }
        let modulate = opacity < 1.0 || tint != "#ffffff";
        match kind {
            "tilelayer" => {
                if self.orientation == "isometric" {
                    offset_x += self.tile_w as f64 * (self.map_h as f64 / 2.0 - 0.5);
                }
                let mut tml = TileMapLayer::new_alloc();
                let name = l.name.clone().unwrap_or_default();
                if !name.is_empty() {
                    tml.set_name(name.as_str());
                }
                tml.set_visible(visible);
                tml.set_position(Vector2::new(offset_x as f32, offset_y as f32));
                if modulate {
                    tml.set_modulate(layer_color(&tint, opacity));
                }
                tml.set_tile_set(&self.built.tileset);
                let n: Gd<Node> = tml.clone().upcast();
                self.handle_parallaxes(parent, &n, l);
                if self.orientation == "isometric" || self.orientation == "staggered" {
                    tml.set_y_sort_enabled(true);
                }
                self.apply_blend_mode(&mut tml.clone().upcast(), l);
                self.texture_filter(&mut tml);
                match &mut self.skel {
                    Some(skel) => skel.tile_layers.push((stream::layer_key(l), tml.clone())),
                    None => self.fill_layer(l, &mut tml),
                }
                self.set_layer_class_meta(&mut tml, l);
                let id = l.id.unwrap_or(0);
                if self.opts.add_id_as_metadata && id != 0 {
                    tml.set_meta("id", &id.to_variant());
                }
                if let Some(p) = &props {
                    let mut n: Gd<Node> = tml.upcast();
                    self.handle_properties(&mut n, p, "");
                }
            }
            "objectgroup" => {
                let node_type = self.layer_node_type(l, props.as_deref());
                let mut layer_node: Gd<Node2D> = if node_type == GType::Parallax {
                    let mut px = crate::dynobj::new_parallax2d();
                    parent.clone().add_child(&px);
                    self.own(&mut px);
                    self.configure_parallax_node(&mut px, l);
                    px.upcast()
                } else {
                    let n = Node2D::new_alloc();
                    self.handle_parallaxes(parent, &n.clone().upcast(), l);
                    n
                };
                if let Some(name) = &l.name {
                    layer_node.set_name(name.as_str());
                }
                if modulate {
                    layer_node.set_modulate(layer_color(&tint, opacity));
                }
                layer_node.set_visible(visible);
                let px = l.x.unwrap_or(0) as f64 + offset_x;
                let py = l.y.unwrap_or(0) as f64 + offset_y;
                layer_node.set_position(Vector2::new(px as f32, py as f32));
                self.parallax_offset(&mut layer_node);
                self.texture_filter(&mut layer_node);
                let draw_order = l.draworder.as_deref().unwrap_or("topdown");
                if self.orientation == "isometric"
                    || self.orientation == "staggered"
                    || draw_order != "index"
                {
                    layer_node.set_y_sort_enabled(true);
                }
                self.apply_blend_mode(&mut layer_node.clone().upcast(), l);
                let parent_node: Gd<Node> = layer_node.clone().upcast();
                match &mut self.skel {
                    Some(skel) => skel
                        .object_layers
                        .push((stream::layer_key(l), layer_node.clone())),
                    None => {
                        for obj in l.objects.iter().flatten() {
                            let o = self.map_object(obj);
                            let ts = self.built.clone();
                            self.handle_object(o, &parent_node, &ts, Vector2::ZERO);
                        }
                    }
                }
                self.set_layer_class_meta(&mut layer_node, l);
                if let Some(p) = &props {
                    let mut n: Gd<Node> = layer_node.upcast();
                    self.handle_properties(&mut n, p, "");
                }
            }
            "group" => {
                let node_type = self.layer_node_type(l, props.as_deref());
                let mut group: Gd<Node2D> = if node_type == GType::Parallax {
                    let mut px = crate::dynobj::new_parallax2d();
                    parent.clone().add_child(&px);
                    self.own(&mut px);
                    self.configure_parallax_node(&mut px, l);
                    px.upcast()
                } else {
                    let n = Node2D::new_alloc();
                    self.handle_parallaxes(parent, &n.clone().upcast(), l);
                    n
                };
                group.set_name(l.name.as_deref().unwrap_or("group"));
                if modulate {
                    group.set_modulate(layer_color(&tint, opacity));
                }
                group.set_visible(visible);
                let px = l.x.unwrap_or(0) as f64 + offset_x;
                let py = l.y.unwrap_or(0) as f64 + offset_y;
                group.set_position(Vector2::new(px as f32, py as f32));
                self.parallax_offset(&mut group);
                self.apply_blend_mode(&mut group.clone().upcast(), l);
                // Tiled multiplies the parallax factors of nested layers
                let enclosing = self.parallax_scale;
                if self.parallax_mode != "legacy" {
                    self.parallax_scale *= parallax_factor(l);
                }
                let gnode: Gd<Node> = group.clone().upcast();
                for c in l.layers.iter().flatten() {
                    self.handle_layer(c, &gnode);
                }
                self.parallax_scale = enclosing;
                // A material on a plain Node2D does not reach its children
                if group.get_material().is_some() {
                    use_parent_material_recursively(&gnode);
                }
                self.set_layer_class_meta(&mut group, l);
                if let Some(p) = &props {
                    let mut n: Gd<Node> = group.upcast();
                    self.handle_properties(&mut n, p, "");
                }
            }
            "imagelayer" => self.handle_image_layer(
                l, parent, props, offset_x, offset_y, opacity, visible, &tint,
            ),
            _ => {}
        }
    }

    /// A Parallax2D sets its own position, so a layer offset goes into its scroll_offset.
    fn parallax_offset(&self, n: &mut Gd<Node2D>) {
        if self.parallax_mode != "legacy" && n.is_class("Parallax2D") {
            let o = get_vec(n, "scroll_offset") + n.get_position();
            set(n, "scroll_offset", o);
        }
    }

    fn layer_node_type(&self, l: &Layer, props: Option<&[GProp]>) -> GType {
        let (prop, found) = godot_node_type_property(props);
        GType::of(&if found { prop } else { layer_class(l) })
    }

    /// A Parallax2D of a layer whose class is `parallax`: configured, or (legacy mode) only its
    /// scroll scale from the layer's factors.
    fn configure_parallax_node(&self, px: &mut Gd<Node2D>, l: &Layer) {
        if self.parallax_mode != "legacy" {
            self.configure_parallax2d(px, l);
        } else if l.parallaxx.is_some() || l.parallaxy.is_some() {
            set(
                px,
                "scroll_scale",
                Vector2::new(
                    l.parallaxx.map(|n| n.as_f64()).unwrap_or(1.0) as f32,
                    l.parallaxy.map(|n| n.as_f64()).unwrap_or(1.0) as f32,
                ),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_image_layer(
        &mut self,
        l: &Layer,
        parent: &Gd<Node>,
        props: Option<Vec<GProp>>,
        offset_x: f64,
        offset_y: f64,
        opacity: f64,
        visible: bool,
        tint: &str,
    ) {
        let node_type = self.layer_node_type(l, props.as_deref());
        let modulate = opacity < 1.0 || tint != "#ffffff";
        let image = l.image.clone().unwrap_or_default();
        let iw = l.imagewidth.unwrap_or(0);
        let ih = l.imageheight.unwrap_or(0);
        if node_type == GType::Parallax {
            let mut px = crate::dynobj::new_parallax2d();
            parent.clone().add_child(&px);
            self.own(&mut px);
            px.set_name(l.name.as_deref().unwrap_or("parallax"));
            self.configure_parallax_node(&mut px, l);
            if l.repeatx == Some(true) && iw > 0 {
                let y = get_vec(&px, "repeat_size").y;
                set(&mut px, "repeat_size", Vector2::new(iw as f32, y));
                set(&mut px, "repeat_times", 2);
            }
            if l.repeaty == Some(true) && ih > 0 {
                let x = get_vec(&px, "repeat_size").x;
                set(&mut px, "repeat_size", Vector2::new(x, ih as f32));
                set(&mut px, "repeat_times", 2);
            }
            let mut tr = TextureRect::new_alloc();
            tr.set_name(get_basename(get_file(&image)));
            tr.set_position(Vector2::new(offset_x as f32, offset_y as f32));
            tr.set_size(Vector2::new(iw as f32, ih as f32));
            if modulate {
                tr.set_modulate(layer_color(tint, opacity));
            }
            tr.set_visible(visible);
            let tex = self.load_image(&image);
            tr.set_texture(tex.as_ref());
            self.apply_blend_mode(&mut tr.clone().upcast(), l);
            px.add_child(&tr);
            self.own(&mut tr);
            self.set_layer_class_meta(&mut px, l);
            if let Some(p) = &props {
                let mut n: Gd<Node> = px.upcast();
                self.handle_properties(&mut n, p, "");
            }
        } else {
            let mut tr = TextureRect::new_alloc();
            self.handle_parallaxes(parent, &tr.clone().upcast(), l);
            tr.set_name(l.name.as_deref().unwrap_or("image"));
            tr.set_position(Vector2::new(offset_x as f32, offset_y as f32));
            tr.set_size(Vector2::new(iw as f32, ih as f32));
            if modulate {
                tr.set_modulate(layer_color(tint, opacity));
            }
            tr.set_visible(visible);
            let tex = self.load_image(&image);
            tr.set_texture(tex.as_ref());
            self.apply_blend_mode(&mut tr.clone().upcast(), l);
            // A repeated image (unless a ParallaxLayer's mirroring or a Parallax2D repeats it)
            if l.parallaxx.is_none() && l.parallaxy.is_none() {
                let rx = l.repeatx == Some(true) && iw > 0;
                let ry = l.repeaty == Some(true) && ih > 0;
                if rx || ry {
                    tr.set_stretch_mode(StretchMode::TILE);
                    tr.set_texture_repeat(godot::classes::canvas_item::TextureRepeat::ENABLED);
                    let mut pos = tr.get_position();
                    let mut size = tr.get_size();
                    if rx {
                        let start = repeat_start(offset_x, iw as f64);
                        pos.x = start as f32;
                        size.x = ((self.map_w * self.tile_w) as f64 - start).max(iw as f64) as f32;
                    }
                    if ry {
                        let start = repeat_start(offset_y, ih as f64);
                        pos.y = start as f32;
                        size.y = ((self.map_h * self.tile_h) as f64 - start).max(ih as f64) as f32;
                    }
                    tr.set_position(pos);
                    tr.set_size(size);
                }
            }
            self.set_layer_class_meta(&mut tr, l);
            if let Some(p) = &props {
                let mut n: Gd<Node> = tr.upcast();
                self.handle_properties(&mut n, p, "");
            }
        }
    }

    fn load_image(&mut self, image: &str) -> Option<Gd<godot::classes::Texture2D>> {
        let base = self.base_path.clone();
        let zip = self.zip.clone();
        self.tilesets
            .textures
            .load(image, &base, &zip, &mut self.diag)
    }

    fn apply_blend_mode(&mut self, n: &mut Gd<CanvasItem>, l: &Layer) {
        let Some(mode) = &l.mode else { return };
        let mode = mode.to_lowercase();
        if mode.is_empty() || mode == "normal" {
            return;
        }
        if mode == "add" || mode == "multiply" {
            let mut mat = CanvasItemMaterial::new_gd();
            mat.set_blend_mode(if mode == "add" {
                BlendMode::ADD
            } else {
                BlendMode::MUL
            });
            n.set_material(&mat);
        } else {
            self.diag.warn(format!(
                " -- Blend mode '{mode}' of layer '{}' is not supported. -> Ignored",
                l.name.as_deref().unwrap_or("")
            ));
        }
    }

    /// `configure_parallax2d`.
    fn configure_parallax2d(&self, px: &mut Gd<Node2D>, l: &Layer) {
        let f = parallax_factor(l);
        let s = self.parallax_scale;
        let scale = Vector2::new(1.0 - s.x * (1.0 - f.x), 1.0 - s.y * (1.0 - f.y));
        set(px, "scroll_scale", scale);
        let o = self.parallax_origin;
        set(
            px,
            "scroll_offset",
            Vector2::new(o.x * (1.0 - scale.x), o.y * (1.0 - scale.y)),
        );
        let iw = l.imagewidth.unwrap_or(0);
        let ih = l.imageheight.unwrap_or(0);
        if l.repeatx == Some(true) && iw > 0 {
            let mut r = get_vec(px, "repeat_size");
            r.x = iw as f32;
            set(px, "repeat_size", r);
            set(px, "repeat_times", 2);
        }
        if l.repeaty == Some(true) && ih > 0 {
            let mut r = get_vec(px, "repeat_size");
            r.y = ih as f32;
            set(px, "repeat_size", r);
            set(px, "repeat_times", 2);
        }
    }

    /// `handle_parallaxes`: adds the layer's node under `parent`, wrapped in a Parallax2D (or a
    /// ParallaxLayer of the ParallaxBackground in legacy mode) when it has a parallax factor.
    fn handle_parallaxes(&mut self, parent: &Gd<Node>, layer_node: &Gd<Node>, l: &Layer) {
        let has_factor = l.parallaxx.is_some() || l.parallaxy.is_some();
        let mut layer_node = layer_node.clone();
        if has_factor && self.parallax_mode != "legacy" {
            let mut px = crate::dynobj::new_parallax2d();
            let name = l.name.clone().unwrap_or_default();
            px.set_name(
                if name.is_empty() {
                    "Parallax2D".to_string()
                } else {
                    format!("{name} (PX)")
                }
                .as_str(),
            );
            self.configure_parallax2d(&mut px, l);
            parent.clone().add_child(&px);
            self.own(&mut px);
            px.add_child(&layer_node);
        } else if has_factor {
            if !self.parallax_layer_existing {
                if let (Some(bg), Some(pbg)) =
                    (self.background.clone(), self.parallax_background.clone())
                {
                    let mut bg = bg;
                    bg.set_owner(Gd::<Node>::null_arg());
                    bg.reparent(&pbg);
                    self.own(&mut bg);
                }
                self.parallax_layer_existing = true;
            }
            let par_x = l.parallaxx.map(|n| n.as_f64()).unwrap_or(1.0) as f32;
            let par_y = l.parallaxy.map(|n| n.as_f64()).unwrap_or(1.0) as f32;
            let mut pl = ParallaxLayer::new_alloc();
            if let Some(mut pbg) = self.parallax_background.clone() {
                pbg.add_child(&pl);
            }
            self.own(&mut pl);
            let name = l.name.clone().unwrap_or_default();
            pl.set_name(
                if name.is_empty() {
                    "ParallaxLayer".to_string()
                } else {
                    format!("{name} (PL)")
                }
                .as_str(),
            );
            pl.set_motion_scale(Vector2::new(par_x, par_y));
            let mx = if l.repeatx == Some(true) {
                l.imagewidth.unwrap_or(0)
            } else {
                0
            };
            let my = if l.repeaty == Some(true) {
                l.imageheight.unwrap_or(0)
            } else {
                0
            };
            if mx != 0 || my != 0 {
                set(
                    &mut pl,
                    "motion_mirroring",
                    Vector2::new(mx as f32, my as f32),
                );
            }
            pl.add_child(&layer_node);
        } else {
            parent.clone().add_child(&layer_node);
        }
        self.own(&mut layer_node);
    }
}

/// A layer's modulate: its tint colour with the layer opacity on its alpha.
fn layer_color(tint: &str, opacity: f64) -> Color {
    let mut c = tiled_color(tint, Color::WHITE);
    c.a *= opacity as f32;
    c
}

/// The start (<= 0) of a repeated image whose phase matches its offset.
fn repeat_start(offset: f64, size: f64) -> f64 {
    offset - (offset / size).ceil() * size
}

fn parallax_factor(l: &Layer) -> Vector2 {
    Vector2::new(
        l.parallaxx.map(|n| n.as_f64()).unwrap_or(1.0) as f32,
        l.parallaxy.map(|n| n.as_f64()).unwrap_or(1.0) as f32,
    )
}

fn use_parent_material_recursively(node: &Gd<Node>) {
    for child in node.get_children().iter_shared() {
        if let Ok(mut ci) = child.clone().try_cast::<CanvasItem>() {
            if ci.get_material().is_none() {
                ci.set_use_parent_material(true);
            }
        }
        use_parent_material_recursively(&child);
    }
}

/// `recursively_change_owner`.
fn change_owner_recursively(node: &mut Gd<Node>, owner: &Gd<Node>) {
    if node != owner {
        node.set_owner(owner);
    }
    for mut c in node.get_children().iter_shared() {
        change_owner_recursively(&mut c, owner);
    }
}

/// An object as the builder handles it: the typed object and its property dictionaries.
#[derive(Clone)]
pub(crate) struct Obj {
    pub o: Object,
    pub props: Option<Vec<GProp>>,
    /// `template_dir_path`: set on a template's objects.
    pub template_dir: Option<String>,
}

impl Build<'_> {
    fn map_object(&self, o: &Object) -> Obj {
        Obj {
            props: gprops(&o.properties, self.json),
            o: o.clone(),
            template_dir: None,
        }
    }
}

/// Builds node trees from loaded maps: `build(map, tilesets, options)` returns the root of the
/// tree the GDScript runtime makes for the map (parentless), or `null` when the map is not
/// loaded. The counts of the messages it printed are in `get_error_count` and
/// `get_warning_count` afterwards.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct TiledBuilder {
    base: Base<RefCounted>,
    errors: u32,
    warnings: u32,
    messages: Vec<String>,
    timing: cells::TileTiming,
}

#[godot_api]
impl TiledBuilder {
    /// The whole tree of a loaded map, in one call. The map's TileSet comes from `tilesets`
    /// (built there on first use); `options` may be `null` for the defaults.
    #[func]
    fn build(
        &mut self,
        map: Gd<TiledMap>,
        tilesets: Gd<TiledTileSets>,
        options: Option<Gd<TiledImportOptions>>,
    ) -> Option<Gd<Node>> {
        self.errors = 0;
        self.warnings = 0;
        self.messages.clear();
        let (inner, zip, prepared) = {
            let m = map.bind();
            (m.inner(), m.zip_file.clone(), m.prepared())
        };
        let inner = inner?;
        let opts = options_of(options);
        let mut tilesets = tilesets;
        let mut ts = tilesets.bind_mut();
        let mut b = Build::new(&inner, &zip, &mut ts, opts);
        b.cells.given(prepared);
        let (node, diag, timing) = b.run();
        ts.textures.close_zip();
        self.timing = timing;
        self.errors = diag.errors;
        self.warnings = diag.warnings;
        self.messages = diag.messages;
        node
    }

    /// Errors the last `build` printed (the map's own load errors not included).
    #[func]
    fn get_error_count(&self) -> i64 {
        self.errors as i64
    }

    /// Warnings the last `build` printed.
    #[func]
    fn get_warning_count(&self) -> i64 {
        self.warnings as i64
    }

    /// Where the last `build` spent its tile layers' main-thread time, in µs: `prepare_us`
    /// (decoding the layers' payloads into cell blobs; 0 when the map came prepared, see
    /// `TiledMap.prepare_tile_layers`), `cells_us` (resolving the distinct cell values and
    /// writing them into the blobs), `assign_us` (the engine's `tile_map_data` assignments).
    #[func]
    fn get_timings(&self) -> VarDictionary {
        let mut d = VarDictionary::new();
        d.set("prepare_us", self.timing.prepare_us as i64);
        d.set("cells_us", self.timing.cells_us as i64);
        d.set("assign_us", self.timing.assign_us as i64);
        d
    }

    /// The messages of the last `build`, each prefixed `E:` (error) or `W:` (warning).
    #[func]
    fn get_messages(&self) -> PackedStringArray {
        self.messages
            .iter()
            .map(|s| GString::from(s.as_str()))
            .collect()
    }
}
