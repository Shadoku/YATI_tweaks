extends Area2D
# No class_name: the class is named after the scene file (door.tscn -> Door).

@export var locked: bool = false
@export var key: Item.Kind = Item.Kind.KEY
@export var target: NodePath
@export var width: float = 32.0
