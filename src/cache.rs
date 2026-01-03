//! Caching functions
//!
//! This module contains functions related to caching. Details about how the
//! cache works will be provided later.
//!
//! It has an ungodly implementation, namely the `gen_hash` function that calls
//! /bin/sh directly, this is to maintain backwards compatibility with the
//! original /bin/sh implementation. It should be replaced with a native Rust
//! implementation in the future.

use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    fmt::Display,
    io::{self, Write},
    os::unix::ffi::OsStrExt,
    path::Path,
    process::{Child, Command, Stdio},
};

use crate::{debug::Debugger, env_var, os_str_concat, os_str_read_lines, os_str_strip_suffix};

const LF: &str = r#"
"#;
const RSEP: char = '\u{1E}';

/// Struct with the data that's cached between `xdg-terminal-exec`.
///
/// Fields are self explanatory.
#[derive(Default)]
pub struct Cache {
    hash: OsString,
    cmd: OsString,
    pub exec_usep: OsString,
    pub entry_path: OsString,
    pub entry_id: OsString,
    pub entry_action: OsString,
    pub execarg: OsString,
    pub appidarg: OsString,
    pub titlearg: OsString,
    pub dirarg: OsString,
    pub holdarg: OsString,
}

/// Errors that can happen while reading the cache.
#[derive(Debug)]
pub enum ReadCacheError {
    // An IO error happened while readin the cache file
    CacheReadError(io::Error),
    // An error ocurred while executing a script that's used to calculate the
    // cache.
    ScriptExecutionError(ScriptExecutionError),
}

impl From<ScriptExecutionError> for ReadCacheError {
    fn from(error: ScriptExecutionError) -> Self {
        ReadCacheError::ScriptExecutionError(error)
    }
}

impl From<io::Error> for ReadCacheError {
    fn from(error: io::Error) -> Self {
        ReadCacheError::CacheReadError(error)
    }
}

/// Errors for when executing a script, entries should be self explanatory.
#[derive(Debug)]
pub enum ScriptExecutionError {
    MissingStdinError,
    ScriptError(i32),
    ScriptExitedWithSignalError,
    SpawnError(io::Error),
    StdinWriteError(io::Error),
    WaitError(io::Error),
}

fn check_cached_cmd<T>(
    debugger: &Box<dyn Debugger>,
    cached_cmd: T,
) -> Result<bool, ScriptExecutionError>
where
    T: AsRef<OsStr>,
{
    // Equivalent to `format!("command -v {cached_cmd} > /dev/null")`
    let script = os_str_concat(&[
        OsStr::new("command -v "),
        cached_cmd.as_ref(),
        OsStr::new(" > /dev/null"),
    ]);

    match execute_sh_script(debugger, script) {
        Ok(_) => Ok(true),
        Err(error) => match error {
            ScriptExecutionError::ScriptError(_) => Ok(false),
            _ => Err(error),
        },
    }
}

