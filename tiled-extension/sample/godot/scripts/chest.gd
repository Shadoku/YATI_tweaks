extends StaticBody2D
# No class_name: the class is named after the scene, Chest.

@export var locked: bool = false
@export_file("*.tres") var loot: String = "res://loot/gold.tres"
@export_enum("Wood", "Iron", "Gold") var material: String = "Wood"
