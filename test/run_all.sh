#!/bin/sh
# Runs the YATI runtime test suite and the benchmark headless.
# Usage: GODOT=/path/to/godot test/run_all.sh [--runtime=res://runtime]
set -e
cd "$(dirname "$0")/.."
GODOT="${GODOT:-godot}"
RUNTIME="${1:---runtime=res://runtime}"
status=0
"$GODOT" --headless -s test/run_tests.gd -- "$RUNTIME" || status=$?
"$GODOT" --headless -s test/benchmark.gd -- "$RUNTIME"
exit $status