/// Returns an optional [`Cache`] entry or if there was a major error reading
/// the cache a [`ReadCacheError`].
pub fn read_cache<T1, T2, T3>(
    debugger: &Box<dyn Debugger>,
    xte_cache_file: T1,
    xte_configs: T2,
    xte_applications_dirs: T3,
) -> Result<Option<Cache>, ReadCacheError>
where
    T1: AsRef<OsStr>,
    T2: AsRef<OsStr>,
    T3: AsRef<OsStr>,
{
    let cache_file = Path::new(xte_cache_file.as_ref());

    if cache_file.exists() {
        let mut line_num = 0;
        const LINE_LIMIT: u8 = 50;
        let mut finished = false;
        let mut cache: Cache = Cache::default();

        for line in os_str_read_lines(cache_file)?.map_while(Result::ok) {
            line_num += 1;
            match line_num {
                1 => cache.hash = line,
                2 => cache.cmd = line,
                3 => cache.entry_path = line,
                4 => cache.entry_id = line,
                5 => cache.entry_action = line,
                6 => cache.execarg = line,
                7 => cache.appidarg = line,
                8 => cache.titlearg = line,
                9 => cache.dirarg = line,
                10 => cache.holdarg = line,
                LINE_LIMIT => {
                    debugger.print_line(&format!("reached cache line limit ({LINE_LIMIT})"));
                    return Ok(None);
                }
                _ => {
                    // Command is stored as raw expanded and tokenized $XTE__USEP-separated command,
                    // technically it can contain newline characters.
                    // Reconstruct newlines, use ${XTE__RSEP}END_OF_EXEC_USEP string as terminator.
                    let cache_exec_usep = cache.exec_usep.clone();
                    cache.exec_usep = if cache_exec_usep.is_empty() {
                        line.clone()
                    } else {
                        // Equivalent to `format!("{}{LF}{line}", cache.exec_usep)`
                        os_str_concat(&[cache_exec_usep.as_os_str(), OsStr::new(LF), &line])
                    };

                    let cache_exec_usep = os_str_strip_suffix(
                        &cache_exec_usep,
                        // Equivalent to `format!("{RSEP}END_OF_EXEC_USEP")`
                        os_str_concat(&[
                            OsString::from(RSEP.to_string()).as_os_str(),
                            OsStr::new("END_OF_EXEC_USEP"),
                        ]),
                    );

                    if let Some(cache_exec_usep) = cache_exec_usep {
                        cache.exec_usep = cache_exec_usep.to_owned();
                        finished = true;
                        break;
                    }
                }
            }
        }

        if finished {
            debugger.print_slice(&[
                &"got cache",
                &format!("hash={}", cache.hash.display()),
                &format!("cmd={}", cache.cmd.display()),
                &format!("entry_path={}", cache.entry_path.display()),
                &format!("entry_id={}", cache.entry_id.display()),
                &format!("entry_action={}", cache.entry_action.display()),
                &format!("execarg={}", cache.execarg.display()),
                &format!("appidarg={}", cache.appidarg.display()),
                &format!("titlearg={}", cache.titlearg.display()),
                &format!("dirarg={}", cache.dirarg.display()),
                &format!("holdarg={}", cache.holdarg.display()),
                &format!("exec_usep={}", cache.exec_usep.display()),
            ]);

            let hash_result = gen_hash(
                debugger,
                xte_configs.as_ref(),
                xte_applications_dirs.as_ref(),
            );
            let hash = match hash_result {
                Ok(hash) => OsString::from(hash),
                Err(error) => match error {
                    ScriptExecutionError::ScriptError(_) => return Ok(None),
                    error => return Err(ReadCacheError::ScriptExecutionError(error)),
                },
            };

            if hash == cache.hash && check_cached_cmd(debugger, &cache.cmd)? {
                debugger.print_line(&"cache is actual");

                Ok(Some(cache))
            } else {
                debugger.print_line(&"cache is out-of-date");

                Ok(None)
            }
        } else {
            debugger.print_line(&"invalid cache data");
            Ok(None)
        }
    } else {
        debugger.print_line(&"no cache data");
        Ok(None)
    }
}

fn write_script_to_stdin<T>(child: &mut Child, script: T) -> Result<(), ScriptExecutionError>
where
    T: AsRef<OsStr>,
{
    match child.stdin.take() {
        Some(mut stdin) => stdin
            .write_all(script.as_ref().as_bytes())
            .map_err(ScriptExecutionError::StdinWriteError),
        None => Err(ScriptExecutionError::MissingStdinError),
    }
}

