func heal(amount):
	health += amount
	health = min(health, max_health)
	health_changed.emit(health)


func take_damage(amount, effect=null):
	health -= amount
	health = max(0, health)
	health_changed.emit(health)
