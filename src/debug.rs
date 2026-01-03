//! Debugging functions
//!
//! This module contains simple debugging logic for `xdg-term-exec`. The two
//! core components here are the [`Debugger`] trait and the [`build_debugger()`]
//! function that will create a debugger instance.
//!
//! ```
//! use xdg_terminal_exec::debug::*;
//!
//! let debugger = build_debugger();
//!
//! println!("Debugger is enabled: {}", debugger.is_enabled());
//! debugger.print_slice(&[&"value1", &"value2"]);
//! ```
use std::fmt::Display;

use super::*;

/// Trait to describe a debugger object.
///
/// To create a new instance use [`build_debugger()`].
pub trait Debugger {
    /// Prints the given [`Display`] object to the debugging output.
    ///
    /// ```
    /// use xdg_terminal_exec::debug::build_debugger;
    ///
    /// let debugger = build_debugger();
    ///
    /// debugger.print_line(&"my value");
    /// ```
    fn print_line(&self, arg: &dyn Display);

    /// Prints the given slice of [`Display`] objects to the debugging output.
    ///
    /// Each individual entry is considered a line.
    ///
    /// ```
    /// use xdg_terminal_exec::debug::build_debugger;
    ///
    /// let debugger = build_debugger();
    ///
    /// debugger.print_slice(&[&"value1", &"value2"]);
    /// ```
    fn print_slice(&self, slice: &[&dyn Display]);

    /// Prints the given [`Vec`] of [`Display`] objects to the debugging output.
    ///
    /// Each individual entry is considered a line.
    ///
    /// ```
    /// use xdg_terminal_exec::debug::build_debugger;
    ///
    /// let debugger = build_debugger();
    ///
    /// let values: Vec<&dyn std::fmt::Display> = vec![&"value1", &"value2"];
    /// debugger.print_vec(&values);
    /// ```
    fn print_vec(&self, vec: &Vec<&dyn Display>);

    /// Returns whether the debugger is enabled (prints anything to anywhere) or
    /// not.
    fn is_enabled(&self) -> bool;
}

/// Creates a new [`Debugger`] instance based on environment variables.
///
/// If debugging is enabled a [`Debugger`] that prints to [`std::io::Stderr`]
/// will be created, otherwise a no-op [`Debugger`] will be created.
///
/// The state of the debugger is decided based on the `XTE_DEBUG` environment
/// variable, if is set to a value that [`check_bool()`] considers [`true`] then
/// it is considered enabled, otherwise it is considered siabled. If the
/// environment variable `XTE_DEBUG` is not set, then the value of the
/// environment variable `DEBUG` is used with the same logic.
pub fn build_debugger() -> Box<dyn Debugger> {
    let enabled = env_var("XTE_DEBUG")
        .or(env_var("DEBUG"))
        .map(|value| check_bool(&value))
        .unwrap_or(false);

    if enabled {
        Box::new(StdErrDebugger {})
    } else {
        Box::new(NoOpDebugger {})
    }
}

struct NoOpDebugger;

struct StdErrDebugger;

impl Debugger for NoOpDebugger {
    fn print_line(&self, _arg: &dyn Display) {}

    fn print_slice(&self, _slice: &[&dyn Display]) {}

    fn print_vec(&self, _vec: &Vec<&dyn Display>) {}

    fn is_enabled(&self) -> bool {
        false
    }
}

impl Debugger for StdErrDebugger {
    fn print_line(&self, arg: &dyn Display) {
        eprintln!("D: {}", arg);
    }

    fn print_slice(&self, slice: &[&dyn Display]) {
        for &entry in slice {
            self.print_line(entry);
        }
    }

    fn print_vec(&self, vec: &Vec<&dyn Display>) {
        for &entry in vec {
            self.print_line(entry);
        }
    }

    fn is_enabled(&self) -> bool {
        true
    }
}
