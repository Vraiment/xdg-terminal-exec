//! Caching functions
//!
//! This module contains functions related to caching. Details about how the
//! cache works will be provided later.

use std::{
    collections::HashMap,
    ffi::OsStr,
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
}
