use std::{
    env,
    ffi::{OsStr, OsString},
    path::Path,
    process::exit,
};

use xdg_terminal_exec::{
    debug::{Debugger, build_debugger},
    emplace_to_csv_list, env_var, os_str_concat, os_str_remove_trailing_slash, os_str_split,
    push_to_csv_list,
};

#[derive(Debug)]
enum Error {
    #[allow(dead_code)]
    UndefinedEnvVarError(OsString),
}

#[derive(Default)]
struct Globals {
    lowercase_xdg_current_desktop: OsString,
    //compat mode
    execarg_compat: OsString,
    // flag reused in directive encounter
    execarg_compat_configured: OsString,
    exec_usep: OsString,
    expanded_str: OsString,
    entry_path: OsString,
    entry_action: OsString,
    // this will receive proper value later
    applications_dirs: OsString,
    // this will be filled with values from /execarg_default:*:* directives
    execarg_defaults: OsString,
    // the following values are updated by reset_keys on the original implementation
    // Init vars used in entry checks
    is_terminal: OsString,
    // exec_usep: OsString, // duplicated
    execarg: OsString,
    execarg_defined: OsString,
    appidarg: OsString,
    titlearg: OsString,
    dirarg: OsString,
    holdarg: OsString,
    // the following values are updated by make_paths on the original implementation
    // Global constants and lists for later use and iterator
    configs: OsString,
    // applications_dirs: OsString, // duplicated
    cache_file: OsString,
    xdg_cache_home: OsString,
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
    xte.lowercase_xdg_current_desktop = env_var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    xte.execarg_compat = env_var("XTE_EXECARG_COMPAT").unwrap_or(OsString::from("true"));
    xte.execarg_compat_configured = env_var("XTE_EXECARG_COMPAT").unwrap_or_default();
    reset_keys(&mut xte);

    let debugger = build_debugger();
    make_paths(&debugger, &mut xte).unwrap();
}

fn reset_keys(xte: &mut Globals) {
    xte.is_terminal = OsString::new();
    xte.exec_usep = OsString::new();
    xte.execarg = OsString::from("-e");
    xte.execarg_defined = OsString::from("false");
    xte.appidarg = OsString::new();
    xte.titlearg = OsString::new();
    xte.dirarg = OsString::new();
    xte.holdarg = OsString::new();
}

