use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Command,
};

use assert_cmd::assert::OutputAssertExt;

fn executable_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_xdg-terminal-exec"))
}

fn shell_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("xdg-terminal-exec")
}

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn build_command(command_path: &Path) -> Command {
    let mut command = Command::new(command_path);
    let test_nothing_dir = tests_dir().join("nothing");

    command
        .env_remove("XDG_CURRENT_DESKTOP")
        .env("XDG_CONFIG_HOME", test_nothing_dir.clone())
        .env("XDG_CONFIG_DIRS", test_nothing_dir.clone())
        .env("XDG_DATA_HOME", test_nothing_dir.clone())
        .env("XDG_DATA_DIRS", test_nothing_dir.clone())
        .env(
            "PATH",
            join_os_strs_to_env_var_list(&[
                tests_dir().join("bin").as_os_str(),
                &OsString::from(env!("PATH")),
            ]),
        );

    command
}

/// Joins a slice of `&OsStr` into a `OsString` using `:` as the separator
fn join_os_strs_to_env_var_list(os_strs: &[&OsStr]) -> OsString {
    use std::os::unix::ffi::OsStringExt;

    match os_strs.len() {
        0 => OsString::from(String::new()),
        1 => OsString::from(os_strs.first().unwrap()),
        _ => {
            let strings_size: usize = os_strs.iter().map(|os_str| os_str.len()).sum();
            let mut buffer: Vec<u8> = Vec::with_capacity(strings_size + os_strs.len() - 1);

            buffer.extend(OsString::from(os_strs.first().unwrap()).into_vec());

            let separator = OsString::from(String::from(':')).into_vec();
            for os_str in os_strs[1..].iter() {
                buffer.extend(separator.clone());
                buffer.extend(OsString::from(os_str).into_vec());
            }

            OsString::from_vec(buffer)
        }
    }
}

#[test]
fn uses_globally_configured_entry_with_bash() {
    uses_globally_configured_entry(&shell_script_path());
}

#[test]
#[ignore]
fn uses_globally_configured_entry_with_rust() {
    uses_globally_configured_entry(&executable_path());
}

fn uses_globally_configured_entry(command_path: &Path) {
    build_command(command_path)
        .env(
            "XDG_CONFIG_DIRS",
            tests_dir().join("config").join("default"),
        )
        .env("XDG_DATA_DIRS", tests_dir().join("data").join("default"))
        .assert()
        .success()
        .stdout("default terminal\n");
}

#[test]
fn ignores_missing_config_directory_with_bash() {
    ignores_missing_config_directory(&shell_script_path());
}

#[test]
#[ignore]
fn ignores_missing_config_directory_with_rust() {
    ignores_missing_config_directory(&executable_path());
}

fn ignores_missing_config_directory(command_path: &Path) {
    let xdg_config_dirs = join_os_strs_to_env_var_list(&[
        tests_dir().join("missing").as_os_str(),
        tests_dir().join("config").join("default").as_os_str(),
    ]);

    build_command(command_path)
        .env("XDG_CONFIG_DIRS", xdg_config_dirs)
        .env("XDG_DATA_DIRS", tests_dir().join("data").join("default"))
        .assert()
        .success()
        .stdout("default terminal\n");
}

#[test]
fn ignores_missing_data_directory_with_bash() {
    ignores_missing_data_directory(&shell_script_path());
}

#[test]
#[ignore]
fn ignores_missing_data_directory_with_rust() {
    ignores_missing_data_directory(&executable_path());
}

fn ignores_missing_data_directory(command_path: &Path) {
    let xdg_data_dirs = join_os_strs_to_env_var_list(&[
        tests_dir().join("missing").as_os_str(),
        tests_dir().join("data").join("default").as_os_str(),
    ]);

    build_command(command_path)
        .env(
            "XDG_CONFIG_DIRS",
            tests_dir().join("config").join("default"),
        )
        .env("XDG_DATA_DIRS", xdg_data_dirs)
        .assert()
        .success()
        .stdout("default terminal\n");
}

#[test]
fn uses_locally_configured_entry_with_bash() {
    uses_locally_configured_entry(&shell_script_path());
}

#[test]
#[ignore]
fn uses_locally_configured_entry_with_rust() {
    uses_locally_configured_entry(&executable_path());
}

fn uses_locally_configured_entry(command_path: &Path) {
    build_command(command_path)
        .env(
            "XDG_CONFIG_HOME",
            tests_dir().join("config").join("default"),
        )
        .env("XDG_DATA_HOME", tests_dir().join("data").join("default"))
        .assert()
        .success()
        .stdout("default terminal\n");
}

#[test]
fn finds_any_global_entry_when_there_is_no_configuration_with_bash() {
    finds_any_global_entry_when_there_is_no_configuration(&shell_script_path());
}

#[test]
#[ignore]
fn finds_any_global_entry_when_there_is_no_configuration_with_rust() {
    finds_any_global_entry_when_there_is_no_configuration(&executable_path());
}

fn finds_any_global_entry_when_there_is_no_configuration(command_path: &Path) {
    build_command(command_path)
        .env("XDG_DATA_DIRS", tests_dir().join("data").join("default"))
        .assert()
        .success()
        .stdout("default terminal\n");
}

#[test]
fn uses_configured_exec_arg_with_bash() {
    uses_configured_exec_arg(&shell_script_path());
}

#[test]
#[ignore]
fn uses_configured_exec_arg_with_rust() {
    uses_configured_exec_arg(&executable_path());
}