/// Invokes the given `script` using `/bin/sh` and setting the environment variables
/// to the given values. Any call site for this should be substituted by proper
/// native Rust code in the future.
fn execute_sh_script_with_env<K, V, T>(
    debugger: &Box<dyn Debugger>,
    env: &HashMap<K, Option<V>>,
    script: T,
) -> Result<Vec<u8>, ScriptExecutionError>
where
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
    T: AsRef<OsStr>,
{
    let mut command = Command::new("/bin/sh");

    for (name, value) in env {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }

    let child_result = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    match child_result {
        Ok(mut child) => {
            // Writing to stdin on a different method is important to happen as
            // it will close the `stdin` pipe when the method ends
            write_script_to_stdin(&mut child, script)?;

            match child.wait_with_output() {
                Ok(output) => {
                    if debugger.is_enabled() {
                        match String::from_utf8(output.stderr) {
                            Ok(value) => value
                                .split('\n')
                                .for_each(|line| debugger.print_line(&line)),
                            Err(error) => {
                                debugger.print_line(&format!("Could not read stderr due {error}"))
                            }
                        }
                    }

                    match output.status.code() {
                        Some(0) => Ok(output.stdout),
                        code => match code {
                            Some(return_code) => {
                                Err(ScriptExecutionError::ScriptError(return_code))
                            }
                            None => Err(ScriptExecutionError::ScriptExitedWithSignalError),
                        },
                    }
                }
                Err(error) => Err(ScriptExecutionError::WaitError(error)),
            }
        }
        Err(error) => Err(ScriptExecutionError::SpawnError(error)),
    }
}

fn execute_sh_script<T>(
    debugger: &Box<dyn Debugger>,
    script: T,
) -> Result<Vec<u8>, ScriptExecutionError>
where
    T: AsRef<OsStr>,
{
    execute_sh_script_with_env(debugger, &HashMap::<&OsStr, Option<&OsStr>>::new(), script)
}

fn gen_hash<T1, T2>(
    debugger: &Box<dyn Debugger>,
    xte_configs: T1,
    xte_applications_dirs: T2,
) -> Result<String, ScriptExecutionError>
where
    T1: AsRef<OsStr>,
    T2: AsRef<OsStr>,
{
    let xdg_current_desktop = env_var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let xte_configs = xte_configs.as_ref();
    let xte_applications_dirs = xte_applications_dirs.as_ref();

    if debugger.is_enabled() {
        let mut message = vec![format!(
            ">     hashing '{}' and listing of:",
            xdg_current_desktop.display()
        )];
        format!(
            "{}:{}",
            xte_configs.display(),
            xte_applications_dirs.display()
        )
        .split(':')
        .for_each(|path| message.push(path.to_owned()));
        message.push("^     end of hash listing".to_owned());

        debugger.print_vec(&message.iter().map(|line| line as &dyn Display).collect());
    }

    // Use a script to try to keep the hashing algorithm as close as possible
    // to the original bash implementation
    let script = r#"set -eufx
    xte__hash_paths=${XTE__CONFIGS}:${XTE__APPLICATIONS_DIRS}
    echo 4
    echo "${XDG_CURRENT_DESKTOP-}"
    IFS=':'
    LANG=C ls -LRl ${xte__hash_paths} 2> /dev/null"#;
    // return md5 of custom string, XDG_CURRENT_DESKTOP and ls -LRl output for config and data paths
    // md5 is 4x faster than sha*, and there is no need for cryptography here
    let digest = execute_sh_script_with_env(
        debugger,
        &HashMap::from([
            ("XDG_CURRENT_DESKTOP", Some(xdg_current_desktop.as_os_str())),
            ("XTE__CONFIGS", Some(xte_configs)),
            ("XTE__APPLICATIONS_DIRS", Some(xte_applications_dirs)),
        ]),
        script,
    )
    .map(md5::compute)?;

    Ok(format!("{:x}", digest))
}

#[cfg(test)]
mod test {
    use std::ffi::OsString;
    use std::{cell::RefCell, rc::Rc};

    use super::*;
    use crate::debug::Debugger;
    use crate::testing::with_env;

    #[derive(Default)]
    struct TestingDebugger {
        entries: Rc<RefCell<Vec<String>>>,
    }

    impl TestingDebugger {
        fn get_entries(&self) -> Rc<RefCell<Vec<String>>> {
            self.entries.clone()
        }
    }

    impl Debugger for TestingDebugger {
        fn print_line(&self, arg: &dyn std::fmt::Display) {
            self.entries.borrow_mut().push(format!("{arg}"));
        }

