//! `TiledImportOptions`: every option of the importer, as the facade's variables have them.

use godot::prelude::*;

/// The import options (`addons/yati_native/Importer.gd`'s variables, the GDScript runtime's
/// options): set the fields, pass the resource to `TiledTileSets.tileset_for` and
/// `TiledBuilder.build`.
#[derive(GodotClass)]
#[class(base = Resource)]
pub struct TiledImportOptions {
    /// Use Godot's default texture filter instead of `nearest` on the created layers.
    #[var]
    pub use_default_filter: bool,
    /// Store a Tiled class as metadata `class` on the created nodes and tiles.
    #[var]
    pub add_class_as_metadata: bool,
    /// Store the Tiled object id as metadata `id` on the created nodes.
    #[var]
    pub add_id_as_metadata: bool,
    /// Flipped and rotated tiles use the engine's transform flags instead of alternative tiles.
    #[var]
    pub no_alternative_tiles: bool,
    /// Map each wangset to a terrain instead of to a terrain set (the old mapping).
    #[var]
    pub map_wangset_to_terrain: bool,
    /// Tile properties with this prefix become custom data layers (`""`: every tile property).
    #[var]
    pub custom_data_prefix: GString,
    /// When not empty the map's TileSet is saved to this resource path.
    #[var]
    pub save_tileset_to: GString,
    /// `parallax2d` (a Parallax2D at the layer's place) or `legacy` (ParallaxLayers in one
    /// ParallaxBackground).
    #[var]
    pub parallax_mode: GString,
    /// `strip` (an atlas of an animation's frames in order) or `atlas` (Godot's animation in
    /// the tileset's atlas).
    #[var]
    pub animation_mode: GString,
    /// A Node2D root also for a map with a single layer.
    #[var]
    pub always_wrap_root: bool,
    /// What an object of a class that is no Godot node type becomes: `staticbody`, another
    /// class name of the mapping table, or `node2d`.
    #[var]
    pub unknown_class_fallback: GString,
    /// What an object property (a reference to another object) becomes: `nodepath` (a NodePath
    /// from the node to the target object's node, or the node itself for a Node-typed property
    /// of that name), `geometry` (the target's points, as before 3.1) or `both` (the NodePath,
    /// and the geometry as metadata `<name>_shape`).
    #[var]
    pub object_refs: GString,
    /// Tiled class name (case-insensitive) -> scene path. Objects and tiles of that class are
    /// instantiated like class `instance` with `res_path`. Takes precedence over the class
    /// field mapping (not over a `godot_node_type` property). A relative path is relative to
    /// the map.
    #[var]
    pub class_scenes: VarDictionary,
    /// Also accept a `godot_scene` (file) member on Tiled custom class types (needs the project
    /// file).
    #[var]
    pub read_godot_scene_from_custom_types: bool,
    /// Custom properties that match a property on the created node are assigned with `set()`;
    /// properties that match nothing become metadata (as before). Off: the old behaviour.
    #[var]
    pub assign_node_properties: bool,
    base: Base<Resource>,
}

#[godot_api]
impl IResource for TiledImportOptions {
    fn init(base: Base<Resource>) -> Self {
        let d = Options::default();
        TiledImportOptions {
            use_default_filter: d.use_default_filter,
            add_class_as_metadata: d.add_class_as_metadata,
            add_id_as_metadata: d.add_id_as_metadata,
            no_alternative_tiles: d.no_alternative_tiles,
            map_wangset_to_terrain: d.map_wangset_to_terrain,
            custom_data_prefix: GString::from(d.custom_data_prefix.as_str()),
            save_tileset_to: GString::from(d.save_tileset_to.as_str()),
            parallax_mode: GString::from(d.parallax_mode.as_str()),
            animation_mode: GString::from(d.animation_mode.as_str()),
            always_wrap_root: d.always_wrap_root,
            unknown_class_fallback: GString::from(d.unknown_class_fallback.as_str()),
            object_refs: GString::from(d.object_refs.as_str()),
            class_scenes: VarDictionary::new(),
            read_godot_scene_from_custom_types: d.read_godot_scene_from_custom_types,
            assign_node_properties: d.assign_node_properties,
            base,
        }
    }
}

#[godot_api]
impl TiledImportOptions {}

impl TiledImportOptions {
    /// The options as plain Rust values.
    pub fn snapshot(&self) -> Options {
        Options {
            use_default_filter: self.use_default_filter,
            add_class_as_metadata: self.add_class_as_metadata,
            add_id_as_metadata: self.add_id_as_metadata,
            no_alternative_tiles: self.no_alternative_tiles,
            map_wangset_to_terrain: self.map_wangset_to_terrain,
            custom_data_prefix: self.custom_data_prefix.to_string(),
            save_tileset_to: self.save_tileset_to.to_string(),
            parallax_mode: self.parallax_mode.to_string(),
            animation_mode: self.animation_mode.to_string(),
            always_wrap_root: self.always_wrap_root,
            unknown_class_fallback: self.unknown_class_fallback.to_string(),
            object_refs: self.object_refs.to_string(),
            // `set_class_scenes`: keys in lower case, both sides as `str()` prints them
            class_scenes: self
                .class_scenes
                .iter_shared()
                .map(|(k, v)| {
                    (
                        k.stringify().to_string().to_lowercase(),
                        v.stringify().to_string(),
                    )
                })
                .collect(),
            read_godot_scene_from_custom_types: self.read_godot_scene_from_custom_types,
            assign_node_properties: self.assign_node_properties,
        }
    }
}

/// The options, with the GDScript runtime's defaults.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Options {
    pub use_default_filter: bool,
    pub add_class_as_metadata: bool,
    pub add_id_as_metadata: bool,
    pub no_alternative_tiles: bool,
    pub map_wangset_to_terrain: bool,
    pub custom_data_prefix: String,
    pub save_tileset_to: String,
    pub parallax_mode: String,
    pub animation_mode: String,
    pub always_wrap_root: bool,
    pub unknown_class_fallback: String,
    pub object_refs: String,
    /// `class_scenes`, its keys in lower case, in the dictionary's order.
    pub class_scenes: Vec<(String, String)>,
    pub read_godot_scene_from_custom_types: bool,
    pub assign_node_properties: bool,
}

impl Options {
    /// The scene `class_scenes` maps a class to (compared in lower case), if any.
    pub fn class_scene(&self, class: &str) -> Option<&str> {
        let lower = class.to_lowercase();
        self.class_scenes
            .iter()
            .rev() // a later key wins, as in a Dictionary filled in order
            .find(|(k, _)| *k == lower)
            .map(|(_, v)| v.as_str())
    }
}

impl Default for Options {
    fn default() -> Self {
        Options {
            use_default_filter: false,
            add_class_as_metadata: true,
            add_id_as_metadata: false,
            no_alternative_tiles: true,
            map_wangset_to_terrain: false,
            custom_data_prefix: "data_".to_string(),
            save_tileset_to: String::new(),
            parallax_mode: "parallax2d".to_string(),
            animation_mode: "strip".to_string(),
            always_wrap_root: false,
            unknown_class_fallback: "staticbody".to_string(),
            object_refs: "nodepath".to_string(),
            class_scenes: Vec::new(),
            read_godot_scene_from_custom_types: true,
            assign_node_properties: true,
        }
    }
}

/// Options given as `null`: the defaults.
pub fn options_of(o: Option<Gd<TiledImportOptions>>) -> Options {
    o.map(|o| o.bind().snapshot()).unwrap_or_default()
}
