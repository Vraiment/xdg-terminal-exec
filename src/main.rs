use std::{
    env,
    ffi::{OsStr, OsString},
    fmt::Display,
    io,
    ops::Range,
    os::unix::ffi::OsStrExt,
    path::Path,
    process::exit,
};

use xdg_terminal_exec::{
    LF,
    cache::read_cache,
    check_bool,
    debug::{Debugger, build_debugger},
    emplace_to_csv_list, env_var, os_str_concat, os_str_read_lines, os_str_remove_trailing_slash,
    os_str_split, os_str_starts_with, os_str_strip_suffix, os_str_trim, push_to_csv_list,
};

const ASCII_DIGITS: Range<u8> = Range { start: 48, end: 58 }; // 0-9
const ASCII_UPPERCASE_LETTERS: Range<u8> = Range { start: 65, end: 91 }; // A-Z
const ASCII_LOWERCASE_LETTERS: Range<u8> = Range {
    start: 97,
    end: 123,
}; // a-z
const ASCII_UNDERSCORE: u8 = 95;
const ASCII_DASH: u8 = 45;
const ASCII_COLON: u8 = 58;
const ASCII_PLUS_SIGN: u8 = 43;
const ASCII_MINUS_SIGN: u8 = 45;

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
    // The following are used related to cache
    cache_used: OsString,
    entry_ids: OsString,
    fallback_entry_ids: OsString,
    excluded_entry_ids: OsString,
    included_entry_ids: OsString,
    cache_enabled: OsString,
    cache_configured: OsString,
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

    xte.cache_enabled = env_var("XTE_CACHE_ENABLED").unwrap_or(OsString::from("true"));
    xte.cache_configured = env_var("XTE_CACHE_ENABLED").unwrap_or_default();

    let cache = if check_bool(&xte.cache_enabled) {
        read_cache(
            &debugger,
            &xte.cache_file,
            &xte.configs,
            &xte.applications_dirs,
        )
        .unwrap()
    } else {
        None
    };

    if cache.is_some() {
        xte.cache_used = OsString::from("true");
    } else {
        // continue with globals
        xte.cache_used = OsString::from("false");

        // All desktop entry ids in descending order of preference from *xdg-terminals.list configs,
        // with duplicates removed
        xte.entry_ids = OsString::new();
        // All desktop entry ids found in data dirs in descending order of preference,
        // with duplicates (including those in $XTE__ENTRY_IDS) removed
        xte.fallback_entry_ids = OsString::new();

        //# Entry IDs excluded from fallback by '-entry.desktop' directives
        xte.excluded_entry_ids = OsString::new();
        // Entry IDS included (exclusion prevented) by '+entry.desktop' directives
        xte.included_entry_ids = OsString::new();

        // Modifies $XTE__ENTRY_IDS
        read_config_paths(&debugger, &mut xte).unwrap();
        // Modifies $XTE__ENTRY_IDS and sets global aliases
        find_entry_paths(&debugger, &mut xte).unwrap();

        if debugger.is_enabled() {
            assert!(LF.len() == 1, "{LF} should have a single character");
            let lf_as_char = LF.chars().next().unwrap();

            debugger.print_line(&">     final entry ID list:");
            for line in os_str_split(&xte.entry_ids, lf_as_char).unwrap() {
                debugger.print_line(&line.display());
            }
            debugger.print_line(&"^     end of final entry ID list");

            debugger.print_line(&">     final fallback entry ID list:");
            for line in os_str_split(&xte.fallback_entry_ids, lf_as_char).unwrap() {
                debugger.print_line(&line.display());
            }
            debugger.print_line(&"^     end of final fallback entry ID list");
        }

        // walk ID lists and find first applicable
        if !find_entry(&debugger, &mut xte).unwrap() {
            exit(1)
        }
    }
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

