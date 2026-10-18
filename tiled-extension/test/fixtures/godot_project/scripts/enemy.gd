class_name Enemy
extends "res://scripts/base_unit.gd"
## Every export form the extension reads.

enum Mood { CALM, ANGRY = 5, SLEEPY }
enum { ANONYMOUS_A, ANONYMOUS_B }

const DEFAULT_SPEED := 120.0

@export_category("Stats")
@export var speed: float = 2.5
@export var level: int = 3
@export var boss: bool = true
@export var faction: String = "orcs"
@export var tag: StringName = &"grunt"
@export var mood: Mood = Mood.ANGRY
@export var loot_kind: Item.Kind = Item.Kind.GEM
@export var tint: Color = Color(1, 0.5, 0, 1)
@export var outline_color: Color = Color.RED
@export var plain_color: Color
@export_group("Movement")
@export var offset: Vector2 = Vector2(4, -8.5)
@export var cell: Vector2i = Vector2i(2, 3)
@export var direction := Vector2.LEFT
@export_range(0.0, 10.0, 0.5, "or_greater") var jump: float = 1.0
@export_range(1, 12) var lives: int = 3:
	set(value):
		lives = clampi(value, 1, 12)
@export var cruise: float = DEFAULT_SPEED
@export_subgroup("References")
@export var patrol: NodePath
@export var target: Node2D
@export var arena: Arena
@export_node_path("Marker2D") var spawn_point: NodePath
@export_group("")
@export var icon: Texture2D = preload("res://icon.png")
@export var loot: Item
@export_file("*.json") var dialog: String = "res://data/dialog.json"
@export_dir var sounds: String = "res://sounds"
@export_multiline var description: String = "A grunt.\nAngry."
@export_placeholder("Name") var nickname: String
@export_enum("Walk", "Run:5", "Fly") var gait: int = 5
@export_enum("North", "South") var heading: String = "South"
@export_flags_2d_physics var hit_layers: int = 5
@export_flags("Fire", "Water") var elements: int = 0
@export
var drops: Array[int] = [1, 2]
@export var path_points: PackedVector2Array = PackedVector2Array()
@export var extra: Dictionary = {}
@export var counts: Dictionary[String, int] = {}
@export var untyped_number = 7
@export var inferred_text := "hi"
@export var setter_inline: int = 4: set = _set_setter_inline
@export_storage var hidden_state: int = 0
@export var godot_scene: String = "not a member"
var not_exported: int = 1
@onready var also_not_exported: Node = $Child


func _set_setter_inline(v: int) -> void:
	setter_inline = v


class Inner:
	@export var inner_export: int = 1