fn uses_configured_exec_arg(command_path: &Path) {
    build_command(command_path)
        .env("XDG_DATA_DIRS", tests_dir().join("data").join("execarg"))
        .arg("argument")
        .assert()
        .success()
        .stdout("TerminalArgExec terminal -- argument\n");
}

#[test]
fn adds_default_exec_arg_with_bash() {
    adds_default_exec_arg(&shell_script_path());
}

#[test]
#[ignore]
fn adds_default_exec_arg_with_rust() {
    adds_default_exec_arg(&executable_path());
}

fn adds_default_exec_arg(command_path: &Path) {
    build_command(command_path)
        .env("XDG_DATA_DIRS", tests_dir().join("data").join("default"))
        .arg("argument")
        .assert()
        .success()
        .stdout("default terminal -e argument\n");
}

#[test]
fn deals_with_large_desktop_entries_with_bash() {
    deals_with_large_desktop_entries(&shell_script_path());
}

#[test]
#[ignore]
fn deals_with_large_desktop_entries_with_rust() {
    deals_with_large_desktop_entries(&executable_path());
}

fn deals_with_large_desktop_entries(command_path: &Path) {
    build_command(command_path)
        .env("XDG_DATA_DIRS", tests_dir().join("data").join("huge"))
        .assert()
        .success()
        .stdout("huge terminal\n");
}

#[test]
fn finds_any_local_entry_when_there_is_no_configuration_with_bash() {
    finds_any_local_entry_when_there_is_no_configuration(&shell_script_path());
}

#[test]
#[ignore]
fn finds_any_local_entry_when_there_is_no_configuration_with_rust() {
    finds_any_local_entry_when_there_is_no_configuration(&executable_path());
}

fn finds_any_local_entry_when_there_is_no_configuration(command_path: &Path) {
    build_command(command_path)
        .env("XDG_DATA_HOME", tests_dir().join("data").join("default"))
        .assert()
        .success()
        .stdout("default terminal\n");
}

#[test]
fn prefers_earlier_configured_entry_with_rust_with_bash() {
    prefers_earlier_configured_entry(&shell_script_path());
}

#[test]
#[ignore]
fn prefers_earlier_configured_entry_with_rust() {
    prefers_earlier_configured_entry(&executable_path());
}

fn prefers_earlier_configured_entry(command_path: &Path) {
    build_command(command_path)
        .env(
            "XDG_CONFIG_DIRS",
            join_os_strs_to_env_var_list(&[
                tests_dir().join("config").join("preferred").as_os_str(),
                tests_dir().join("config").join("default").as_os_str(),
            ]),
        )
        .env(
            "XDG_DATA_DIRS",
            join_os_strs_to_env_var_list(&[
                tests_dir().join("data").join("preferred").as_os_str(),
                tests_dir().join("data").join("default").as_os_str(),
            ]),
        )
        .assert()
        .success()
        .stdout("preferred terminal\n");
}

#[test]
fn prefers_locally_configured_entry_with_bash() {
    prefers_locally_configured_entry(&shell_script_path());
}

#[test]
#[ignore]
fn prefers_locally_configured_entry_with_rust() {
    prefers_locally_configured_entry(&executable_path());
}

fn prefers_locally_configured_entry(command_path: &Path) {
    build_command(command_path)
        .env(
            "XDG_CONFIG_HOME",
            tests_dir().join("config").join("preferred"),
        )
        .env(
            "XDG_CONFIG_DIRS",
            tests_dir().join("config").join("default"),
        )
        .env("XDG_DATA_HOME", tests_dir().join("data").join("preferred"))
        .env("XDG_DATA_DIRS", tests_dir().join("data").join("default"))
        .assert()
        .success()
        .stdout("preferred terminal\n");
}

#[test]
fn ignores_hidden_entry_with_bash() {
    ignores_hidden_entry(&shell_script_path());
}

#[test]
#[ignore]
fn ignores_hidden_entry_with_rust() {
    ignores_hidden_entry(&executable_path());
}

fn ignores_hidden_entry(command_path: &Path) {
    build_command(command_path)
        .env("XDG_CONFIG_HOME", tests_dir().join("config").join("hidden"))
        .env(
            "XDG_CONFIG_DIRS",
            tests_dir().join("config").join("default"),
        )
        .env("XDG_DATA_HOME", tests_dir().join("data").join("hidden"))
        .env("XDG_DATA_DIRS", tests_dir().join("data").join("default"))
        .assert()
        .success()
        .stdout("default terminal\n");
}

#[test]
fn ignores_entry_when_its_tryexec_fails_with_bash() {
    ignores_entry_when_its_tryexec_fails(&shell_script_path());
}

#[test]
#[ignore]
fn ignores_entry_when_its_tryexec_fails_with_rust() {
    ignores_entry_when_its_tryexec_fails(&executable_path());
}

fn ignores_entry_when_its_tryexec_fails(command_path: &Path) {
    build_command(command_path)
        .env(
            "XDG_CONFIG_HOME",
            tests_dir().join("config").join("tryexec-fails"),
        )
        .env(
            "XDG_CONFIG_DIRS",
            tests_dir().join("config").join("default"),
        )
        .env(
            "XDG_DATA_HOME",
            tests_dir().join("data").join("tryexec-fails"),
        )
        .env("XDG_DATA_DIRS", tests_dir().join("data").join("default"))
        .assert()
        .success()
        .stdout("default terminal\n");
}
