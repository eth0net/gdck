func _ready():
	if position.x > width: position.x = 0

	if flag: print("flagged")
