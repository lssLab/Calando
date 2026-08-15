use std::ffi::{OsStr, OsString};
use std::path::Path;

pub mod app_guard;
pub mod codex_app;
pub mod config;
pub mod containment;
pub mod control;
pub mod events;
pub mod gate;
pub mod integration;
pub mod notify;
pub mod platform;
pub mod policy;
pub mod runtime;
pub mod status;
pub mod storage;
pub mod supervisor;
pub mod terminal;
pub mod topology;

#[derive(Debug, Eq, PartialEq)]
pub enum Command {
    Daemon(Vec<OsString>),
    Gate(Vec<OsString>),
    Status(Vec<OsString>),
    Control(Vec<OsString>),
    Notify(Vec<OsString>),
    Integration(Vec<OsString>),
    AppResumeGuard(Vec<OsString>),
    Help,
    Version,
}

pub fn resolve_command<I>(arguments: I) -> Result<Command, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut arguments = arguments.into_iter();
    let executable = arguments.next().unwrap_or_default();
    let remaining: Vec<_> = arguments.collect();
    let alias = Path::new(&executable)
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or_default();
    if alias == "memory-status" {
        return Ok(Command::Status(remaining));
    }

    let Some(first) = remaining.first() else {
        return Ok(Command::Help);
    };
    let name = first.to_str().map(str::to_owned);
    let rest: Vec<OsString> = remaining[1..].to_vec();
    match name.as_deref() {
        Some("daemon") => Ok(Command::Daemon(rest)),
        Some("gate") => Ok(Command::Gate(rest)),
        Some("status") => Ok(Command::Status(rest)),
        Some("control") => Ok(Command::Control(rest)),
        Some(
            "resume" | "terminate" | "kill" | "budget" | "hard-cap" | "notifications" | "update"
            | "uninstall" | "on" | "off",
        ) => Ok(Command::Control(remaining)),
        Some("notify") => Ok(Command::Notify(rest)),
        Some("integration") => Ok(Command::Integration(rest)),
        Some("app-resume-guard") => Ok(Command::AppResumeGuard(rest)),
        Some("--help" | "-h" | "help") => Ok(Command::Help),
        Some("--version" | "-V") => Ok(Command::Version),
        Some(value) => Err(format!("unknown command: {value}")),
        None => Err("command must be valid Unicode".to_owned()),
    }
}

pub fn help() -> &'static str {
    "Memory Supervisor\n\nUSAGE:\n  memory-supervisor <on|off|status|resume|terminate|kill|budget|hard-cap|notifications|update|uninstall> [arguments]\n  memory-supervisor <daemon|gate|notify|integration> [arguments]\n  memory-status [arguments]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_names_share_one_binary() {
        assert_eq!(
            resolve_command(["memory-status", "--json"].map(OsString::from)).unwrap(),
            Command::Status(vec![OsString::from("--json")])
        );
        assert_eq!(
            resolve_command(["memory-supervisor", "resume", "42"].map(OsString::from)).unwrap(),
            Command::Control(vec![OsString::from("resume"), OsString::from("42")])
        );
        assert_eq!(
            resolve_command(["memory-supervisor", "control", "resume", "42"].map(OsString::from))
                .unwrap(),
            Command::Control(vec![OsString::from("resume"), OsString::from("42")])
        );
        assert_eq!(
            resolve_command(["memory-supervisor", "hard-cap", "show"].map(OsString::from)).unwrap(),
            Command::Control(vec![OsString::from("hard-cap"), OsString::from("show")])
        );
        assert_eq!(
            resolve_command(["memory-supervisor", "off"].map(OsString::from)).unwrap(),
            Command::Control(vec![OsString::from("off")])
        );
        assert_eq!(
            resolve_command(["memory-supervisor", "on"].map(OsString::from)).unwrap(),
            Command::Control(vec![OsString::from("on")])
        );
        assert_eq!(
            resolve_command(["memory-supervisor", "gate", "PreToolUse"].map(OsString::from))
                .unwrap(),
            Command::Gate(vec![OsString::from("PreToolUse")])
        );
        assert_eq!(
            resolve_command(
                [
                    "memory-supervisor",
                    "app-resume-guard",
                    "42",
                    "42:start",
                    "incident-1",
                    "1000",
                    "/tmp/runtime.json",
                    "linux",
                    "42",
                    "shared-host",
                    "/tmp/app-guards/42-1",
                ]
                .map(OsString::from)
            )
            .unwrap(),
            Command::AppResumeGuard(
                [
                    "42",
                    "42:start",
                    "incident-1",
                    "1000",
                    "/tmp/runtime.json",
                    "linux",
                    "42",
                    "shared-host",
                    "/tmp/app-guards/42-1",
                ]
                .map(OsString::from)
                .to_vec()
            )
        );
    }
}
