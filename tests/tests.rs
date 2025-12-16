use std::{
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
    let test_bin_dir = tests_dir()
        .join("bin")
        .into_os_string()
        .into_string()
        .expect("Failed to get tests/bin directory");

    command
        .env_remove("XDG_CURRENT_DESKTOP")
        .env("XDG_CONFIG_HOME", test_nothing_dir.clone())
        .env("XDG_CONFIG_DIRS", test_nothing_dir.clone())
        .env("XDG_DATA_HOME", test_nothing_dir.clone())
        .env("XDG_DATA_DIRS", test_nothing_dir.clone())
        .env("PATH", format!("{}:{}", test_bin_dir, env!("PATH")));

    command
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
    let xdg_config_dirs = [
        tests_dir().join("missing"),
        tests_dir().join("config").join("default"),
    ]
    .map(|path| path.into_os_string().into_string().unwrap())
    .join(":");

    build_command(command_path)
        .env("XDG_CONFIG_DIRS", xdg_config_dirs)
        .env("XDG_DATA_DIRS", tests_dir().join("data").join("default"))
        .assert()
        .success()
        .stdout("default terminal\n");
}
