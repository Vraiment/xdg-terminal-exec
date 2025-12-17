#!/usr/bin/env bats

# TODO: Add following tests:
# Ensure that duplicates are removed

assert_failure() {
	[ "$status" -ne 0 ] || {
		echo "status: $status" >&2
		echo "output: $output" >&2
		return 1
	}
}

assert_success() {
	[ "$status" -eq 0 ] || {
		echo "status: $status" >&2
		echo "output: $output" >&2
		return 1
	}
}

assert_output() {
	expected="${*:-$(cat)}"
	[ "$output" = "$expected" ] || {
		echo "status: $status" >&2
		echo "output diff:" >&2
		diff -u <(echo "$expected") <(echo "$output") >&2
		return 1
	}
}
