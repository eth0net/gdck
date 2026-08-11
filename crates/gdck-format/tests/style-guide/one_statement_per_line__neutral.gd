func _ready():
	next_state = "idle" if is_on_floor() else "fall"
