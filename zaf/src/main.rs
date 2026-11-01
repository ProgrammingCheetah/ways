//! The `zaf` binary: a thin shell over the library's command line.

use std::process::ExitCode;

fn main() -> ExitCode {
    let code = zaf::cli::run(std::env::args_os());
    ExitCode::from(code)
}
