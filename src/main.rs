use std::{env, path::Path, process::exit};

fn main() {
    // Skip the program name
    let args: Vec<String> = env::args().skip(0).collect();

    for arg in &args {
        match arg.as_str() {
            "--" => break,
            "-h" | "--help" => print_help_and_exit(),
            _ => {}
        }
    }
}

fn print_help_and_exit() -> ! {
    use std::ffi::OsStr;

    let self_name = env::args()
        .next()
        .as_ref()
        .map(Path::new)
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
        .map(String::from)
        .unwrap_or(String::from(env!("CARGO_PKG_NAME")));

    println!(
        r#"{self_name} - Shell-based default terminal launcher.

Implementation of the proposed Default Terminal Specification.

Usage:
  {self_name} [options] [--] [command [arguments ...]]

Launches given command in default terminal, or launches default terminal.

Options for modifying terminal behavior (if supported by terminals's
entry):

  --app-id=app-id    set app-id (Wayland) or window class (X11)
  --title=title      set tile of terminal.
  --dir=workdir      set workdir of terminal.
  --hold             instruct terminal to hold after command ends.

Options for printing data instead of executing terminal:

  --print-id
      print selected Desktop Entry ID. Action is appended delimited
      by ":".

  --print-path
      print path to selected Desktop Entry. Action is appended
      delimited by ":".

  --print-content
      print content of selected Desktop Entry. Conflicts with --print-cmd.

  --print-cmd[=printf_sequence]
      print resulting command line, delimited by given printf sequence,
      "\n" by default. If sequence is "\n", output is also terminated with
      a newline.

  --print-delimiter=printf_sequence
      printf sequence to be used as the delimiter between multiple requested
      print statements, "\n" by default. If sequence is "\n", output is
      also terminated with a newline.

Configuration:

Preferred terminals are configured by listing their Desktop Entry IDs
in config files named "[${{desktop}}-]xdg-terminals.list" placed in XDG Config
hierarchy. Where "${{desktop}}" is a lowercased string that is matched
(case-insensitively) against items of "${{XDG_CURRENT_DESKTOP}}".

See "man {self_name}" for more details."#
    );

    exit(0)
}
