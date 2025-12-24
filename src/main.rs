use std::{
    env::{self, VarError},
    path::Path,
    process::exit,
};

use xdg_terminal_exec::{
    debug::{Debugger, build_debugger},
    emplace_to_csv_list, push_to_csv_list,
};

#[derive(Default)]
struct Globals {
    lowercase_xdg_current_desktop: String,
    //compat mode
    execarg_compat: String,
    // flag reused in directive encounter
    execarg_compat_configured: String,
    exec_usep: String,
    expanded_str: String,
    entry_path: String,
    entry_action: String,
    // this will receive proper value later
    applications_dirs: String,
    // this will be filled with values from /execarg_default:*:* directives
    execarg_defaults: String,
    // the following values are updated by reset_keys on the original implementation
    // Init vars used in entry checks
    is_terminal: String,
    // exec_usep: String, // duplicated
    execarg: String,
    execarg_defined: String,
    appidarg: String,
    titlearg: String,
    dirarg: String,
    holdarg: String,
    // the following values are updated by make_paths on the original implementation
    // Global constants and lists for later use and iterator
    configs: String,
    // applications_dirs: String, // duplicated
    cache_file: String,
    xdg_cache_home: String,
}

fn main() {
    // Skip the program name
    let args: Vec<String> = env::args().skip(0).collect();

    for arg in &args {
        match arg.as_str() {
            "--" => break,
            "-h" | "--help" => print_help_and_exit(),
            _ => {}
        }
    }

    let mut xte = Globals::default();
    xte.lowercase_xdg_current_desktop = env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_lowercase();
    xte.execarg_compat = env::var("XTE_EXECARG_COMPAT").unwrap_or(String::from("true"));
    xte.execarg_compat_configured = env::var("XTE_EXECARG_COMPAT").unwrap_or_default();
    reset_keys(&mut xte);

    let debugger = build_debugger();
    make_paths(&debugger, &mut xte).unwrap();
}

fn reset_keys(xte: &mut Globals) {
    xte.is_terminal = String::new();
    xte.exec_usep = String::new();
    xte.execarg = String::from("-e");
    xte.execarg_defined = String::from("false");
    xte.appidarg = String::new();
    xte.titlearg = String::new();
    xte.dirarg = String::new();
    xte.holdarg = String::new();
}

fn make_paths(debugger: &Box<dyn Debugger>, xte: &mut Globals) -> Result<(), VarError> {
    // Populate list of config files to read, in descending order of preference
    let config_dirs = format!(
        "{}:{}",
        env::var("XDG_CONFIG_HOME")
            .or_else(|_| env::var("HOME").map(|value| format!("{value}/.config")))?,
        env::var("XDG_CONFIG_DIRS").unwrap_or(String::from("/etc/xdg"))
    );
    for xte_dir in config_dirs.split(':') {
        // Normalise base path and append the data subdirectory with a trailing '/'
        let xte_dir = match xte_dir.strip_suffix('/') {
            Some(value) => value,
            None => xte_dir,
        };

        if !xte.lowercase_xdg_current_desktop.is_empty() {
            for xte_desktop in xte.lowercase_xdg_current_desktop.split(':') {
                push_to_csv_list(
                    &mut xte.configs,
                    &format!("{xte_dir}/{xte_desktop}-xdg-terminals.list"),
                );
            }
        }

        push_to_csv_list(&mut xte.configs, &format!("{xte_dir}/xdg-terminals.list"));
    }

    // append xdg-terminal-exec dirs in XDG_DATA_DIRS to config hierarchy for distro/upstream level defaults
    let xdg_data_dirs =
        env::var("XDG_DATA_DIRS").unwrap_or(String::from("/usr/local/share:/usr/share"));
    for xte_dir in xdg_data_dirs.split(':') {
        // Normalise base path and append the data subdirectory with a trailing '/'
        let xte_dir = match xte_dir.strip_suffix('/') {
            Some(value) => value,
            None => xte_dir,
        };

        if !xte.lowercase_xdg_current_desktop.is_empty() {
            for xte_desktop in xte.lowercase_xdg_current_desktop.split(':') {
                push_to_csv_list(
                    &mut xte.configs,
                    &format!("{xte_dir}/xdg-terminal-exec/{xte_desktop}-xdg-terminals.list"),
                );
            }
        }

        push_to_csv_list(
            &mut xte.configs,
            &format!("{xte_dir}/xdg-terminal-exec/xdg-terminals.list"),
        );
    }

    // Populate list of directories to search for entries in, in ascending order of preference
    let data_home_dirs = format!(
        "{}:{}",
        env::var("XDG_DATA_HOME")
            .or_else(|_| env::var("HOME").map(|value| format!("{value}/.local/share")))?,
        env::var("XDG_DATA_DIRS").unwrap_or(String::from("/usr/local/share:/usr/share"))
    );
    for xte_dir in data_home_dirs.split(':') {
        // Normalise base path and append the data subdirectory with a trailing '/'
        let xte_dir = match xte_dir.strip_suffix('/') {
            Some(value) => value,
            None => xte_dir,
        };

        emplace_to_csv_list(
            &mut xte.applications_dirs,
            &format!("{xte_dir}/applications/"),
        );
    }

    xte.xdg_cache_home = env::var("XDG_CACHE_HOME")
        .or_else(|_| env::var("HOME").map(|value| format!("{value}/.cache")))?;
    xte.cache_file = format!("{}/xdg-terminal-exec", xte.xdg_cache_home);

    debugger.print(&[
        &"paths:",
        &format!("  XTE__CONFIGS={}", xte.configs),
        &format!("  XTE__APPLICATIONS_DIRS={}", xte.applications_dirs),
    ]);

    Ok(())
}