fn read_config_paths(debugger: &Box<dyn Debugger>, xte: &mut Globals) -> Result<(), io::Error> {
    for config_path in os_str_split(&xte.configs, ':').unwrap() {
        let config_file = Path::new(config_path);

        debugger.print_line(&format!("reading config '{}'", config_path.display()));

        // Nonexistant file is not an error
        if !config_file.exists() {
            continue;
        }

        for line in os_str_read_lines(config_file)?.map_while(Result::ok) {
            // Originally `read` would trim leading/trailing whitespace from the line
            match os_str_trim(&line) {
                // Catch directives first

                // Cache control
                line if os_str_starts_with(&line, "/enable_cache") => {
                    debugger.print_line(&format!(
                        "found '{}' directive{}",
                        line.display(),
                        if !xte.cache_configured.is_empty() {
                            " (ignored)"
                        } else {
                            ""
                        }
                    ));

                    if !xte.cache_configured.is_empty() {
                        continue;
                    }

                    xte.cache_enabled = OsString::from("true");
                    xte.cache_configured = OsString::from("1");
                }
                line if os_str_starts_with(&line, "/disable_cache") => {
                    debugger.print_line(&format!(
                        "found '{}' directive{}",
                        line.display(),
                        if !xte.cache_configured.is_empty() {
                            " (ignored)"
                        } else {
                            ""
                        }
                    ));

                    if !xte.cache_configured.is_empty() {
                        continue;
                    }

                    xte.cache_enabled = OsString::from("false");
                    xte.cache_configured = OsString::from("1");
                }

                // Compat mode
                line if os_str_starts_with(&line, "/execarg_compat") => {
                    debugger.print_line(&format!(
                        "found '{}' directive{}",
                        line.display(),
                        if !xte.execarg_compat_configured.is_empty() {
                            " (ignored)"
                        } else {
                            ""
                        }
                    ));

                    if !xte.execarg_compat_configured.is_empty() {
                        continue;
                    }

                    xte.execarg_compat = OsString::from("true");
                    xte.execarg_compat_configured = OsString::from("1");
                }
                line if os_str_starts_with(&line, "/execarg_strict") => {
                    debugger.print_line(&format!(
                        "found '{}' directive{}",
                        line.display(),
                        if !xte.execarg_compat_configured.is_empty() {
                            " (ignored)"
                        } else {
                            ""
                        }
                    ));

                    if !xte.execarg_compat_configured.is_empty() {
                        continue;
                    }

                    xte.execarg_compat = OsString::from("false");
                    xte.execarg_compat_configured = OsString::from("1");
                }

                // default TerminalArgExec overrides
                line if is_default_terminal_arg_exec_overrides(&line) => {
                    if !check_bool(&xte.execarg_compat) {
                        debugger.print_line(&format!(
                            "ignored directive '{}' (strict mode)",
                            line.display()
                        ));
                        continue;
                    }

                    let (entry_id, execarg_default) =
                        split_default_terminal_arg_exec_overrides(&line);
                    if validate_entry_id(debugger, entry_id) {
                        debugger.print_line(&format!(
                            "added TerminalArgExec default '{}' for '{}'",
                            execarg_default.display(),
                            entry_id.display()
                        ));

                        // do not bother with deduplication, first entry ID will win
                        let mut entry = Vec::<&OsStr>::new();
                        if !xte.execarg_defaults.is_empty() {
                            entry.push(&xte.execarg_defaults);
                            entry.push(OsStr::new(LF));
                        }

                        entry.push(entry_id);
                        entry.push(OsStr::new(":"));
                        entry.push(execarg_default);

                        xte.execarg_defaults = os_str_concat(&entry);
                    }
                }

                line if is_potential_entry(&line) => {
                    let line_without_exclusion: &OsStr;
                    let exclusion: &OsStr;
                    match line.as_bytes() {
                        [ASCII_PLUS_SIGN, ..] | [ASCII_MINUS_SIGN, ..] => {
                            let line_bytes = (&line).as_bytes();
                            // save and cut exclusion marker
                            let _line = OsStr::from_bytes(&line_bytes[1..]);
                            exclusion = os_str_strip_suffix(&line, _line).unwrap();
                            line_without_exclusion = _line;
                        }
                        _ => {
                            exclusion = OsStr::new("");
                            line_without_exclusion = &line;
                        }
                    }

                    // consider only the first ':' as a delimiter
                    let (entry_id, action_id) =
                        split_entry_id_and_action_id(&line_without_exclusion);
                    if validate_entry_id(&debugger, &entry_id)
                        && validate_action_id(&debugger, &action_id)
                    {
                        match exclusion {
                            _ if exclusion.is_empty() => {
                                xte.entry_ids = if !xte.entry_ids.is_empty() {
                                    os_str_concat(&[
                                        xte.entry_ids.as_os_str(),
                                        OsStr::new(LF),
                                        line,
                                    ])
                                } else {
                                    line.to_os_string()
                                };
                                debugger.print_line(&format!(
                                    "added entry ID with action ID '{}'",
                                    line.display()
                                ));
                            }
                            _ if exclusion == OsStr::from_bytes(&[ASCII_PLUS_SIGN]) => {
                                if list_contains(&xte.excluded_entry_ids, entry_id, None) {
                                    debugger.print_line(&format!(
                                        "entry '{}' was already excluded from fallback",
                                        entry_id.display()
                                    ));
                                } else if list_contains(&xte.included_entry_ids, entry_id, None) {
                                    debugger.print_line(&format!(
                                        "entry '{}' fallback exclusion was already prevented",
                                        entry_id.display()
                                    ));
                                } else {
                                    debugger.print_line(&format!(
                                        "preventing fallback exclusion for entry '{}'",
                                        entry_id.display()
                                    ));
                                    xte.included_entry_ids = if !xte.included_entry_ids.is_empty() {
                                        os_str_concat(&[
                                            xte.included_entry_ids.as_os_str(),
                                            OsStr::new(LF),
                                        ])
                                    } else {
                                        entry_id.to_os_string()
                                    };
                                }
                            }
                            _ if exclusion == OsStr::from_bytes(&[ASCII_MINUS_SIGN]) => {
                                if list_contains(&xte.included_entry_ids, entry_id, None) {
                                    debugger.print_line(&format!(
                                        "entry '{}' fallback exclusion was already prevented",
                                        entry_id.display()
                                    ));
                                } else if list_contains(&xte.excluded_entry_ids, entry_id, None) {
                                    debugger.print_line(&format!(
                                        "entry '{}' was already excluded from fallback",
                                        entry_id.display()
                                    ));
                                } else {
                                    debugger.print_line(&format!(
                                        "excluding entry '{}' from fallback",
                                        entry_id.display()
                                    ));
                                    xte.excluded_entry_ids = if !xte.excluded_entry_ids.is_empty() {
                                        os_str_concat(&[
                                            xte.excluded_entry_ids.as_os_str(),
                                            OsStr::new(LF),
                                        ])
                                    } else {
                                        entry_id.to_os_string()
                                    };
                                }
                            }
                            _ => panic!("This branch should never happen"),
                        }
                    }
                }

                _ => {} // By default empty lines and comments get ignored
            }
        }
    }

    Ok(())
}

fn find_entry_paths(debugger: &Box<dyn Debugger>, xte: &mut Globals) -> Result<(), ()> {
    // Return type TBD
    todo!()
}

fn find_entry(debugger: &Box<dyn Debugger>, xte: &mut Globals) -> Result<bool, ()> {
    // Return type TBD
    todo!()
}

