class_name Slime
extends CharacterBody2D
## An enemy: what a Tiled object of class Slime sets.

enum Mood { CALM, ANGRY }

@export var hp: int = 3
@export var speed: float = 40.0
@export var mood: Mood = Mood.CALM
@export var tint: Color = Color(0.4, 0.8, 0.3)
@export var patrol: NodePath
@export var home: Vector2 = Vector2(0, 0)
@export_flags_2d_physics var hits: int = 2
@export var drops: Array[String] = []
