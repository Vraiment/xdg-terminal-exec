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
//! debugger.print(&[&"value1", &"value2"]);
//! ```
use std::{env, fmt::Display};

use crate::*;

/// Trait to describe a debugger object.
///
/// To create a new instance use [`build_debugger()`].
pub trait Debugger {
    /// Prints the given slice of [`Display`] objects to the debugging output.
    ///
    /// ```
    /// use xdg_terminal_exec::debug::build_debugger;
    ///
    /// let debugger = build_debugger();
    ///
    /// debugger.print(&[&"value1", &"value2"]);
    /// ```
    fn print(&self, args: &[&dyn Display]);

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
    let enabled = env::var("XTE_DEBUG")
        .or(env::var("DEBUG"))
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
    fn print(&self, _args: &[&dyn Display]) {}

    fn is_enabled(&self) -> bool {
        false
    }
}

impl Debugger for StdErrDebugger {
    fn print(&self, args: &[&dyn Display]) {
        for &arg in args {
            eprintln!("D: {}", arg);
        }
    }

    fn is_enabled(&self) -> bool {
        true
    }
}