fn make_paths(debugger: &Box<dyn Debugger>, xte: &mut Globals) -> Result<(), Error> {
    // Populate list of config files to read, in descending order of preference
    // Equivalent to `${XDG_CONFIG_HOME:-"${HOME}/.config"}${IFS}${XDG_CONFIG_DIRS:-/etc/xdg}`
    let config_dirs = os_str_concat(&[
        match env_var("XDG_CONFIG_HOME") {
            Some(value) => value,
            None => match env_var("HOME") {
                Some(value) => os_str_concat(&[value.as_os_str(), &OsStr::new("/.config")]),
                None => return Err(Error::UndefinedEnvVarError(OsString::from("HOME"))),
            },
        },
        OsString::from(":"),
        env_var("XDG_CONFIG_DIRS").unwrap_or(OsString::from("/etc/xdg")),
    ]);
    for xte_dir in os_str_split(&config_dirs, ':').unwrap() {
        // Normalise base path and append the data subdirectory with a trailing '/'
        let xte_dir = os_str_remove_trailing_slash(&xte_dir);

        if !xte.lowercase_xdg_current_desktop.is_empty() {
            for xte_desktop in os_str_split(&xte.lowercase_xdg_current_desktop, ':').unwrap() {
                xte.configs = push_to_csv_list(
                    &xte.configs,
                    // Equivalent to `format!("{xte_dir}/{xte_desktop}-xdg-terminals.list")`
                    os_str_concat(&[
                        xte_dir,
                        OsStr::new("/"),
                        xte_desktop,
                        OsStr::new("-xdg-terminals.list"),
                    ]),
                );
            }
        }

        xte.configs = push_to_csv_list(
            &xte.configs,
            // Equivalent to `format!("{xte_dir}/xdg-terminals.list")`
            os_str_concat(&[xte_dir, OsStr::new("/xdg-terminals.list")]),
        );
    }

    // append xdg-terminal-exec dirs in XDG_DATA_DIRS to config hierarchy for distro/upstream level defaults
    // Equivalent to `${XDG_DATA_DIRS:-/usr/local/share:/usr/share}`
    let xdg_data_dirs = match env_var("XDG_DATA_DIRS") {
        Some(value) => value,
        None => OsString::from("/usr/local/share:/usr/share"),
    };
    for xte_dir in os_str_split(&xdg_data_dirs, ':').unwrap() {
        // Normalise base path and append the data subdirectory with a trailing '/'
        let xte_dir = os_str_remove_trailing_slash(&xte_dir);

        if !xte.lowercase_xdg_current_desktop.is_empty() {
            for xte_desktop in os_str_split(&xte.lowercase_xdg_current_desktop, ':').unwrap() {
                xte.configs = push_to_csv_list(
                    &xte.configs,
                    // Equivalent to `format!("{xte_dir}/xdg-terminal-exec/{xte_desktop}-xdg-terminals.list")`
                    os_str_concat(&[
                        xte_dir,
                        OsStr::new("/xdg-terminal-exec/"),
                        xte_desktop,
                        OsStr::new("-xdg-terminals.list"),
                    ]),
                );
            }
        }

        xte.configs = push_to_csv_list(
            &xte.configs,
            // Equivalent to `format!("{xte_dir}/xdg-terminal-exec/xdg-terminals.list")`
            os_str_concat(&[xte_dir, OsStr::new("/xdg-terminal-exec/xdg-terminals.list")]),
        );
    }

    // Populate list of directories to search for entries in, in ascending order of preference
    // Equivalent to `${XDG_DATA_HOME:-${HOME}/.local/share}${IFS}${XDG_DATA_DIRS:-/usr/local/share:/usr/share}`
    let data_home_dirs = os_str_concat(&[
        match env_var("XDG_DATA_HOME") {
            Some(value) => value,
            None => match env_var("HOME") {
                Some(value) => os_str_concat(&[value.as_os_str(), &OsStr::new("/.local/share")]),
                None => return Err(Error::UndefinedEnvVarError(OsString::from("HOME"))),
            },
        },
        OsString::from(":"),
        env_var("XDG_DATA_DIRS").unwrap_or(OsString::from("/usr/local/share:/usr/share")),
    ]);
    for xte_dir in os_str_split(&data_home_dirs, ':').unwrap() {
        // Normalise base path and append the data subdirectory with a trailing '/'
        let xte_dir = os_str_remove_trailing_slash(&xte_dir);

        xte.applications_dirs = emplace_to_csv_list(
            &xte.applications_dirs,
            // Equivalent to `format!("{xte_dir}/applications/")`,
            os_str_concat(&[xte_dir, OsStr::new("/applications/")]),
        );
    }

    xte.xdg_cache_home = match env_var("XDG_CACHE_HOME") {
        Some(value) => value,
        None => match env_var("HOME") {
            // Equivalent to `format!("{value}/.cache")`
            Some(value) => os_str_concat(&[value.as_os_str(), OsStr::new("/.cache")]),
            None => return Err(Error::UndefinedEnvVarError(OsString::from("HOME"))),
        },
    };
    // Equivalent to `format!("{}/xdg-terminal-exec", xte.xdg_cache_home)`
    xte.cache_file = os_str_concat(&[
        xte.xdg_cache_home.as_os_str(),
        OsStr::new("/xdg-terminal-exec"),
    ]);

    debugger.print_slice(&[
        &"paths:",
        &format!("  XTE__CONFIGS={}", xte.configs.display()),
        &format!(
            "  XTE__APPLICATIONS_DIRS={}",
            xte.applications_dirs.display()
        ),
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
        env,
        ffi::OsStr,
        sync::{LazyLock, Mutex},
    };

    use xdg_terminal_exec::debug::build_debugger;

    use super::*;

    static ENV_LOCK: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

    #[test]
    fn test_reset_keys() {
        let mut xte = Globals::default();

        xte.is_terminal = OsString::from("Not default value for is_terminal");
        xte.exec_usep = OsString::from("Not default value for exec_usep");
        xte.execarg = OsString::from("Not default value for execarg");
        xte.execarg_defined = OsString::from("Not default value for execarg_defined");
        xte.appidarg = OsString::from("Not default value for appidarg");
        xte.titlearg = OsString::from("Not default value for titlearg");
        xte.dirarg = OsString::from("Not default value for dirarg");
        xte.holdarg = OsString::from("Not default value for holdarg");

        reset_keys(&mut xte);

        assert!(xte.is_terminal.is_empty());
        assert!(xte.exec_usep.is_empty());
        assert_eq!(xte.execarg, OsString::from("-e"));
        assert_eq!(xte.execarg_defined, OsString::from("false"));
        assert!(xte.appidarg.is_empty());
        assert!(xte.titlearg.is_empty());
        assert!(xte.dirarg.is_empty());
        assert!(xte.holdarg.is_empty());
    }

    fn overwrite_env<K, V>(env: HashMap<K, Option<V>>)
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
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

    fn with_env<K, V, F, R>(env: HashMap<K, Option<V>>, function: &mut F) -> R
    where
        K: AsRef<OsStr> + Eq + std::hash::Hash + Copy,
        V: AsRef<OsStr>,
        F: FnMut() -> R,
    {
        let _mutex_guard = ENV_LOCK.lock().unwrap();
        let mut original_env: HashMap<K, Option<OsString>> = HashMap::new();

        env.keys().for_each(|name: &K| {
            original_env.insert(name.clone(), env_var(name));
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
                ("HOME", Some("/home/user")),
                ("XDG_CONFIG_HOME", Some("/home/user/.custom_config")),
                ("XDG_CONFIG_DIRS", Some("/etc/custom_config")),
                ("XDG_DATA_DIRS", Some("/etc/custom_data")),
            ]),
            &mut || make_paths(&debugger, &mut xte),
        )
        .unwrap();

        assert_eq!(
            xte.configs,
            OsString::from(
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

        xte.lowercase_xdg_current_desktop = OsString::from("distro:DE");

        with_env(
            HashMap::from([
                ("HOME", Some("/home/user")),
                ("XDG_CONFIG_HOME", Some("/home/user/.custom_config")),
                ("XDG_CONFIG_DIRS", Some("/etc/custom_config")),
                ("XDG_DATA_DIRS", Some("/etc/custom_data")),
            ]),
            &mut || make_paths(&debugger, &mut xte),
        )
        .unwrap();

        assert_eq!(
            xte.configs,
            OsString::from(
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
                ("HOME", Some("/home/user")),
                (
                    "XDG_DATA_HOME",
                    Some("/home/user/.custom_data1:/home/user/.custom_data2"),
                ),
                ("XDG_DATA_DIRS", Some("/etc/custom_data1:/etc/custom_data2")),
            ]),
            &mut || make_paths(&debugger, &mut xte),
        )
        .unwrap();

        assert_eq!(
            xte.applications_dirs,
            OsString::from(
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
                ("HOME", Some("/home/user")),
                ("XDG_CACHE_HOME", Some("/tmp/cache")),
            ]),
            &mut || make_paths(&debugger, &mut xte),
        )
        .unwrap();

        assert_eq!(xte.xdg_cache_home, OsString::from("/tmp/cache"));
        assert_eq!(
            xte.cache_file,
            OsString::from("/tmp/cache/xdg-terminal-exec")
        );
    }

    #[test]
    fn test_make_paths_updates_the_cache_values_when_no_xdg_cache_home_set() {
        let debugger = build_debugger();
        let mut xte = Globals::default();

        with_env(
            HashMap::from([("HOME", Some("/home/user")), ("XDG_CACHE_HOME", None)]),
            &mut || make_paths(&debugger, &mut xte),
        )
        .unwrap();

        assert_eq!(xte.xdg_cache_home, OsString::from("/home/user/.cache"));
        assert_eq!(
            xte.cache_file,
            OsString::from("/home/user/.cache/xdg-terminal-exec")
        );
    }

    #[test]
    fn test_make_paths_but_xdg_config_home_and_home_are_not_set() {
        let debugger = build_debugger();
        let mut xte = Globals::default();

        let result = with_env(
            HashMap::<&str, Option<&str>>::from([("HOME", None), ("XDG_CONFIG_HOME", None)]),
            &mut || make_paths(&debugger, &mut xte),
        );

        match result {
            Err(Error::UndefinedEnvVarError(name)) => assert_eq!(name, OsString::from("HOME")),
            Ok(()) => assert!(
                false,
                "Expected make_paths to fail with variable not present"
            ),
        }
    }

    #[test]
    fn test_make_paths_but_xdg_data_home_and_home_are_not_set() {
        let debugger = build_debugger();
        let mut xte = Globals::default();

        let result = with_env(
            HashMap::from([
                ("HOME", None),
                // XDG_CONFIG_HOME is required to be present for this fallback
                ("XDG_CONFIG_HOME", Some("/home/user")),
                ("XDG_DATA_HOME", None),
            ]),
            &mut || make_paths(&debugger, &mut xte),
        );

        match result {
            Err(Error::UndefinedEnvVarError(name)) => assert_eq!(name, OsString::from("HOME")),
            Ok(()) => assert!(
                false,
                "Expected make_paths to fail with variable not present"
            ),
        }
    }

    #[test]
    fn test_make_paths_but_xdg_cache_home_and_home_are_not_set() {
        let debugger = build_debugger();
        let mut xte = Globals::default();

        let result = with_env(
            HashMap::from([
                ("HOME", None),
                // XDG_CONFIG_HOME is required to be present for this fallback
                ("XDG_CONFIG_HOME", Some("/home/user")),
                // XDG_DATA_HOME is required to be present for this fallback
                ("XDG_DATA_HOME", None),
                ("XDG_CACHE_HOME", None),
            ]),
            &mut || make_paths(&debugger, &mut xte),
        );

        match result {
            Err(Error::UndefinedEnvVarError(name)) => assert_eq!(name, OsString::from("HOME")),
            Ok(()) => assert!(
                false,
                "Expected make_paths to fail with variable not present"
            ),
        }
    }
}
