class_name Item extends Resource
## A Resource class with an enum another script uses as Item.Kind.

enum Kind {
	KEY,
	GEM = 4,
	COIN,
}

@export var kind: Kind = Kind.KEY
