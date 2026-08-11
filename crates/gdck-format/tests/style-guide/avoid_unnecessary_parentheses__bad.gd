func _ready():
	if (is_colliding()):
		queue_free()