        fn print_slice(&self, slice: &[&dyn std::fmt::Display]) {
            for &entry in slice {
                self.print_line(entry);
            }
        }

        fn print_vec(&self, vec: &Vec<&dyn std::fmt::Display>) {
            for &entry in vec {
                self.print_line(entry);
            }
        }

        fn is_enabled(&self) -> bool {
            true
        }
    }

    #[test]
    fn test_execute_sh_script_with_env() -> Result<(), ScriptExecutionError> {
        let debugger = TestingDebugger::default();
        let debugger_entries = debugger.get_entries();
        let env = HashMap::from([("ENVIRONMENT_VARIABLE", Some(OsString::from("my value")))]);
        let script = r#"echo "$ENVIRONMENT_VARIABLE""#;
        let result = String::from_utf8(execute_sh_script_with_env(
            &(Box::new(debugger) as Box<dyn Debugger>),
            &env,
            script,
        )?)
        .unwrap();

        assert_eq!(result, "my value\n");
        assert_eq!(*debugger_entries.borrow(), vec![String::new()]);

        Ok(())
    }

    #[test]
    fn test_execute_sh_script() -> Result<(), ScriptExecutionError> {
        let debugger = TestingDebugger::default();
        let debugger_entries = debugger.get_entries();
        let script = "echo hello world";
        let result = String::from_utf8(execute_sh_script(
            &(Box::new(debugger) as Box<dyn Debugger>),
            script,
        )?)
        .unwrap();

        assert_eq!(result, "hello world\n");
        assert_eq!(*debugger_entries.borrow(), vec![String::new()]);

        Ok(())
    }

    #[test]
    fn test_execute_sh_script_prints_stderr_to_debugger() -> Result<(), ScriptExecutionError> {
        let debugger = TestingDebugger::default();
        let debugger_entries = debugger.get_entries();
        let script = ">&2 echo hello debugging";
        let result = String::from_utf8(execute_sh_script(
            &(Box::new(debugger) as Box<dyn Debugger>),
            script,
        )?)
        .unwrap();

        assert_eq!(result, "");
        assert_eq!(
            *debugger_entries.borrow(),
            vec![String::from("hello debugging"), String::from("")]
        );

        Ok(())
    }

    #[test]
    fn test_execute_sh_script_exits_non_zero() {
        const EXIT_CODE: i32 = 12;
        let debugger = TestingDebugger::default();
        let script = format!("exit {EXIT_CODE}");
        let result = execute_sh_script(&(Box::new(debugger) as Box<dyn Debugger>), &script)
            .map(|bytes| String::from_utf8(bytes).unwrap());

        match result {
            Ok(output) => assert!(false, "Was expecting to fail, got {output} instead"),
            Err(error) => match error {
                ScriptExecutionError::ScriptError(exit_code) => assert_eq!(
                    exit_code, EXIT_CODE,
                    "Was expecting to fail with exit code {EXIT_CODE}, it failed with {:?} instead",
                    exit_code
                ),
                error => assert!(
                    false,
                    "Was expeting to fail with exit code {EXIT_CODE} failed due {:?} instead",
                    error
                ),
            },
        }
    }

    #[test]
    fn test_execute_sh_script_exits_with_signal() {
        let debugger = TestingDebugger::default();
        let script = format!("kill -15 $$");
        let result = execute_sh_script(&(Box::new(debugger) as Box<dyn Debugger>), &script)
            .map(|bytes| String::from_utf8(bytes).unwrap());

        match result {
            Ok(output) => assert!(false, "Was expecting to fail, got {output} instead"),
            Err(error) => match error {
                ScriptExecutionError::ScriptExitedWithSignalError => {}
                error => assert!(
                    false,
                    "Was expeting to fail with a signal failed due {:?} instead",
                    error
                ),
            },
        };
    }

