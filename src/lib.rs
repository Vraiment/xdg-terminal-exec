//! Supporting functions for `xdg-terminal-exec`
//!
//! This provides "utility" functions for the `xdg-terminal-exec` command. The
//! actual implementation is located on the `main.rs` file.

use std::{
    char::TryFromCharError,
    env::{self, VarError},
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, BufRead, BufReader},
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::Path,
};

pub mod cache;
pub mod debug;
pub mod testing;

pub const LF: &str = r#"
"#;

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

/// Returns an iterator that reads lines from the given `filename` as `OsString`
/// values.
///
/// ```ignore
/// use std::fs::File;
/// use xdg_terminal_exec::os_str_read_lines;
///
/// let file = Path::new("myfile.txt");
///
/// for line in read_lines(file).unwrap().map_while(Result::ok) {
///     // Line holds an `OsString` with the contents
/// }
/// ```
pub fn os_str_read_lines<P>(filename: P) -> io::Result<impl Iterator<Item = io::Result<OsString>>>
where
    P: AsRef<Path>,
{
    let file = File::open(filename)?;
    let iterator = BufReader::new(file)
        .split(b'\n')
        .map(|result| result.map(OsString::from_vec));

    Ok(iterator)
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

/// Simplified [`OsStr`] equivalent of [`str::strip_suffix`].
///
/// ```
/// use std::ffi::OsStr;
/// use xdg_terminal_exec::os_str_strip_suffix;
///
/// let value = OsStr::new("bar:foo");
/// assert_eq!(os_str_strip_suffix(&value, ":foo"), Some(OsStr::new("bar")));
///
/// let value = OsStr::new("bar:foo");
/// assert_eq!(os_str_strip_suffix(&value, "bar"), None);
///
/// let value = OsStr::new("foofoo");
/// assert_eq!(os_str_strip_suffix(&value, "foo"), Some(OsStr::new("foo")));
/// ```
pub fn os_str_strip_suffix<'a, T1, T2>(string: &'a T1, suffix: T2) -> Option<&'a OsStr>
where
    T1: AsRef<OsStr>,
    T2: AsRef<OsStr>,
{
    let string_bytes = string.as_ref().as_bytes();
    let suffix_bytes = suffix.as_ref().as_bytes();

    let mut n = string_bytes.len() - 1;
    for suffix_byte in suffix_bytes.iter().rev() {
        if string_bytes[n] != *suffix_byte {
            return None;
        }

        n -= 1;
    }

    Some(OsStr::from_bytes(&string_bytes[..n + 1]))
}

/// idem, true if `string` starts with `value`
///
/// ```
/// use xdg_terminal_exec::os_str_starts_with;
///
/// // `value1` starts with `value1`
/// assert!(os_str_starts_with("value1", "value1"));
///
/// // `value1 extra text` starts with `value1`
/// assert!(os_str_starts_with("value1 extra text", "value1"));
///
/// // `value1 extra text` doesn't start with `value2`
/// assert!(!os_str_starts_with("value1 extra text", "value2"));
///
/// // `value1` doesn't start with `value1 extra text`
/// assert!(!os_str_starts_with("value1", "value1 extra text"));
/// ```
pub fn os_str_starts_with<T1, T2>(string: T1, value: T2) -> bool
where
    T1: AsRef<OsStr>,
    T2: AsRef<OsStr>,
{
    let string = string.as_ref();
    let value = value.as_ref();

    if value.len() > string.len() {
        return false;
    }

    let string_bytes = string.as_bytes();
    for (n, byte) in value.as_bytes().iter().enumerate() {
        if *byte != string_bytes[n] {
            return false;
        }
    }

    true
}

/// Trims whitespaces from the beginning and end of the given `string`
///
/// ```
/// use xdg_terminal_exec::os_str_trim;
///
/// assert_eq!(os_str_trim(&"a random string"), "a random string");
/// assert_eq!(os_str_trim(&" \t\na random string"), "a random string");
/// assert_eq!(os_str_trim(&"a random string \t\n"), "a random string");
/// assert_eq!(os_str_trim(&" \t\na random string \t\n"), "a random string");
/// assert_eq!(os_str_trim(&" \t\n"), ""); // Removes everything
/// ```
pub fn os_str_trim<'a, T>(string: &'a T) -> &'a OsStr
where
    T: AsRef<OsStr>,
{
    let string = string.as_ref();
    let bytes = string.as_bytes();

    const WHITESPACE_CHARACTERS: &[u8] = &[0x20, 0x09, 0x0A];

    let mut start = 0;
    while start < bytes.len() {
        if !WHITESPACE_CHARACTERS.contains(&bytes[start]) {
            break;
        }

        start += 1;
    }

    let mut end = bytes.len() - 1;
    while end >= start {
        if !WHITESPACE_CHARACTERS.contains(&bytes[end]) {
            break;
        }

        end -= 1;
    }

    OsStr::from_bytes(&bytes[start..=end])
}

