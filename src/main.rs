use std::env;
use std::process::ExitCode;

use calando::{Command, help, resolve_command};

fn main() -> ExitCode {
    let command = match resolve_command(env::args_os()) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("{error}\n\n{}", help());
            return ExitCode::from(2);
        }
    };
    match command {
        Command::Daemon(arguments) => {
            return ExitCode::from(calando::supervisor::run_daemon(&arguments) as u8);
        }
        Command::Gate(arguments) => {
            return ExitCode::from(calando::gate::run_gate(&arguments) as u8);
        }
        Command::Notify(arguments) => {
            return ExitCode::from(calando::notify::run_notify(&arguments) as u8);
        }
        Command::Status(arguments) => {
            return ExitCode::from(calando::status::run_status(&arguments) as u8);
        }
        Command::Control(arguments) => {
            return ExitCode::from(calando::control::run_control(&arguments) as u8);
        }
        Command::Integration(arguments) => {
            return ExitCode::from(calando::integration::run_integration(&arguments) as u8);
        }
        Command::AppResumeGuard(arguments) => {
            return ExitCode::from(calando::app_guard::run_resume_guard(&arguments) as u8);
        }
        Command::Help => println!("{}", help()),
        Command::Version => println!("calando {}", env!("CARGO_PKG_VERSION")),
    }
    ExitCode::SUCCESS
}
