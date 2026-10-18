extends Node2D
## F1 fixture: the scene a class 'Enemy' maps to. Its exports receive the object's custom properties.

enum Mood { CALM, ANGRY = 5 }

@export var hp: int = 1
@export var faction: String = ""
@export var patrol: NodePath
@export var speed: float = 0.0
@export var mood: Mood = Mood.CALM
@export var tag: StringName
@export var icon: Texture2D
@export var loot: Dictionary = {}
@export var drops: Array[int] = []

var imported: Dictionary = {}
var hook_calls: int = 0


func _tiled_import(info: Dictionary) -> void:
	imported = info
	hook_calls += 1