fn print_help_and_exit() -> ! {
    use std::ffi::OsStr;

    let self_name = env::args()
        .next()
        .as_ref()
        .map(Path::new)
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
        .map(String::from)
        .unwrap_or(String::from(env!("CARGO_PKG_NAME")));

    println!(
        r#"{self_name} - Shell-based default terminal launcher.

Implementation of the proposed Default Terminal Specification.

Usage:
  {self_name} [options] [--] [command [arguments ...]]

Launches given command in default terminal, or launches default terminal.

Options for modifying terminal behavior (if supported by terminals's
entry):

  --app-id=app-id    set app-id (Wayland) or window class (X11)
  --title=title      set tile of terminal.
  --dir=workdir      set workdir of terminal.
  --hold             instruct terminal to hold after command ends.

Options for printing data instead of executing terminal:

  --print-id
      print selected Desktop Entry ID. Action is appended delimited
      by ":".

  --print-path
      print path to selected Desktop Entry. Action is appended
      delimited by ":".

  --print-content
      print content of selected Desktop Entry. Conflicts with --print-cmd.

  --print-cmd[=printf_sequence]
      print resulting command line, delimited by given printf sequence,
      "\n" by default. If sequence is "\n", output is also terminated with
      a newline.

  --print-delimiter=printf_sequence
      printf sequence to be used as the delimiter between multiple requested
      print statements, "\n" by default. If sequence is "\n", output is
      also terminated with a newline.

Configuration:

Preferred terminals are configured by listing their Desktop Entry IDs
in config files named "[${{desktop}}-]xdg-terminals.list" placed in XDG Config
hierarchy. Where "${{desktop}}" is a lowercased string that is matched
(case-insensitively) against items of "${{XDG_CURRENT_DESKTOP}}".

See "man {self_name}" for more details."#
    );

    exit(0)
}

#[cfg(test)]
mod test {
    use std::{
        collections::HashMap,
        env::{self, VarError},
        ffi::OsStr,
        sync::{LazyLock, Mutex},
    };

    use xdg_terminal_exec::debug::build_debugger;

    use crate::*;

    static ENV_LOCK: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

    #[test]
    fn test_reset_keys() {
        let mut xte = Globals::default();

        xte.is_terminal = String::from("Not default value for is_terminal");
        xte.exec_usep = String::from("Not default value for exec_usep");
        xte.execarg = String::from("Not default value for execarg");
        xte.execarg_defined = String::from("Not default value for execarg_defined");
        xte.appidarg = String::from("Not default value for appidarg");
        xte.titlearg = String::from("Not default value for titlearg");
        xte.dirarg = String::from("Not default value for dirarg");
        xte.holdarg = String::from("Not default value for holdarg");

        reset_keys(&mut xte);

        assert!(xte.is_terminal.is_empty());
        assert!(xte.exec_usep.is_empty());
        assert_eq!(xte.execarg, String::from("-e"));
        assert_eq!(xte.execarg_defined, String::from("false"));
        assert!(xte.appidarg.is_empty());
        assert!(xte.titlearg.is_empty());
        assert!(xte.dirarg.is_empty());
        assert!(xte.holdarg.is_empty());
    }

