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

/// Emplaces `value` into the "*colon separate value*" list `list`.
///
/// The original implementation of `xdg-terminal-exec` is written in bash and
/// relies heavily on variables using `:` as the record separator. This function
/// is a straighforward *emplace* implementation for this concept: if the list is
/// empty then the value gets added, if is not empty the value gets added
/// suffixed by `:`.
///
///
/// ```
/// use xdg_terminal_exec::emplace_to_csv_list;
///
/// let mut list = String::new();
///
/// emplace_to_csv_list(&mut list, "value1");
/// assert_eq!(list, "value1");
///
/// emplace_to_csv_list(&mut list, "value2");
/// assert_eq!(list, "value2:value1");
/// ```
pub fn emplace_to_csv_list(list: &mut String, value: &str) {
    if !list.is_empty() {
        list.insert(0, ':');
    }

    list.insert_str(0, value);
}

/// Pushes `value` into the "*colon separate value*" list `list`.
///
/// The original implementation of `xdg-terminal-exec` is written in bash and
/// relies heavily on variables using `:` as the record separator. This function
/// is a straighforward *push* implementation for this concept: if the list is
/// empty then the value gets added, if is not empty the value gets added
/// prefixed by `:`.
///
///
/// ```
/// use xdg_terminal_exec::push_to_csv_list;
///
/// let mut list = String::new();
///
/// push_to_csv_list(&mut list, "value1");
/// assert_eq!(list, "value1");
///
/// push_to_csv_list(&mut list, "value2");
/// assert_eq!(list, "value1:value2");
/// ```
pub fn push_to_csv_list(list: &mut String, value: &str) {
    if !list.is_empty() {
        list.push(':');
    }

    list.push_str(value);
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

    #[test]
    fn test_emplace_to_empty_csv_list() {
        let mut list = String::new();

        emplace_to_csv_list(&mut list, "value");

        assert_eq!(list, "value");
    }

    #[test]
    fn test_emplace_to_non_empty_csv_list() {
        let mut list = String::from("existing");

        emplace_to_csv_list(&mut list, "value");

        assert_eq!(list, "value:existing");
    }

    #[test]
    fn test_push_to_empty_csv_list() {
        let mut list = String::new();

        push_to_csv_list(&mut list, "value");

        assert_eq!(list, "value");
    }

    #[test]
    fn test_push_to_non_empty_csv_list() {
        let mut list = String::from("existing");

        push_to_csv_list(&mut list, "value");

        assert_eq!(list, "existing:value");
    }
}
