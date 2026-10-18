extends CharacterBody2D
## A base script without class_name: enemy.gd extends it by path and inherits its exports and enum.

enum Team { RED, BLUE }

@export var max_hp: int = 10
@export var team: Team = Team.BLUE
