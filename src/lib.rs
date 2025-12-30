//! Supporting functions for `xdg-terminal-exec`
//!
//! This provides "utility" functions for the `xdg-terminal-exec` command. The
//! actual implementation is located on the `main.rs` file.

pub mod debug;

/// This method converts a string to a typed [`bool`]
///
/// There's only specific values that are considered [`true`] or [`false`]
/// (case sensitive):
///
/// - `true` / `false`
/// - `True` / `False`
/// - `TRUE` / `FALSE`
/// - `yes` / `no`
/// - `Yes` / `No`
/// - `YES` / `NO`
/// - `1` / `0`
///
/// Any other value will be treated as [`false`] and a warning will be printed
pub fn check_bool(value: &str) -> bool {
    const TRUE_CONSTANTS: &'static [&str] = &["true", "True", "TRUE", "yes", "Yes", "YES", "1"];
    const FALSE_CONSTANTS: &'static [&str] = &["false", "False", "FALSE", "no", "No", "NO", "0"];

    if TRUE_CONSTANTS.iter().any(|constant| *constant == value) {
        true
    } else {
        if FALSE_CONSTANTS.iter().all(|constant| *constant != value) {
            eprintln!("Assuming {} means no", value);
        }

        false
    }
}

#[cfg(test)]
mod test {
    use crate::*;

    #[test]
    fn test_check_bool_for_true_values() -> Result<(), String> {
        ["true", "True", "TRUE", "yes", "Yes", "YES", "1"]
            .iter()
            .find(|value| !check_bool(value))
            .map(|value| Err(format!("check_bool({value}) should return true")))
            .unwrap_or(Ok(()))
    }

    #[test]
    fn test_check_bool_for_false_values() -> Result<(), String> {
        ["false", "False", "FALSE", "no", "No", "NO", "0"]
            .iter()
            .find(|value| check_bool(value))
            .map(|value| Err(format!("check_bool({value}) should return false")))
            .unwrap_or(Ok(()))
    }

    #[test]
    fn test_check_bool_for_any_value() -> Result<(), String> {
        let value = "any value";

        if !check_bool(value) {
            Ok(())
        } else {
            Err(String::from("check_bool(any_value) should return false"))
        }
    }
}
