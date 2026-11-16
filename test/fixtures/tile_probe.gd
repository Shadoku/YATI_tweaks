extends Node2D
## info.tile fixture (tile_info.tmx): the scene tile objects of class Bush map to. Its exports
## receive the tile's custom properties and the object's.

@export var hp: int = 0
@export var slashable: bool = false

var imported: Dictionary = {}
var hook_calls: int = 0


func _tiled_import(info: Dictionary) -> void:
	imported = info
	hook_calls += 1
