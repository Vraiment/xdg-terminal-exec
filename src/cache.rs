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
    ffi::OsStr,
    fmt::Display,
    io::{self, Write},
    process::{Child, Command, Stdio},
};

use crate::debug::Debugger;

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

fn write_script_to_stdin(child: &mut Child, script: &str) -> Result<(), ScriptExecutionError> {
    match child.stdin.take() {
        Some(mut stdin) => stdin
            .write_all(script.as_bytes())
            .map_err(ScriptExecutionError::StdinWriteError),
        None => Err(ScriptExecutionError::MissingStdinError),
    }
}

/// Invokes the given `script` using `/bin/sh` and setting the environment variables
/// to the given values. Any call site for this should be substituted by proper
/// native Rust code in the future.
fn execute_sh_script_with_env<K, V>(
    debugger: &Box<dyn Debugger>,
    env: &HashMap<K, Option<V>>,
    script: &str,
) -> Result<Vec<u8>, ScriptExecutionError>
where
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
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

fn execute_sh_script(
    debugger: &Box<dyn Debugger>,
    script: &str,
) -> Result<Vec<u8>, ScriptExecutionError> {
    execute_sh_script_with_env(debugger, &HashMap::<&OsStr, Option<&OsStr>>::new(), script)
}

fn gen_hash<T>(
    debugger: &Box<dyn Debugger>,
    xdg_current_desktop: T,
    xte_configs: T,
    xte_applications_dirs: T,
) -> Result<String, ScriptExecutionError>
where
    T: AsRef<OsStr>,
{
    if debugger.is_enabled() {
        let mut message = vec![format!(
            ">     hashing '{}' and listing of:",
            xdg_current_desktop.as_ref().display()
        )];
        format!(
            "{}:{}",
            xte_configs.as_ref().display(),
            xte_applications_dirs.as_ref().display()
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
            ("XDG_CURRENT_DESKTOP", Some(xdg_current_desktop)),
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

    use crate::cache::*;
    use crate::debug::Debugger;

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
    //     EOH
    //
    //     echo "$xte__hash"
    // }
    //
    // cat << EOF
    // // DO NOT COMMIT THESE CHANGES
    // let xdg_current_desktop = "$XDG_CURRENT_DESKTOP";
    // let xte_configs = "$XTE__CONFIGS";
    // let xte_applications_dirs = "$XTE__APPLICATIONS_DIRS";
    // let expected_hash = "$xte__hash";
    // EOF
    // ```
    #[test]
    #[ignore]
    fn test_gen_hash() {
        let debugger = TestingDebugger::default();
        // Replace the lines below with the values generated by the script
        let xdg_current_desktop = "";
        let xte_configs = "";
        let xte_applications_dirs = "";
        let expected_hash = "";

        let result = gen_hash(
            &(Box::new(debugger) as Box<dyn Debugger>),
            xdg_current_desktop,
            &xte_configs,
            &xte_applications_dirs,
        )
        .unwrap();

        assert_eq!(result, expected_hash);
    }
}
