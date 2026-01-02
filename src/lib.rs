//! Supporting functions for `xdg-terminal-exec`
//!
//! This provides "utility" functions for the `xdg-terminal-exec` command. The
//! actual implementation is located on the `main.rs` file.

use std::{
    char::TryFromCharError,
    env::{self, VarError},
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStrExt,
};

pub mod debug;

/// Method to retrieve an environment variable as an optional [`OsString`].
///
/// Given this program was origanlly a shell script most of the interactions
/// can be done as a `OsString` so is simpler to figure out if an env variable
/// is not set regardless of the enconding of its contents.
pub fn env_var<T>(var_name: T) -> Option<OsString>
where
    T: AsRef<OsStr>,
{
    match env::var(var_name) {
        Ok(value) => Some(OsString::from(value)),
        Err(VarError::NotUnicode(value)) => Some(OsString::from(value)),
        Err(VarError::NotPresent) => None,
    }
}

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
pub fn check_bool<T>(value: T) -> bool
where
    T: AsRef<OsStr>,
{
    let value = value.as_ref();
    const TRUE_CONSTANTS: &'static [&str] = &["true", "True", "TRUE", "yes", "Yes", "YES", "1"];
    const FALSE_CONSTANTS: &'static [&str] = &["false", "False", "FALSE", "no", "No", "NO", "0"];

    if TRUE_CONSTANTS.iter().any(|constant| *constant == value) {
        true
    } else {
        if FALSE_CONSTANTS.iter().all(|constant| *constant != value) {
            eprintln!("Assuming {} means no", value.display());
        }

        false
    }
}

/// Emplaces `value` into the "*colon separate value*" list `list`.
///
/// The original implementation of `xdg-terminal-exec` is written as a shell
/// script and relies heavily on variables using `:` as the record separator.
/// This function is a straighforward *emplace* implementation for this concept:
/// if the list is empty then the value gets added, if is not empty the value
/// gets added suffixed by `:`. It also operates on [`OsString`]s and
/// [`OsStr`]s, again, given the original implementation is written as a shell
/// script.
///
/// ```
/// use xdg_terminal_exec::emplace_to_csv_list;
///
/// let list = String::new();
///
/// let list = emplace_to_csv_list(&list, "value1");
/// assert_eq!(list, "value1");
///
/// let list = emplace_to_csv_list(&list, "value2");
/// assert_eq!(list, "value2:value1");
/// ```
#[must_use]
pub fn emplace_to_csv_list<T1, T2>(list: T1, value: T2) -> OsString
where
    T1: AsRef<OsStr>,
    T2: AsRef<OsStr>,
{
    let list = list.as_ref();
    let mut result = OsString::from(value.as_ref());

    if !list.is_empty() {
        result.push(":");
        result.push(list);
    }

    result
}

/// Pushes `value` into the "*colon separate value*" list `list`.
///
/// The original implementation of `xdg-terminal-exec` is written as a shell
/// script and relies heavily on variables using `:` as the record separator.
/// This function is a straighforward *push* implementation for this concept:
/// if the list is empty then the value gets added, if is not empty the value
/// gets added prefixed by `:`. It also operates on [`OsString`]s and
/// [`OsStr`]s, again, given the original implementation is written as a shell
/// script.
///
/// ```
/// use xdg_terminal_exec::push_to_csv_list;
///
/// let list = String::new();
///
/// let list = push_to_csv_list(&list, "value1");
/// assert_eq!(list, "value1");
///
/// let list = push_to_csv_list(&list, "value2");
/// assert_eq!(list, "value1:value2");
/// ```
#[must_use]
pub fn push_to_csv_list<T1, T2>(list: T1, value: T2) -> OsString
where
    T1: AsRef<OsStr>,
    T2: AsRef<OsStr>,
{
    let mut result = OsString::from(list.as_ref());
    if !result.is_empty() {
        result.push(":");
    }

    result.push(value);

    result
}

/// Splits anything that can become a reference to an [`OsStr`] with the given
/// separator.
///
/// If the `separator` fails to be converted to a [`u8`] then a
/// [`TryFromCharError`] is returned instead.
///
/// ```
/// use std::ffi::OsString;
/// use xdg_terminal_exec::os_str_split;
///
/// assert_eq!(
///     os_str_split(&"value1:value2", ':').unwrap(),
///     vec![&OsString::from("value1"), &OsString::from("value2")]
/// );
/// ```
pub fn os_str_split<T>(string: &T, separator: char) -> Result<Vec<&OsStr>, TryFromCharError>
where
    T: AsRef<OsStr>,
{
    let string = string.as_ref();
    let separator = u8::try_from(separator)?;
    let bytes = string.as_bytes();
    let mut result = Vec::<&OsStr>::new();
    let mut n = 0;

    for (m, byte) in bytes.iter().enumerate() {
        if *byte == separator {
            result.push(OsStr::from_bytes(&bytes[n..m]));
            n = m + 1;
        }
    }

    result.push(OsStr::from_bytes(&bytes[n..]));

    Ok(result)
}

