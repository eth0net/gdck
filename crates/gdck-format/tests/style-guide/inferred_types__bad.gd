# Typed as int, but it could be that float was intended.
var health := 0

# The type hint has redundant information.
var direction: Vector3 = Vector3(1, 2, 3)

# What type is this? It's not immediately clear to the reader, so it's bad.
var value := complex_function()
