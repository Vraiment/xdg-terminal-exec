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

@test "fails on globally configured entry with missing action" {
	export XDG_CONFIG_DIRS="$BATS_TEST_DIRNAME/config/missing-action"
	export XDG_DATA_DIRS="$BATS_TEST_DIRNAME/data/default"
	run "$XTE"
	assert_failure
}

@test "ignores comments, blank lines, and trailing whitespace" {
	export XDG_CONFIG_DIRS="$BATS_TEST_DIRNAME/config/whitespace"
	export XDG_DATA_DIRS="$BATS_TEST_DIRNAME/data/default"
	run "$XTE"
	assert_success
	assert_output "default terminal"
}

@test "output of --print* options" {
	export XDG_DATA_HOME="$BATS_TEST_DIRNAME/data/preferred"
	run "$XTE" --print-cmd=';;' --print-path --print-id --print-delimiter='\n\n' and 'custom arguments' 'with
newline'
	assert_success
	assert_output <<- EOF
		preferred-term.desktop
		
		${XDG_DATA_HOME}/applications/preferred-term.desktop
		
		echo;;preferred;;terminal;;-e;;and;;custom arguments;;with
		newline
	EOF
}