/// idem, this method works only on platforms where characters for `OsString`
/// are 8 bits wide.
pub fn os_str_remove_trailing_slash<T>(string: &T) -> &OsStr
where
    T: AsRef<OsStr>,
{
    const SLASH: u8 = 47;
    let string = string.as_ref();
    let bytes = string.as_bytes();

    match bytes.split_last() {
        Some((last_byte, rest)) => {
            if *last_byte == SLASH {
                OsStr::from_bytes(rest)
            } else {
                string
            }
        }
        None => string,
    }
}

/// idem, this method is to cover for the absense of a `format!` macro that's
/// able to take `OsString` or `OsStr` values.
///
/// It is considerably less readable in comparison to `format!` but is the tool
/// that's available, see the following example:
///
/// ```
/// use std::ffi::{OsStr, OsString};
/// use xdg_terminal_exec::os_str_concat;
///
/// // Example with `format!`
/// let value1 = "value1";
/// let value2 = "value2";
/// let string = format!("{value1} is a {value2}");
///
/// // Equivalent with `os_str_concat`
/// let value1 = OsString::from("value1");
/// let value2 = OsStr::new("value2");
/// let os_str = os_str_concat(&[
///     value1.as_os_str(),
///     OsStr::new(" is a "),
///     value2,
/// ]);
///
/// assert_eq!(OsString::from(string), os_str);
/// ```
pub fn os_str_concat<T>(values: &[T]) -> OsString
where
    T: AsRef<OsStr>,
{
    let mut result = OsString::new();

    for value in values {
        result.push(value);
    }

    result
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
        assert_eq!(emplace_to_csv_list("", "value"), "value");
    }

    #[test]
    fn test_emplace_to_non_empty_csv_list() {
        assert_eq!(emplace_to_csv_list("existing", "value"), "value:existing");
    }

    #[test]
    fn test_push_to_empty_csv_list() {
        assert_eq!(push_to_csv_list("", "value"), "value");
    }

    #[test]
    fn test_push_to_non_empty_csv_list() {
        assert_eq!(push_to_csv_list("existing", "value"), "existing:value");
    }

    #[test]
    fn test_os_str_split() {
        assert_eq!(
            os_str_split(&"value1:value2", ':').unwrap(),
            vec![&OsString::from("value1"), &OsString::from("value2")]
        );
    }

    #[test]
    fn test_os_str_split_with_empty_string() {
        assert_eq!(os_str_split(&"", ':').unwrap(), vec![&OsString::from("")],);
    }

    #[test]
    fn test_os_str_split_with_separator_at_the_beginning() {
        assert_eq!(
            os_str_split(&":value1:value2", ':').unwrap(),
            vec![
                &OsString::from(""),
                &OsString::from("value1"),
                &OsString::from("value2")
            ]
        );
    }

    #[test]
    fn test_os_str_split_with_separator_at_the_end() {
        assert_eq!(
            os_str_split(&"value1:value2:", ':').unwrap(),
            vec![
                &OsString::from("value1"),
                &OsString::from("value2"),
                &OsString::from("")
            ]
        );
    }

    #[test]
    fn test_os_str_split_with_separator_thats_not_u8() -> Result<(), String> {
        match os_str_split(&"value1:value2:", '🦀') {
            Ok(value) => Err(format!("Was expecting to fail but got {:?}", value)),
            Err(_) => Ok(()),
        }
    }

    #[test]
    fn test_os_str_remove_trailing_slash_when_no_slash() {
        assert_eq!(
            os_str_remove_trailing_slash(&"value"),
            OsString::from("value").as_os_str()
        );
    }

    #[test]
    fn test_os_str_remove_trailing_slash_when_slash_at_the_end() {
        assert_eq!(
            os_str_remove_trailing_slash(&"value/"),
            OsString::from("value").as_os_str()
        );
    }

    #[test]
    fn test_os_str_remove_trailing_slash_when_slash_not_at_the_end() {
        assert_eq!(
            os_str_remove_trailing_slash(&"/value"),
            OsString::from("/value").as_os_str()
        );
    }

    #[test]
    fn test_os_str_concat_with_empty_values() {
        assert_eq!(os_str_concat(&Vec::<&OsStr>::new()), OsString::from(""));
    }

    #[test]
    fn test_os_str_concat_with_one_value() {
        assert_eq!(os_str_concat(&vec!(&"value")), OsString::from("value"));
    }

    #[test]
    fn test_os_str_concat_with_multiple_values() {
        assert_eq!(
            os_str_concat(&vec!(&"value1", "value2", "value3")),
            OsString::from("value1value2value3")
        );
    }
}
