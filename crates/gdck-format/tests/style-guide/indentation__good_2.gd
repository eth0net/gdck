func _ready():
	effect.interpolate_property(sprite, "transform/scale",
			sprite.get_scale(), Vector2(2.0, 2.0), 0.3,
			Tween.TRANS_QUAD, Tween.EASE_OUT)
