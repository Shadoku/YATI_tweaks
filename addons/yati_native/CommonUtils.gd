# The counters of the last import, as the GDScript runtime's CommonUtils.gd keeps them (static, per
# process): Importer.gd sets them after every import, so code (and the test suite's helpers) that
# read CommonUtils.error_count / warning_count of the runtime read the same here. Prefer the
# importer's own last_error_count / last_warning_count.
extends RefCounted

static var error_count: int = 0
static var warning_count: int = 0
