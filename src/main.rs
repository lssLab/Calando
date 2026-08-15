use std::env;
use std::process::ExitCode;

use memory_supervisor::{Command, help, resolve_command};

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
            return ExitCode::from(memory_supervisor::supervisor::run_daemon(&arguments) as u8);
        }
        Command::Gate(arguments) => {
            return ExitCode::from(memory_supervisor::gate::run_gate(&arguments) as u8);
        }
        Command::Notify(arguments) => {
            return ExitCode::from(memory_supervisor::notify::run_notify(&arguments) as u8);
        }
        Command::Status(arguments) => {
            return ExitCode::from(memory_supervisor::status::run_status(&arguments) as u8);
        }
        Command::Control(arguments) => {
            return ExitCode::from(memory_supervisor::control::run_control(&arguments) as u8);
        }
        Command::Integration(arguments) => {
            return ExitCode::from(
                memory_supervisor::integration::run_integration(&arguments) as u8
            );
        }
        Command::AppResumeGuard(arguments) => {
            return ExitCode::from(memory_supervisor::app_guard::run_resume_guard(&arguments) as u8);
        }
        Command::Help => println!("{}", help()),
        Command::Version => println!("memory-supervisor {}", env!("CARGO_PKG_VERSION")),
    }
    ExitCode::SUCCESS
}