    // This test is ignored because it needs to be manually setup on each
    // machine and even between reboots to ensure the results are valid as the
    // values used for the hash take into account creation time and user name.
    //
    // Execute the following script to generate a Rust snippet that can be
    // used in this test.
    //
    // ```shell
    // SCRATCH_DIR="$(mktemp --directory)"
    //
    // mkdir --parents \
    //     "$SCRATCH_DIR"/configs/a \
    //     "$SCRATCH_DIR"/configs/b \
    //     "$SCRATCH_DIR"/configs/c \
    //     "$SCRATCH_DIR"/config_symlink_target
    //
    // ln --symbolic "$SCRATCH_DIR"/config_symlink_target "$SCRATCH_DIR"/configs/symlink
    //
    // touch \
    //     "$SCRATCH_DIR"/configs/a/config \
    //     "$SCRATCH_DIR"/configs/b/config \
    //     "$SCRATCH_DIR"/configs/c/config \
    //     "$SCRATCH_DIR"/config_symlink_target/config
    //
    // mkdir --parents \
    //     "$SCRATCH_DIR"/applications/a \
    //     "$SCRATCH_DIR"/applications/b \
    //     "$SCRATCH_DIR"/applications/c \
    //     "$SCRATCH_DIR"/applications_symlink_target
    //
    // touch \
    //     "$SCRATCH_DIR"/applications/a/config \
    //     "$SCRATCH_DIR"/applications/b/config \
    //     "$SCRATCH_DIR"/applications/c/config \
    //     "$SCRATCH_DIR"/applications_symlink_target/config
    //
    // ln --symbolic "$SCRATCH_DIR"/applications_symlink_target "$SCRATCH_DIR"/applications/symlink
    //
    // XDG_CURRENT_DESKTOP=de
    // XTE__CONFIGS="$SCRATCH_DIR"/configs/a:"$SCRATCH_DIR"/configs/b:"$SCRATCH_DIR"/configs/c:"$SCRATCH_DIR"/configs/symlink
    // XTE__APPLICATIONS_DIRS="$SCRATCH_DIR"/applications/a:"$SCRATCH_DIR"/applications/b:"$SCRATCH_DIR"/applications/c:"$SCRATCH_DIR"/applications/symlink
    //
    // function gen_hash() {
    //     # return md5 of custom string, XDG_CURRENT_DESKTOP and ls -LRl output for config and data paths
    //     # md5 is 4x faster than sha*, and there is no need for cryptography here
    //     # writes to XTE__NEW_HASH var
    //     # shellcheck disable=SC2034
    //     read -r xte__hash _drop <<- EOH
    //         $(
    //             xte__hash_paths=${XTE__CONFIGS}:${XTE__APPLICATIONS_DIRS}
    //             {
    //                 # cache 'version', change to invalidate when format changes
    //                 echo 4
    //                 echo "${XDG_CURRENT_DESKTOP-}"
    //                 IFS=':'
    //                 LANG=C ls -LRl ${xte__hash_paths} 2> /dev/null
    //             } | md5sum 2> /dev/null
    //         )
    // EOH
    //
    //     echo "$xte__hash"
    // }
    //
    // cat << EOF
    // // DO NOT COMMIT THESE CHANGES
    // let xdg_current_desktop = "$XDG_CURRENT_DESKTOP";
    // let xte_configs = "$XTE__CONFIGS";
    // let xte_applications_dirs = "$XTE__APPLICATIONS_DIRS";
    // let expected_hash = "$(gen_hash)";
    // EOF
    // ```
    #[test]
    #[ignore]
    fn test_gen_hash() {
        // Replace the lines below with the values generated by the script
        let xdg_current_desktop = "";
        let xte_configs = "";
        let xte_applications_dirs = "";
        let expected_hash = "";

        with_env(
            HashMap::from([("XDG_CURRENT_DESKTOP", Some(xdg_current_desktop))]),
            &mut || {
                let debugger = TestingDebugger::default();

                let result = gen_hash(
                    &(Box::new(debugger) as Box<dyn Debugger>),
                    &xte_configs,
                    &xte_applications_dirs,
                )
                .unwrap();

                assert_eq!(result, expected_hash);
            },
        )
    }
}
