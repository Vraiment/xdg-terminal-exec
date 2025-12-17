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

@test "ignores entry when its TryExec fails" {
	export XDG_CONFIG_HOME="$BATS_TEST_DIRNAME/config/tryexec-fails"
	export XDG_CONFIG_DIRS="$BATS_TEST_DIRNAME/config/default"
	export XDG_DATA_HOME="$BATS_TEST_DIRNAME/data/tryexec-fails"
	export XDG_DATA_DIRS="$BATS_TEST_DIRNAME/data/default"
	run "$XTE"
	assert_success
	assert_output "default terminal"
}

@test "uses desktop-specific configuration when available" {
	export XDG_CONFIG_HOME="$BATS_TEST_DIRNAME/config/desktop/lists"
	export XDG_CONFIG_DIRS="$BATS_TEST_DIRNAME/config/default"
	export XDG_DATA_HOME="$BATS_TEST_DIRNAME/data/desktop/lists"
	export XDG_DATA_DIRS="$BATS_TEST_DIRNAME/data/default"
	export XDG_CURRENT_DESKTOP=desktop
	run "$XTE"
	assert_success
	assert_output "specific terminal"
}

@test "uses desktop-agnostic configuration when none is available" {
	export XDG_CONFIG_HOME="$BATS_TEST_DIRNAME/config/desktop/lists"
	export XDG_CONFIG_DIRS="$BATS_TEST_DIRNAME/config/default"
	export XDG_DATA_HOME="$BATS_TEST_DIRNAME/data/desktop/lists"
	export XDG_DATA_DIRS="$BATS_TEST_DIRNAME/data/default"
	export XDG_CURRENT_DESKTOP=other
	run "$XTE"
	assert_success
	assert_output "generic terminal"
}

@test "considers entry when its OnlyShowIn matches" {
	export XDG_CONFIG_HOME="$BATS_TEST_DIRNAME/nothing"
	export XDG_CONFIG_DIRS="$BATS_TEST_DIRNAME/nothing"
	export XDG_DATA_HOME="$BATS_TEST_DIRNAME/data/desktop/onlyshow"
	export XDG_DATA_DIRS="$BATS_TEST_DIRNAME/data/default"
	export XDG_CURRENT_DESKTOP=only:not
	run "$XTE"
	assert_success
	assert_output "only terminal"
}

@test "considers entry when its NotShowIn does not match" {
	export XDG_CONFIG_HOME="$BATS_TEST_DIRNAME/nothing"
	export XDG_CONFIG_DIRS="$BATS_TEST_DIRNAME/nothing"
	export XDG_DATA_HOME="$BATS_TEST_DIRNAME/data/desktop/notshow"
	export XDG_DATA_DIRS="$BATS_TEST_DIRNAME/data/default"
	export XDG_CURRENT_DESKTOP=other
	run "$XTE"
	assert_success
	assert_output "not terminal"
}

@test "ignores entry when its NotShowIn matches or its OnlyShowIn does not match" {
	export XDG_CONFIG_HOME="$BATS_TEST_DIRNAME/nothing"
	export XDG_CONFIG_DIRS="$BATS_TEST_DIRNAME/nothing"
	export XDG_DATA_HOME="$BATS_TEST_DIRNAME/data/desktop/show"
	export XDG_DATA_DIRS="$BATS_TEST_DIRNAME/data/default"
	export XDG_CURRENT_DESKTOP=not
	run "$XTE"
	assert_success
	assert_output "generic terminal"
}

@test "quotes commands and arguments correctly" {
	export XDG_DATA_HOME="$BATS_TEST_DIRNAME/data/quoting"
	run "$XTE" and 'custom arguments' 'with
newline'
	assert_success
	assert_output <<-'EOF'
		|||quoting terminal|||
		|||with 'complex' arguments,|||
		|||quotes ",|||
		||||||
		|||empty args,|||
		|||new
		lines,|||
		|||and "back\slashes"|||
		|||-e|||
		|||and|||
		|||custom arguments|||
		|||with
		newline|||
	EOF
}

@test "uses globally configured entry with custom action" {
	export XDG_CONFIG_DIRS="$BATS_TEST_DIRNAME/config/custom-action"
	export XDG_DATA_DIRS="$BATS_TEST_DIRNAME/data/default"
	run "$XTE"
	assert_success
	assert_output "default terminal - custom action"
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
