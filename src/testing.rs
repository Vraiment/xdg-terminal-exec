//! Supporting functions for testing
//!
//! These structs and functions are not supposed to be used for the runtime
//! execution of `xdg-terminal-exec`.

use std::{
    collections::HashMap,
    env,
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
};

use crate::env_var;

static ENV_LOCK: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

/// Executes the given `function` using the environment defined by `env`
/// preserving the original environment using a lock.
///
/// The locking mechanism is important because tests will run in parallel and
/// which can cause for one test to populate other test's environment. Be
/// aware that this means the tests that use [`with_env`] will be serialized.
///
/// ```
/// use std::{
///     collections::HashMap,
///     env::{self, VarError},
/// };
///
/// use xdg_terminal_exec::testing::with_env;
///
/// unsafe {
///     env::set_var("MY_CUSTOM_ENV_VAR_SET", "value");
///     env::remove_var("MY_CUSTOM_ENV_VAR_UNSET");
/// };
///
/// // Validate the environment is as expected
/// assert_eq!(env::var("MY_CUSTOM_ENV_VAR_SET"), Ok(String::from("value")));
/// assert_eq!(env::var("MY_CUSTOM_ENV_VAR_UNSET"), Err(VarError::NotPresent));
///
/// with_env(
///     // Flip the values from within the function.
///     HashMap::from([
///         ("MY_CUSTOM_ENV_VAR_SET", None),
///         ("MY_CUSTOM_ENV_VAR_UNSET", Some("value")),
///     ]),
///     &mut || {
///         // The values will change within the function from the given hash.
///         assert_eq!(env::var("MY_CUSTOM_ENV_VAR_SET"), Err(VarError::NotPresent));
///         assert_eq!(env::var("MY_CUSTOM_ENV_VAR_UNSET"), Ok(String::from("value")));
///     },
/// );
///
/// // Validate the environment has been restored
/// assert_eq!(env::var("MY_CUSTOM_ENV_VAR_SET"), Ok(String::from("value")));
/// assert_eq!(env::var("MY_CUSTOM_ENV_VAR_UNSET"), Err(VarError::NotPresent));
/// ```
pub fn with_env<K, V, F, R>(env: HashMap<K, Option<V>>, function: &mut F) -> R
where
    K: AsRef<OsStr> + Eq + std::hash::Hash + Copy,
    V: AsRef<OsStr>,
    F: FnMut() -> R,
{
    let _mutex_guard = ENV_LOCK.lock().unwrap();
    let mut original_env: HashMap<K, Option<OsString>> = HashMap::new();

    env.keys().for_each(|name: &K| {
        original_env.insert(name.clone(), env_var(name));
    });

    overwrite_env(env);

    let result = function();

    overwrite_env(original_env);

    result
}

fn overwrite_env<K, V>(env: HashMap<K, Option<V>>)
where
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    env.iter().for_each(|(name, value)| {
        match value {
            Some(value) => unsafe {
                env::set_var(name, value);
            },
            None => unsafe {
                env::remove_var(name);
            },
        };
    });
}

/// Mimics the [`mktemp`](https://www.man7.org/linux/man-pages/man1/mktemp.1.html)
/// command's behavior plus ensuring the file gets deleted after dropping.
///
/// ```
/// use std::env;
/// use xdg_terminal_exec::testing::TempFile;
///
/// let temp_file = TempFile::with_prefix("a-prefix").unwrap();
/// assert!(temp_file.name().to_str().unwrap().starts_with("a-prefix."));
///
/// let temp_file_path = temp_file.path().to_owned();
/// assert!(temp_file_path.starts_with(env::temp_dir()));
/// assert!(temp_file_path.exists());
///
/// drop(temp_file);
///
/// assert!(!temp_file_path.exists());
/// ```
pub struct TempFile {
    path: PathBuf,
}

impl TempFile {
    /// Creates a new [`TempFile`] using `tmp` as a preffix of the file.
    pub fn new() -> io::Result<Self> {
        Self::with_prefix("tmp")
    }

    /// Creates a new [`TempFile`] using the given `prefix` for the name.
    pub fn with_prefix(prefix: &str) -> io::Result<Self> {
        let temp_dir = env::temp_dir();
        let mut path: PathBuf;
        const SUFFIX_LENGTH: usize = 10;

        loop {
            let suffix = Self::generate_random_alphanumeric_string(SUFFIX_LENGTH)?;

            let name = format!("{prefix}.{suffix}");
            path = temp_dir.join(name);

            if !path.exists() {
                File::create(&path)?;
                break;
            }
        }

        Ok(TempFile { path: path })
    }

