func _ready():
	# Normal string.
	print("hello world")

	# Use double quotes as usual to avoid escapes.
	print("hello 'world'")

	# Use single quotes as an exception to the rule to avoid escapes.
	print('hello "world"')

	# Both quote styles would require 2 escapes; prefer double quotes if it's a tie.
	print("'hello' \"world\"")
