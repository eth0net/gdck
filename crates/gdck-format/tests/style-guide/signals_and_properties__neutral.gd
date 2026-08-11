signal player_spawned(position)

enum Job {
	KNIGHT,
	WIZARD,
	ROGUE,
	HEALER,
	SHAMAN,
}

const MAX_LIVES = 3

@export var job: Job = Job.KNIGHT
@export var max_health = 50
@export var attack = 5

var health = max_health:
	set(new_health):
		health = new_health

var _speed = 300.0

@onready var sword = get_node("Sword")
@onready var gun = get_node("Gun")