    fn overwrite_env<K: AsRef<OsStr>>(env: HashMap<K, Option<String>>) {
        env.iter().for_each(|(name, value)| {
            match value {
                Some(value) => unsafe {
                    env::set_var(name, value);
                },
                None => unsafe {
                    env::remove_var(name);
                },
            };
        });
    }

    fn with_env<K, F, R>(env: HashMap<K, Option<String>>, function: &mut F) -> R
    where
        K: AsRef<OsStr> + Eq + std::hash::Hash + Copy,
        F: FnMut() -> R,
    {
        let _mutex_guard = ENV_LOCK.lock().unwrap();
        let mut original_env: HashMap<K, Option<String>> = HashMap::new();

        env.keys().for_each(|name: &K| {
            original_env.insert(
                name.clone(),
                match env::var(name) {
                    Err(VarError::NotPresent) => None,
                    val => Some(val.unwrap()),
                },
            );
        });

        overwrite_env(env);

        let result = function();

        overwrite_env(original_env);

        result
    }

    #[test]
    fn test_make_paths_updates_the_configs_value() {
        let debugger = build_debugger();
        let mut xte = Globals::default();

        with_env(
            HashMap::from([
                ("HOME", Some(String::from("/home/user"))),
                (
                    "XDG_CONFIG_HOME",
                    Some(String::from("/home/user/.custom_config")),
                ),
                ("XDG_CONFIG_DIRS", Some(String::from("/etc/custom_config"))),
                ("XDG_DATA_DIRS", Some(String::from("/etc/custom_data"))),
            ]),
            &mut || make_paths(&debugger, &mut xte),
        )
        .unwrap();

        assert_eq!(
            xte.configs,
            String::from(
                "/home/user/.custom_config/xdg-terminals.list:\
                /etc/custom_config/xdg-terminals.list:\
                /etc/custom_data/xdg-terminal-exec/xdg-terminals.list"
            )
        );
    }

    #[test]
    fn test_make_paths_updates_the_configs_value_when_desktops_are_defined() {
        let debugger = build_debugger();
        let mut xte = Globals::default();

        xte.lowercase_xdg_current_desktop = String::from("distro:DE");

        with_env(
            HashMap::from([
                ("HOME", Some(String::from("/home/user"))),
                (
                    "XDG_CONFIG_HOME",
                    Some(String::from("/home/user/.custom_config")),
                ),
                ("XDG_CONFIG_DIRS", Some(String::from("/etc/custom_config"))),
                ("XDG_DATA_DIRS", Some(String::from("/etc/custom_data"))),
            ]),
            &mut || make_paths(&debugger, &mut xte),
        )
        .unwrap();

        assert_eq!(
            xte.configs,
            String::from(
                "/home/user/.custom_config/distro-xdg-terminals.list:\
                /home/user/.custom_config/DE-xdg-terminals.list:\
                /home/user/.custom_config/xdg-terminals.list:\
                /etc/custom_config/distro-xdg-terminals.list:\
                /etc/custom_config/DE-xdg-terminals.list:\
                /etc/custom_config/xdg-terminals.list:\
                /etc/custom_data/xdg-terminal-exec/distro-xdg-terminals.list:\
                /etc/custom_data/xdg-terminal-exec/DE-xdg-terminals.list:\
                /etc/custom_data/xdg-terminal-exec/xdg-terminals.list"
            )
        );
    }

    #[test]
    fn test_make_paths_updates_applications_dirs() {
        let debugger = build_debugger();
        let mut xte = Globals::default();

        with_env(
            HashMap::from([
                ("HOME", Some(String::from("/home/user"))),
                (
                    "XDG_DATA_HOME",
                    Some(String::from(
                        "/home/user/.custom_data1:/home/user/.custom_data2",
                    )),
                ),
                (
                    "XDG_DATA_DIRS",
                    Some(String::from("/etc/custom_data1:/etc/custom_data2")),
                ),
            ]),
            &mut || make_paths(&debugger, &mut xte),
        )
        .unwrap();

        assert_eq!(
            xte.applications_dirs,
            String::from(
                "/etc/custom_data2/applications/:\
                /etc/custom_data1/applications/:\
                /home/user/.custom_data2/applications/:\
                /home/user/.custom_data1/applications/"
            )
        );
    }