fn validate_entry_id(debugger: &Box<dyn Debugger>, entry: &OsStr) -> bool {
    match entry {
        // invalid characters or degrees of emptiness
        entry if entry_has_invalid_character(entry) || entry.is_empty() || entry == ".desktop" => {
            // Equivalent to `format!("string not valid as Entry ID: '{entry}'")`
            debugger.print_line(
                &os_str_concat(&[
                    OsStr::new("string not valid as Entry ID: '"),
                    entry,
                    OsStr::new("'"),
                ])
                .display(),
            );
            false
        }
        // all that left with .desktop
        entry if entry_ends_with_desktop_extension(entry) => true,
        // and without
        entry => {
            debugger.print_line(
                // Equivalent to `format!("string not valid as Entry ID: '{entry}'")`
                &os_str_concat(&[
                    OsStr::new("string not valid as Entry ID: '"),
                    entry,
                    OsStr::new("'"),
                ])
                .display(),
            );
            false
        }
    }
}

fn entry_has_invalid_character(entry: &OsStr) -> bool {
    const ASCII_DOT: u8 = 46;

    entry.as_bytes().iter().any(|byte| {
        !ASCII_DIGITS.contains(byte)
            && !ASCII_UPPERCASE_LETTERS.contains(byte)
            && !ASCII_LOWERCASE_LETTERS.contains(byte)
            && ASCII_UNDERSCORE != *byte
            && ASCII_DOT != *byte
            && ASCII_DASH != *byte
    })
}

fn entry_ends_with_desktop_extension(entry: &OsStr) -> bool {
    const DESKTOP_EXTENSION: &'static str = ".desktop";

    if entry.len() < DESKTOP_EXTENSION.len() {
        return false;
    }

    // The following line looks a little bit esoteric but what is doing is
    // creating a slice with the bytes where the extension would be located
    // that way is easy to compare as a string with the actual expected
    // extension later
    let entry_suffix = &entry.as_bytes()[entry.len() - DESKTOP_EXTENSION.len()..];

    OsStr::from_bytes(entry_suffix) == DESKTOP_EXTENSION
}

fn is_default_terminal_arg_exec_overrides(entry: &OsStr) -> bool {
    const PREFIX: &'static str = "/execarg_default:";
    if !os_str_starts_with(entry, PREFIX) {
        return false;
    }

    entry.as_bytes()[PREFIX.len()..] // substring after `PREFIX`
        .iter()
        .any(|byte| *byte == ASCII_COLON)
}

fn split_default_terminal_arg_exec_overrides(entry: &OsStr) -> (&OsStr, &OsStr) {
    let entry_bytes = entry.as_bytes();

    let mut first_colon: Option<usize> = None;
    let mut second_colon: Option<usize> = None;
    for (n, byte) in entry_bytes.iter().enumerate() {
        if *byte == ASCII_COLON {
            if first_colon.is_none() {
                first_colon = Some(n);
            } else {
                second_colon = Some(n);
                break;
            }
        }
    }

    let first_colon = first_colon.expect("There should be at least one colon on the entry");
    let second_colon = second_colon.expect("There should be at least two colons on the entry");
    let entry_id: &OsStr = OsStr::from_bytes(&entry_bytes[first_colon + 1..second_colon]);
    let execarg_default = OsStr::from_bytes(&entry_bytes[second_colon + 1..]);

    (entry_id, execarg_default)
}

// Should match `/bin/sh`'s regex: `[a-zA-Z0-9_]* | [+-][a-zA-Z0-9_]*`
fn is_potential_entry(entry: &OsStr) -> bool {
    let entry_bytes = entry.as_bytes();

    if entry_bytes.is_empty() {
        return false;
    }

    let first_byte = if entry_bytes[0] == ASCII_PLUS_SIGN || entry_bytes[0] == ASCII_MINUS_SIGN {
        // Skip first byte if is `+` or `-` and there's more characters
        if entry_bytes.len() > 1 {
            entry_bytes[1]
        } else {
            return false;
        }
    } else {
        entry_bytes[0]
    };

    ASCII_LOWERCASE_LETTERS.contains(&first_byte)
        || ASCII_UPPERCASE_LETTERS.contains(&first_byte)
        || ASCII_DIGITS.contains(&first_byte)
        || ASCII_UNDERSCORE == first_byte
}

fn validate_action_id(debugger: &Box<dyn Debugger>, action: &OsStr) -> bool {
    match action {
        // empty is ok
        _ if action.is_empty() => true,
        // invalid characters
        _ if !action.as_bytes().iter().all(|byte| {
            ASCII_DIGITS.contains(byte)
                || ASCII_UPPERCASE_LETTERS.contains(byte)
                || ASCII_LOWERCASE_LETTERS.contains(byte)
                || ASCII_DASH == *byte
        }) =>
        {
            debugger.print_line(
                // Equivalent to `format!("string not valid as Action ID: '{action}'")`
                &os_str_concat(&[
                    OsStr::new("string not valid as Action ID: '"),
                    action,
                    OsStr::new("'"),
                ])
                .display(),
            );

            false
        }
        // all that left
        _ => true,
    }
}

fn split_entry_id_and_action_id(entry: &OsStr) -> (&OsStr, &OsStr) {
    let entry_bytes = entry.as_bytes();

    for (n, byte) in entry_bytes.iter().enumerate() {
        if *byte == ASCII_COLON {
            let entry_id = OsStr::from_bytes(&entry_bytes[..n]);
            let action_id = OsStr::from_bytes(&entry_bytes[n + 1..]);

            return (entry_id, action_id);
        }
    }

    (entry, OsStr::new(""))
}