    /// Returns the full [`Path`] to the underlying temp file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the filename of the temporary path.
    pub fn name(&self) -> &OsStr {
        self.path.file_name().unwrap()
    }

    fn generate_random_alphanumeric_string(length: usize) -> io::Result<String> {
        let mut dev_urandom = File::open("/dev/urandom")?;
        let mut buffer = vec![0; length];

        dev_urandom.read_exact(&mut buffer)?;

        let bytes: Vec<char> = buffer
            .iter()
            .map(|byte| byte_to_alphanumeric_ascii(*byte))
            .collect();

        Ok(bytes.iter().collect::<String>())
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.path());
    }
}

fn byte_to_alphanumeric_ascii(byte: u8) -> char {
    const ASCII_0: u8 = 48;
    const ASCII_UPPERCASE_A: u8 = 65;
    const ASCII_LOWERCASE_A: u8 = 97;

    let byte = byte % 62;
    let result_as_byte = match byte {
        0..=9 => ASCII_0 + byte,
        10..=35 => ASCII_UPPERCASE_A + byte - 10, // substract 10 to start from 0
        36..=61 => ASCII_LOWERCASE_A + byte - 36, // substract 36 to start from 0
        value => {
            panic!("{byte} % 62 should always return less than 61 but returned {value} instead")
        }
    };

    char::from(result_as_byte)
}

#[cfg(test)]
mod test {
    use std::{env, ffi::OsString, fs};

    use super::*;

    #[test]
    fn test_byte_to_alphanumeric_ascii() {
        // Test just the ranges to validate the logic
        assert_eq!('0', char::from(byte_to_alphanumeric_ascii(0)));
        assert_eq!('9', char::from(byte_to_alphanumeric_ascii(9)));
        assert_eq!('A', char::from(byte_to_alphanumeric_ascii(10)));
        assert_eq!('Z', char::from(byte_to_alphanumeric_ascii(35)));
        assert_eq!('a', char::from(byte_to_alphanumeric_ascii(36)));
        assert_eq!('z', char::from(byte_to_alphanumeric_ascii(61)));

        // Test ranges outside the "base" ranges
        assert_eq!('0', char::from(byte_to_alphanumeric_ascii(62)));
        assert_eq!('0', char::from(byte_to_alphanumeric_ascii(186)));
        assert_eq!('0', char::from(byte_to_alphanumeric_ascii(248)));
    }

    #[test]
    fn test_temp_file_with_default_prefix() {
        let temp_file = TempFile::new().unwrap();
        let file_name = temp_file.path().file_name().unwrap();
        let file_name = file_name.to_str().unwrap();

        assert!(
            file_name.starts_with("tmp"),
            "the path is {}",
            temp_file.path().display()
        );
    }

    #[test]
    fn test_temp_file_has_prefix() {
        let temp_file = TempFile::with_prefix("prefix").unwrap();
        let file_name = temp_file.name().to_str().unwrap();

        assert!(
            file_name.starts_with("prefix"),
            "the path is {}",
            temp_file.path().display()
        );
    }

    fn read_temp_dir() -> Vec<OsString> {
        fs::read_dir(env::temp_dir())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect()
    }

    #[test]
    fn test_temp_file_creates_a_file_that_does_not_exist() {
        let original_temp_dir_contents = read_temp_dir();
        let temp_file = TempFile::new().unwrap();
        let file_name = temp_file.name().to_os_string();

        assert!(
            !original_temp_dir_contents.contains(&file_name),
            "File {} shouldn't exist before the variable is instantiated",
            temp_file.path().display(),
        );

        assert!(
            read_temp_dir().contains(&file_name),
            "File {} should exist after the variable is instantiated",
            temp_file.path().display()
        );
    }

    #[test]
    fn test_temp_file_gets_deleted_after_dropping() {
        let temp_file = TempFile::with_prefix("prefix").unwrap();
        let temp_file_path = temp_file.path().to_owned();
        let file_name = temp_file.name().to_os_string();

        assert!(
            read_temp_dir().contains(&file_name),
            "File {} shouldn't exist before the variable is instantiated",
            temp_file_path.display()
        );

        drop(temp_file);

        assert!(
            !read_temp_dir().contains(&file_name),
            "File {} shouldn't exist after the variable is dropped",
            temp_file_path.display()
        );
    }
}