#[cfg(test)]
mod test {
    use std::fs;

    use super::testing::TempFile;

    use super::*;

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

    #[test]
    fn test_os_str_read_lines_with_empty_file() {
        let dev_null = Path::new("/dev/null");
        let mut result: Vec<OsString> = vec![];

        for line in os_str_read_lines(dev_null).unwrap().map_while(Result::ok) {
            result.push(line);
        }

        assert_eq!(result, Vec::<OsString>::new());
    }

    #[test]
    fn test_os_str_read_lines_with_multiple_lines() {
        let temp_file = TempFile::new().unwrap();
        let temp_file = temp_file.path();
        let expected = vec![
            OsString::from("value 1"),
            OsString::from("line 2"),
            OsString::from("entry 3"),
        ];

        let mut contents: Vec<u8> = Vec::new();
        for line in &expected {
            contents.extend_from_slice(line.as_bytes());
            contents.push(10); // Add the `\n` character
        }
        fs::write(temp_file, contents).unwrap();

        let mut result: Vec<OsString> = vec![];

        for line in os_str_read_lines(temp_file).unwrap().map_while(Result::ok) {
            result.push(line);
        }

        assert_eq!(result, expected);
    }

    #[test]
    fn test_os_str_read_lines_with_empty_lines() {
        let temp_file = TempFile::new().unwrap();
        let temp_file = temp_file.path();
        let expected = vec![OsString::from(""), OsString::from(""), OsString::from("")];

        let mut contents: Vec<u8> = Vec::new();
        for line in &expected {
            contents.extend_from_slice(line.as_bytes());
            contents.push(10); // Add the `\n` character
        }
        fs::write(temp_file, contents).unwrap();

        let mut result: Vec<OsString> = vec![];

        for line in os_str_read_lines(temp_file).unwrap().map_while(Result::ok) {
            result.push(line);
        }

        assert_eq!(result, expected);
    }

    #[test]
    fn test_os_str_strip_suffix_when_suffix_is_valid() {
        let string = String::from("value:suffix");

        assert_eq!(
            os_str_strip_suffix(&string, ":suffix"),
            Some(OsStr::new("value")),
        );
    }

    #[test]
    fn test_os_str_strip_suffix_when_suffix_is_not_valid() {
        let string = String::from("value");

        assert_eq!(os_str_strip_suffix(&string, ":suffix"), None);
    }

    #[test]
    fn test_os_str_strip_suffix_when_suffix_is_empty() {
        let string = String::from("value");

        assert_eq!(os_str_strip_suffix(&string, ""), Some(OsStr::new("value")));
    }

    #[test]
    fn test_os_str_starts_with_when_value_is_the_same() {
        assert!(os_str_starts_with("value1", "value1"));
    }

    #[test]
    fn test_os_str_starts_with_when_it_starts_with() {
        assert!(os_str_starts_with("value1 extra text", "value1"));
    }

    #[test]
    fn test_os_str_starts_with_when_it_does_not_starts_with() {
        assert!(!os_str_starts_with("value1 extra text", "value2"));
    }

    #[test]
    fn test_os_str_starts_with_when_the_value_is_too_large() {
        assert!(!os_str_starts_with("value1", "value1 extra text"));
    }

    #[test]
    fn test_os_str_trim_without_anything_to_trim() {
        assert_eq!(os_str_trim(&"a random string"), "a random string");
    }

    #[test]
    fn test_os_str_trim_with_whitespaces_at_the_beginning() {
        assert_eq!(os_str_trim(&" \t\na random string"), "a random string");
    }

    #[test]
    fn test_os_str_trim_with_whitespaces_at_the_end() {
        assert_eq!(os_str_trim(&"a random string \t\n"), "a random string");
    }

    #[test]
    fn test_os_str_trim_with_whitespaces_at_the_beginning_and_end() {
        assert_eq!(os_str_trim(&" \t\na random string \t\n"), "a random string");
    }

    #[test]
    fn test_os_str_trim_with_only_whitespaces() {
        assert_eq!(os_str_trim(&" \t\n"), "");
    }
}