fn list_contains(list: &OsStr, entry: &OsStr, delimiter: Option<&OsStr>) -> bool {
    // The original implementation would behave like this
    match (list.is_empty(), entry.is_empty()) {
        (true, true) => return true,
        (true, false) => return false,
        (false, true) => return false,
        _ => {} // "normal" implementation if both are false
    };

    if entry.len() > list.len() {
        return false;
    }

    let delimiter = delimiter
        .filter(|delimiter| !delimiter.is_empty())
        .unwrap_or(OsStr::new(LF));

    let list = os_str_concat(&[delimiter, list, delimiter]);
    let entry = os_str_concat(&[delimiter, entry, delimiter]);

    let list_bytes = list.as_bytes();
    let entry_bytes = entry.as_bytes();

    for n in 0..=list.len() - entry.len() {
        let mut found_different_byte = false;
        for m in 0..entry.len() {
            if list_bytes[n + m] != entry_bytes[m] {
                found_different_byte = true;
                break;
            }
        }

        if !found_different_byte {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::fs;

    use xdg_terminal_exec::debug::build_debugger;
    use xdg_terminal_exec::testing::{TempFile, with_env};

    use super::*;

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

    #[test]
    fn test_read_config_paths_with_enable_cache_directive() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), "/enable_cache").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from(""); // Needs to be unset
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(xte.cache_enabled, OsString::from("true"));
        assert_eq!(xte.cache_configured, OsString::from("1"));
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
    }

    #[test]
    fn test_read_config_paths_with_disable_cache_directive() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), "/disable_cache").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from(""); // Needs to be unset
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(xte.cache_enabled, OsString::from("false"));
        assert_eq!(xte.cache_configured, OsString::from("1"));
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
    }

    #[test]
    fn test_read_config_paths_with_disable_cache_directive_and_enable_cache_directive() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(
            &temp_file.path(),
            "/disable_cache\n\
            /enable_cache",
        )
        .unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from(""); // Needs to be unset
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(xte.cache_enabled, OsString::from("false"));
        assert_eq!(xte.cache_configured, OsString::from("1"));
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
    }

    #[test]
    fn test_read_config_paths_with_enable_cache_directive_with_whitespaces() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), "     /enable_cache\t\t\t\t\t\t").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from(""); // Needs to be unset
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(xte.cache_enabled, OsString::from("true"));
        assert_eq!(xte.cache_configured, OsString::from("1"));
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
    }

    #[test]
    fn test_read_config_paths_with_execarg_compat_directive() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), "/execarg_compat").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured = OsString::from(""); // Needs to be unset
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(xte.execarg_compat, OsString::from("true"));
        assert_eq!(xte.execarg_compat_configured, OsString::from("1"));
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
    }

    #[test]
    fn test_read_config_paths_with_execarg_strict_directive() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), "/execarg_strict").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured = OsString::from(""); // Needs to be unset
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(xte.execarg_compat, OsString::from("false"));
        assert_eq!(xte.execarg_compat_configured, OsString::from("1"));
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
    }

    #[test]
    fn test_read_config_paths_with_execarg_strict_directive_and_execarg_compat_directive() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(
            &temp_file.path(),
            "/execarg_strict\n\
            /execarg_compat",
        )
        .unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured = OsString::from(""); // Needs to be unset
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(xte.execarg_compat, OsString::from("false"));
        assert_eq!(xte.execarg_compat_configured, OsString::from("1"));
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
    }

    #[test]
    fn test_read_config_paths_with_execarg_compat_directive_with_whitespaces() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), "     /execarg_compat\t\t\t\t\t\t").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured = OsString::from(""); // Needs to be unset
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(xte.execarg_compat, OsString::from("true"));
        assert_eq!(xte.execarg_compat_configured, OsString::from("1"));
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
    }

    #[test]
    fn test_read_config_paths_with_empty_file_does_not_modify_any_value() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
    }

    #[test]
    fn test_validate_entry_id_with_valid_entry() {
        assert!(validate_entry_id(
            &build_debugger(),
            OsStr::new("some-entry.desktop")
        ));
    }

    #[test]
    fn test_validate_entry_id_with_invalid_entry_that_has_an_invalid_character_halfway() {
        assert!(!validate_entry_id(
            &build_debugger(),
            OsStr::new("some-#entry.desktop")
        ));
    }

    #[test]
    fn test_validate_entry_id_with_invalid_entry_that_ends_with_an_invalid_character() {
        assert!(!validate_entry_id(
            &build_debugger(),
            OsStr::new("some-entry.desktop#")
        ));
    }

    #[test]
    fn test_validate_entry_id_with_invalid_entry_that_starts_with_an_invalid_character() {
        assert!(!validate_entry_id(
            &build_debugger(),
            OsStr::new("#some-entry.desktop")
        ));
    }

    #[test]
    fn test_validate_entry_id_with_invalid_entry_that_is_empty() {
        assert!(!validate_entry_id(&build_debugger(), OsStr::new("")));
    }

    #[test]
    fn test_validate_entry_id_with_invalid_entry_that_is_only_desktop_extension() {
        assert!(!validate_entry_id(
            &build_debugger(),
            OsStr::new(".desktop")
        ));
    }

    #[test]
    fn test_validate_entry_id_with_invalid_entry_that_lacks_desktop_extension() {
        assert!(!validate_entry_id(
            &build_debugger(),
            OsStr::new("some-entry")
        ));
    }

    #[test]
    fn test_entry_has_invalid_character_with_invalid_characters() {
        // Test for the characters between ranges
        const INVALID_CHARACTERS: &[char] = &[
            '/', // character before '0'
            ':', // character after '9'
            '@', // character before 'A'
            '[', // character after 'Z'
            '`', // character before 'a'
            '{', // character after 'z'
        ];

        for character in INVALID_CHARACTERS {
            assert!(
                entry_has_invalid_character(OsStr::new(&String::from(*character))),
                "Character '{character}' is an invalid character but was considered valid"
            );
        }
    }

    #[test]
    fn test_entry_has_invalid_character_with_digits() {
        for digit in '0'..='9' {
            assert!(
                !entry_has_invalid_character(OsStr::new(&String::from(String::from(digit)))),
                "Character '{digit}' is a valid character but was considered invalid"
            );
        }
    }

    #[test]
    fn test_entry_has_invalid_character_with_uppercase_letters() {
        for letter in 'A'..='Z' {
            assert!(
                !entry_has_invalid_character(OsStr::new(&String::from(letter))),
                "Character '{letter}' is a valid character but was considered invalid"
            )
        }
    }

    #[test]
    fn test_entry_has_invalid_character_with_lowercase_letters() {
        for letter in 'a'..='z' {
            assert!(
                !entry_has_invalid_character(OsStr::new(&String::from(letter))),
                "Character '{letter}' is considered an invalid character but it should be valid"
            )
        }
    }

    #[test]
    fn test_entry_has_invalid_character_with_valid_symbols() {
        for symbol in ['_', '.', '-'] {
            assert!(
                !entry_has_invalid_character(OsStr::new(&String::from(symbol))),
                "Character '{symbol}' is a valid character but was considered invalid"
            );
        }
    }

    #[test]
    fn test_entry_ends_with_desktop_extension_with_entry_that_ends_with_desktop_extesion() {
        assert!(entry_ends_with_desktop_extension(OsStr::new(
            "entry.desktop"
        )));
    }

    #[test]
    fn test_entry_ends_with_desktop_extension_with_entry_that_is_just_desktop_extesion() {
        assert!(entry_ends_with_desktop_extension(OsStr::new(".desktop")));
    }

    #[test]
    fn test_entry_ends_with_desktop_extension_with_entry_that_lacks_desktop_extension() {
        assert!(!entry_ends_with_desktop_extension(OsStr::new("entry")));
    }

    #[test]
    fn test_entry_ends_with_desktop_extension_with_entry_that_ends_with_desktop_extesion_but_lacks_the_dot()
     {
        assert!(!entry_ends_with_desktop_extension(OsStr::new(
            "entrydesktop"
        )));
    }

    #[test]
    fn test_read_config_paths_with_file_that_enables_execarg_compat_and_has_a_valid_override() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(
            &temp_file.path(),
            "/execarg_compat\n\
            /execarg_default:entry.desktop:value",
        )
        .unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured = OsString::from(""); // Needs to be unset
        xte.execarg_defaults = OsString::from(""); // Needs to be unset
        xte.entry_ids = OsString::from("default value for entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(xte.execarg_compat, OsString::from("true"));
        assert_eq!(xte.execarg_compat_configured, OsString::from("1"));
        assert_eq!(xte.execarg_defaults, OsString::from("entry.desktop:value"));
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
    }

    #[test]
    fn test_read_config_paths_with_file_that_enables_execarg_compat_and_has_a_valid_override_with_colons_in_the_value()
     {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(
            &temp_file.path(),
            "/execarg_compat\n\
            /execarg_default:entry.desktop:value:with:colons",
        )
        .unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured = OsString::from(""); // Needs to be unset
        xte.execarg_defaults = OsString::from(""); // Needs to be unset
        xte.entry_ids = OsString::from("default value for entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(xte.execarg_compat, OsString::from("true"));
        assert_eq!(xte.execarg_compat_configured, OsString::from("1"));
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("entry.desktop:value:with:colons")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
    }

    #[test]
    fn test_read_config_paths_with_file_that_enables_execarg_compat_and_has_a_multiple_overrides() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(
            &temp_file.path(),
            "/execarg_compat\n\
            /execarg_default:entry1.desktop:value1\n\
            /execarg_default:entry2.desktop:value2",
        )
        .unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured = OsString::from(""); // Needs to be unset
        xte.execarg_defaults = OsString::from(""); // Needs to be unset
        xte.entry_ids = OsString::from("default value for entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(xte.execarg_compat, OsString::from("true"));
        assert_eq!(xte.execarg_compat_configured, OsString::from("1"));
        assert_eq!(
            xte.execarg_defaults,
            OsString::from(format!("entry1.desktop:value1{LF}entry2.desktop:value2"))
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
    }

    #[test]
    fn test_read_config_paths_with_file_that_disables_execarg_compat_and_has_a_valid_override() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(
            &temp_file.path(),
            "/execarg_strict\n\
            /execarg_default:entry.desktop:value",
        )
        .unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured = OsString::from(""); // Needs to be unset
        xte.execarg_defaults = OsString::from("default value for execarg_defaults"); // Needs to be unset
        xte.entry_ids = OsString::from("default value for entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(xte.execarg_compat, OsString::from("false"));
        assert_eq!(xte.execarg_compat_configured, OsString::from("1"));
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
    }

    #[test]
    fn test_read_config_paths_with_file_that_enables_execarg_compat_and_has_a_valid_override_with_whitespaces()
     {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(
            &temp_file.path(),
            "/execarg_compat\n\
            \t\t\t/execarg_default:entry.desktop:value    ",
        )
        .unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured = OsString::from(""); // Needs to be unset
        xte.execarg_defaults = OsString::from(""); // Needs to be unset
        xte.entry_ids = OsString::from("default value for entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(xte.execarg_compat, OsString::from("true"));
        assert_eq!(xte.execarg_compat_configured, OsString::from("1"));
        assert_eq!(xte.execarg_defaults, OsString::from("entry.desktop:value"));
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
    }

    #[test]
    fn test_read_config_paths_with_an_entry_without_action() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), "entry_id.desktop").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from(""); // Needs to be unset
        xte.included_entry_ids = OsString::from("default value for included_entry_ids");
        xte.excluded_entry_ids = OsString::from("default value for excluded_entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("entry_id.desktop"));
        assert_eq!(
            xte.included_entry_ids,
            OsString::from("default value for included_entry_ids")
        );
        assert_eq!(
            xte.excluded_entry_ids,
            OsString::from("default value for excluded_entry_ids")
        );
    }

    #[test]
    fn test_read_config_paths_with_an_entry_without_action_with_whitespaces() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), " \t \t entry_id.desktop\t \t ").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from(""); // Needs to be unset
        xte.included_entry_ids = OsString::from("default value for included_entry_ids");
        xte.excluded_entry_ids = OsString::from("default value for excluded_entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("entry_id.desktop"));
        assert_eq!(
            xte.included_entry_ids,
            OsString::from("default value for included_entry_ids")
        );
        assert_eq!(
            xte.excluded_entry_ids,
            OsString::from("default value for excluded_entry_ids")
        );
    }

    #[test]
    fn test_read_config_paths_with_an_entry_with_valid_action() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), "entry_id.desktop:action").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from(""); // Needs to be unset
        xte.included_entry_ids = OsString::from("default value for included_entry_ids");
        xte.excluded_entry_ids = OsString::from("default value for excluded_entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("entry_id.desktop:action"));
        assert_eq!(
            xte.included_entry_ids,
            OsString::from("default value for included_entry_ids")
        );
        assert_eq!(
            xte.excluded_entry_ids,
            OsString::from("default value for excluded_entry_ids")
        );
    }

    #[test]
    fn test_read_config_paths_with_an_entry_with_invalid_action() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), "entry_id.desktop:act*ion").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");
        xte.included_entry_ids = OsString::from("default value for included_entry_ids");
        xte.excluded_entry_ids = OsString::from("default value for excluded_entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
        assert_eq!(
            xte.included_entry_ids,
            OsString::from("default value for included_entry_ids")
        );
        assert_eq!(
            xte.excluded_entry_ids,
            OsString::from("default value for excluded_entry_ids")
        );
    }

    #[test]
    fn test_read_config_paths_with_multiple_valid_entries() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(
            &temp_file.path(),
            "entry1.desktop:value1\n\
            entry2.desktop:value2",
        )
        .unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from(""); // Needs to be unset
        xte.included_entry_ids = OsString::from("default value for included_entry_ids");
        xte.excluded_entry_ids = OsString::from("default value for excluded_entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(
            xte.entry_ids,
            OsString::from(format!("entry1.desktop:value1{LF}entry2.desktop:value2"))
        );
        assert_eq!(
            xte.included_entry_ids,
            OsString::from("default value for included_entry_ids")
        );
        assert_eq!(
            xte.excluded_entry_ids,
            OsString::from("default value for excluded_entry_ids")
        );
    }

    #[test]
    fn test_read_config_paths_with_an_included_entry_without_action() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), "+entry_id.desktop").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");
        xte.included_entry_ids = OsString::from(""); // Needs to be unset
        xte.excluded_entry_ids = OsString::from("default value for excluded_entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
        assert_eq!(xte.included_entry_ids, OsString::from("entry_id.desktop"));
        assert_eq!(
            xte.excluded_entry_ids,
            OsString::from("default value for excluded_entry_ids")
        );
    }

    #[test]
    fn test_read_config_paths_with_an_included_entry_with_action() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), "+entry_id.desktop:action").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");
        xte.included_entry_ids = OsString::from(""); // Needs to be unset
        xte.excluded_entry_ids = OsString::from("default value for excluded_entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
        assert_eq!(xte.included_entry_ids, OsString::from("entry_id.desktop"));
        assert_eq!(
            xte.excluded_entry_ids,
            OsString::from("default value for excluded_entry_ids")
        );
    }

    #[test]
    fn test_read_config_paths_with_an_included_entry_without_action_with_whitespaces() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), " \t \t+entry_id.desktop\t \t ").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");
        xte.included_entry_ids = OsString::from(""); // Needs to be unset
        xte.excluded_entry_ids = OsString::from("default value for excluded_entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
        assert_eq!(xte.included_entry_ids, OsString::from("entry_id.desktop"));
        assert_eq!(
            xte.excluded_entry_ids,
            OsString::from("default value for excluded_entry_ids")
        );
    }

    #[test]
    fn test_read_config_paths_with_an_included_entry_when_the_entry_was_already_excluded() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), "+entry_id.desktop").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");
        xte.included_entry_ids = OsString::from("default value for included_entry_ids");
        xte.excluded_entry_ids = OsString::from("entry_id.desktop");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
        assert_eq!(
            xte.included_entry_ids,
            OsString::from("default value for included_entry_ids")
        );
        assert_eq!(xte.excluded_entry_ids, OsString::from("entry_id.desktop"));
    }

    #[test]
    fn test_read_config_paths_with_an_included_entry_when_the_entry_was_already_included() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), "+entry_id.desktop").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");
        xte.included_entry_ids = OsString::from("entry_id.desktop");
        xte.excluded_entry_ids = OsString::from("default value for excluded_entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
        assert_eq!(xte.included_entry_ids, OsString::from("entry_id.desktop"));
        assert_eq!(
            xte.excluded_entry_ids,
            OsString::from("default value for excluded_entry_ids")
        );
    }

    #[test]
    fn test_read_config_paths_with_an_excluded_entry_without_action() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), "-entry_id.desktop").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");
        xte.included_entry_ids = OsString::from("default value for included_entry_ids");
        xte.excluded_entry_ids = OsString::from(""); // Needs to be unset

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
        assert_eq!(
            xte.included_entry_ids,
            OsString::from("default value for included_entry_ids")
        );
        assert_eq!(xte.excluded_entry_ids, OsString::from("entry_id.desktop"));
    }

    #[test]
    fn test_read_config_paths_with_an_excluded_entry_with_action() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), "-entry_id.desktop:action").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");
        xte.included_entry_ids = OsString::from("default value for included_entry_ids");
        xte.excluded_entry_ids = OsString::from(""); // Needs to be unset

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
        assert_eq!(
            xte.included_entry_ids,
            OsString::from("default value for included_entry_ids")
        );
        assert_eq!(xte.excluded_entry_ids, OsString::from("entry_id.desktop"));
    }

    #[test]
    fn test_read_config_paths_with_an_excluded_entry_without_action_with_whitespaces() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), " \t \t-entry_id.desktop\t \t ").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");
        xte.included_entry_ids = OsString::from("default value for included_entry_ids");
        xte.excluded_entry_ids = OsString::from(""); // Needs to be unset

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
        assert_eq!(
            xte.included_entry_ids,
            OsString::from("default value for included_entry_ids")
        );
        assert_eq!(xte.excluded_entry_ids, OsString::from("entry_id.desktop"));
    }

    #[test]
    fn test_read_config_paths_with_an_excluded_entry_when_the_entry_was_already_included() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), "-entry_id.desktop").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");
        xte.included_entry_ids = OsString::from("entry_id.desktop");
        xte.excluded_entry_ids = OsString::from("default value for excluded_entry_ids");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
        assert_eq!(xte.included_entry_ids, OsString::from("entry_id.desktop"));
        assert_eq!(
            xte.excluded_entry_ids,
            OsString::from("default value for excluded_entry_ids")
        );
    }

    #[test]
    fn test_read_config_paths_with_an_excluded_entry_when_the_entry_was_already_excluded() {
        let temp_file = TempFile::new().unwrap();
        let mut xte = Globals::default();

        fs::write(&temp_file.path(), "-entry_id.desktop").unwrap();

        xte.configs = OsString::from(temp_file.path());

        xte.cache_enabled = OsString::from("default value for cache_enabled");
        xte.cache_configured = OsString::from("default value for cache_configured");
        xte.execarg_compat = OsString::from("default value for execarg_compat");
        xte.execarg_compat_configured =
            OsString::from("default value for execarg_compat_configured");
        xte.execarg_defaults = OsString::from("default value for execarg_defaults");
        xte.entry_ids = OsString::from("default value for entry_ids");
        xte.included_entry_ids = OsString::from("default value for included_entry_ids");
        xte.excluded_entry_ids = OsString::from("entry_id.desktop");

        read_config_paths(&build_debugger(), &mut xte).unwrap();

        assert_eq!(
            xte.cache_enabled,
            OsString::from("default value for cache_enabled")
        );
        assert_eq!(
            xte.cache_configured,
            OsString::from("default value for cache_configured")
        );
        assert_eq!(
            xte.execarg_compat,
            OsString::from("default value for execarg_compat")
        );
        assert_eq!(
            xte.execarg_compat_configured,
            OsString::from("default value for execarg_compat_configured")
        );
        assert_eq!(
            xte.execarg_defaults,
            OsString::from("default value for execarg_defaults")
        );
        assert_eq!(xte.entry_ids, OsString::from("default value for entry_ids"));
        assert_eq!(
            xte.included_entry_ids,
            OsString::from("default value for included_entry_ids")
        );
        assert_eq!(xte.excluded_entry_ids, OsString::from("entry_id.desktop"));
    }

    #[test]
    fn test_is_default_terminal_arg_exec_overrides_when_is_valid() {
        assert!(is_default_terminal_arg_exec_overrides(OsStr::new(
            "/execarg_default:value:"
        )));
    }

    #[test]
    fn test_is_default_terminal_arg_exec_overrides_when_is_valid_with_multiple_colons() {
        assert!(is_default_terminal_arg_exec_overrides(OsStr::new(
            "/execarg_default:value:value:"
        )));
    }

    #[test]
    fn test_is_default_terminal_arg_exec_overrides_when_is_valid_with_no_entry_id() {
        assert!(is_default_terminal_arg_exec_overrides(OsStr::new(
            "/execarg_default::value:"
        )));
    }

    #[test]
    fn test_is_default_terminal_arg_exec_overrides_when_is_not_valid_due_missing_the_prefix() {
        assert!(!is_default_terminal_arg_exec_overrides(OsStr::new(
            "execarg_default:value:"
        )));
    }

    #[test]
    fn test_is_default_terminal_arg_exec_overrides_when_is_not_valid_due_missing_another_colon() {
        assert!(!is_default_terminal_arg_exec_overrides(OsStr::new(
            "/execarg_default:value"
        )));
    }

    #[test]
    fn test_split_default_terminal_arg_exec_overrides() {
        assert_eq!(
            split_default_terminal_arg_exec_overrides(OsStr::new("/execarg_default:entry:value")),
            (OsStr::new("entry"), OsStr::new("value"))
        );
    }

    #[test]
    fn test_split_default_terminal_arg_exec_overrides_when_it_has_multiple_colons() {
        assert_eq!(
            split_default_terminal_arg_exec_overrides(OsStr::new(
                "/execarg_default:entry:value:with:colons"
            )),
            (OsStr::new("entry"), OsStr::new("value:with:colons"))
        );
    }

    #[test]
    fn test_is_potential_entry_that_starts_with_lowercase() {
        assert!(is_potential_entry(OsStr::new("a:")));
    }

    #[test]
    fn test_is_potential_entry_that_starts_with_uppercase() {
        assert!(is_potential_entry(OsStr::new("A:")));
    }

    #[test]
    fn test_is_potential_entry_that_starts_with_digit() {
        assert!(is_potential_entry(OsStr::new("0:")));
    }

    #[test]
    fn test_is_potential_entry_that_starts_with_underscore() {
        assert!(is_potential_entry(OsStr::new("_:")));
    }

    #[test]
    fn test_is_potential_entry_that_starts_with_plus() {
        assert!(is_potential_entry(OsStr::new("+a:")));
    }

    #[test]
    fn test_is_potential_entry_that_starts_with_minus() {
        assert!(is_potential_entry(OsStr::new("-a:")));
    }

    #[test]
    fn test_is_potential_entry_with_an_empty_string() {
        assert!(!is_potential_entry(OsStr::new("")));
    }

    #[test]
    fn test_is_potential_entry_that_starts_with_invalid_character() {
        assert!(!is_potential_entry(OsStr::new("*asdf")));
    }

    #[test]
    fn test_is_potential_entry_that_starts_with_two_plus() {
        assert!(!is_potential_entry(OsStr::new("++asdf")));
    }

    #[test]
    fn test_validate_action_id_with_empty_string() {
        assert!(validate_action_id(&build_debugger(), OsStr::new("")));
    }

    #[test]
    fn test_validate_action_id_with_valid_characters() {
        assert!(validate_action_id(
            &build_debugger(),
            OsStr::new("abcABC123-")
        ));
    }

    #[test]
    fn test_validate_action_id_with_invalid_character_at_the_beginning() {
        assert!(!validate_action_id(
            &build_debugger(),
            OsStr::new("#abcABC123-")
        ));
    }

    #[test]
    fn test_validate_action_id_with_invalid_character_at_the_middle() {
        assert!(!validate_action_id(
            &build_debugger(),
            OsStr::new("abcAB#C123-")
        ));
    }

    #[test]
    fn test_validate_action_id_with_invalid_character_at_the_end() {
        assert!(!validate_action_id(
            &build_debugger(),
            OsStr::new("abcABC123-#")
        ));
    }

    #[test]
    fn test_validate_action_id_with_only_invalid_character() {
        assert!(!validate_action_id(&build_debugger(), OsStr::new("#")));
    }

    #[test]
    fn test_split_entry_id_and_action_id() {
        assert_eq!(
            split_entry_id_and_action_id(OsStr::new("entry_id:action_id")),
            (OsStr::new("entry_id"), OsStr::new("action_id"))
        );
    }

    #[test]
    fn test_split_entry_id_and_action_id_with_multiple_colons() {
        assert_eq!(
            split_entry_id_and_action_id(OsStr::new("entry_id:value1:value2")),
            (OsStr::new("entry_id"), OsStr::new("value1:value2"))
        );
    }

    #[test]
    fn test_split_entry_id_and_action_id_without_colons() {
        assert_eq!(
            split_entry_id_and_action_id(OsStr::new("entry_id")),
            (OsStr::new("entry_id"), OsStr::new(""))
        );
    }

    #[test]
    fn test_split_entry_id_and_action_id_with_colon_at_the_end() {
        assert_eq!(
            split_entry_id_and_action_id(OsStr::new("entry_id:")),
            (OsStr::new("entry_id"), OsStr::new(""))
        );
    }

    #[test]
    fn test_list_contains_for_empty_entry_with_empty_list() {
        assert!(list_contains(OsStr::new(""), OsStr::new(""), None));
    }

    #[test]
    fn test_list_contains_for_non_empty_entry_with_empty_list() {
        assert!(!list_contains(OsStr::new(""), OsStr::new("entry3"), None));
    }

    #[test]
    fn test_list_contains_for_empty_entry_with_non_empty_list() {
        assert!(!list_contains(
            OsStr::new(&format!("entry1{LF}entry2{LF}entry3")),
            OsStr::new(""),
            None
        ));
    }

    #[test]
    fn test_list_contains_for_entry_in_list_with_default_delimeter() {
        assert!(list_contains(
            OsStr::new(&format!("entry1{LF}entry2{LF}entry3")),
            OsStr::new("entry3"),
            None
        ));
    }

    #[test]
    fn test_list_contains_for_entry_not_in_list_with_default_delimeter() {
        assert!(!list_contains(
            OsStr::new(&format!("entry1{LF}entry2{LF}entry3")),
            OsStr::new("entry4"),
            None
        ));
    }

    #[test]
    fn test_list_contains_for_entry_in_list_with_empty_delimeter() {
        assert!(list_contains(
            OsStr::new(&format!("entry1{LF}entry2{LF}entry3")),
            OsStr::new("entry3"),
            Some(OsStr::new(""))
        ));
    }

    #[test]
    fn test_list_contains_for_entry_not_in_list_with_empty_delimeter() {
        assert!(!list_contains(
            OsStr::new(&format!("entry1{LF}entry2{LF}entry3")),
            OsStr::new("entry4"),
            Some(OsStr::new(""))
        ));
    }

    #[test]
    fn test_list_contains_for_entry_in_list_with_custom_delimeter() {
        assert!(list_contains(
            OsStr::new(&format!("entry1:entry2:entry3")),
            OsStr::new("entry3"),
            Some(OsStr::new(":"))
        ));
    }

    #[test]
    fn test_list_contains_for_entry_not_in_list_with_custom_delimeter() {
        assert!(!list_contains(
            OsStr::new(&format!("entry1:entry2:entry3")),
            OsStr::new("entry4"),
            Some(OsStr::new(":"))
        ));
    }
}
