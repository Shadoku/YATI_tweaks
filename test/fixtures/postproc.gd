extends Node
## Post-processor used by run_tests.gd (test_post_processing).

func _post_process(base_node: Node2D) -> Node2D:
	base_node.set_meta("post_processed", true)
	queue_free() # the importer instantiates this Node and never frees it
	return base_node