    #[test]
    fn test_make_paths_updates_the_cache_values() {
        let debugger = build_debugger();
        let mut xte = Globals::default();

        with_env(
            HashMap::from([
                ("HOME", Some(String::from("/home/user"))),
                ("XDG_CACHE_HOME", Some(String::from("/tmp/cache"))),
            ]),
            &mut || make_paths(&debugger, &mut xte),
        )
        .unwrap();

        assert_eq!(xte.xdg_cache_home, String::from("/tmp/cache"));
        assert_eq!(xte.cache_file, String::from("/tmp/cache/xdg-terminal-exec"));
    }

    #[test]
    fn test_make_paths_updates_the_cache_values_when_no_xdg_cache_home_set() {
        let debugger = build_debugger();
        let mut xte = Globals::default();

        with_env(
            HashMap::from([
                ("HOME", Some(String::from("/home/user"))),
                ("XDG_CACHE_HOME", None),
            ]),
            &mut || make_paths(&debugger, &mut xte),
        )
        .unwrap();

        assert_eq!(xte.xdg_cache_home, String::from("/home/user/.cache"));
        assert_eq!(
            xte.cache_file,
            String::from("/home/user/.cache/xdg-terminal-exec")
        );
    }

    #[test]
    fn test_make_paths_but_xdg_config_home_and_home_are_not_set() -> Result<(), String> {
        let debugger = build_debugger();
        let mut xte = Globals::default();

        let result = with_env(
            HashMap::from([("HOME", None), ("XDG_CONFIG_HOME", None)]),
            &mut || make_paths(&debugger, &mut xte),
        );

        match result {
            Err(VarError::NotPresent) => Ok(()),
            Ok(()) => Err(String::from(
                "Expected make_paths to fail with variable not present",
            )),
            Err(VarError::NotUnicode(name)) => Err(format!(
                "Expected make_paths to fail with variable not present, instead if failed due a unicode error on {:?}",
                name
            )),
        }
    }

    #[test]
    fn test_make_paths_but_xdg_data_home_and_home_are_not_set() -> Result<(), String> {
        let debugger = build_debugger();
        let mut xte = Globals::default();

        let result = with_env(
            HashMap::from([
                ("HOME", None),
                // XDG_CONFIG_HOME is required to be present for this fallback
                ("XDG_CONFIG_HOME", Some(String::from("/home/user"))),
                ("XDG_DATA_HOME", None),
            ]),
            &mut || make_paths(&debugger, &mut xte),
        );

        match result {
            Err(VarError::NotPresent) => Ok(()),
            Ok(()) => Err(String::from(
                "Expected make_paths to fail with variable not present",
            )),
            Err(VarError::NotUnicode(name)) => Err(format!(
                "Expected make_paths to fail with variable not present, instead if failed due a unicode error on {:?}",
                name
            )),
        }
    }

    #[test]
    fn test_make_paths_but_xdg_cache_home_and_home_are_not_set() -> Result<(), String> {
        let debugger = build_debugger();
        let mut xte = Globals::default();

        let result = with_env(
            HashMap::from([
                ("HOME", None),
                // XDG_CONFIG_HOME is required to be present for this fallback
                ("XDG_CONFIG_HOME", Some(String::from("/home/user"))),
                // XDG_DATA_HOME is required to be present for this fallback
                ("XDG_DATA_HOME", None),
                ("XDG_CACHE_HOME", None),
            ]),
            &mut || make_paths(&debugger, &mut xte),
        );

        match result {
            Err(VarError::NotPresent) => Ok(()),
            Ok(()) => Err(String::from(
                "Expected make_paths to fail with variable not present",
            )),
            Err(VarError::NotUnicode(name)) => Err(format!(
                "Expected make_paths to fail with variable not present, instead if failed due a unicode error on {:?}",
                name
            )),
        }
    }
}
