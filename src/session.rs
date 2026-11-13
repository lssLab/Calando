//! Controls a user sets from inside one session with `/calando` (Codex: `$calando`).
//! They apply to that session only; the terminal `calando` command is environment-wide.

use serde_json::{Map, Value, json};
use std::fs;
use std::path::Path;

use crate::storage::write_atomic_json;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SessionControl {
    pub off: bool,
    pub cap_mb: Option<f64>,
}

fn sessions_path(directory: &Path) -> std::path::PathBuf {
    directory.join("sessions.json")
}

fn key(provider: &str, session: &str) -> String {
    format!("{provider}:{session}")
}

fn load_all(directory: &Path) -> Map<String, Value> {
    fs::read(sessions_path(directory))
        .ok()
        .and_then(|source| serde_json::from_slice::<Value>(&source).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn parse_control(value: &Value) -> SessionControl {
    SessionControl {
        off: value.get("off").and_then(Value::as_bool) == Some(true),
        cap_mb: value
            .get("cap_mb")
            .and_then(Value::as_f64)
            .filter(|mb| mb.is_finite() && *mb >= 1.0),
    }
}

pub fn control(directory: &Path, provider: &str, session: &str) -> SessionControl {
    load_all(directory)
        .get(&key(provider, session))
        .map(parse_control)
        .unwrap_or_default()
}

/// Every session that has a setting, as `(provider, session, control)`.
pub fn all_controls(directory: &Path) -> Vec<(String, String, SessionControl)> {
    load_all(directory)
        .iter()
        .filter_map(|(key, value)| {
            let (provider, session) = key.split_once(':')?;
            Some((
                provider.to_owned(),
                session.to_owned(),
                parse_control(value),
            ))
        })
        .collect()
}

fn save(directory: &Path, provider: &str, session: &str, control: &SessionControl) -> bool {
    let mut all = load_all(directory);
    let key = key(provider, session);
    if *control == SessionControl::default() {
        all.remove(&key);
    } else {
        let mut entry = Map::new();
        if control.off {
            entry.insert("off".to_owned(), Value::Bool(true));
        }
        if let Some(cap_mb) = control.cap_mb {
            entry.insert("cap_mb".to_owned(), json!(cap_mb));
        }
        all.insert(key, Value::Object(entry));
    }
    write_atomic_json(&sessions_path(directory), &Value::Object(all), 0o600, true).is_ok()
}

/// The words after `/calando` or `$calando`, or `None` when the prompt is not a Calando command.
pub fn parse_command(prompt: &str) -> Option<Vec<&str>> {
    let prompt = prompt.trim();
    let rest = prompt
        .strip_prefix("/calando")
        .or_else(|| prompt.strip_prefix("$calando"))?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(rest.split_whitespace().collect())
}

/// Memory of the session's own process tree (its lead and everything under it), from state.
pub fn session_tree_mb(state: &Value, provider: &str, session: &str) -> Option<f64> {
    let lead_pid = state
        .get("logical_agents")?
        .as_object()?
        .values()
        .find(|agent| {
            agent.get("provider").and_then(Value::as_str) == Some(provider)
                && agent.get("session_id").and_then(Value::as_str) == Some(session)
                && agent.get("role").and_then(Value::as_str) == Some("lead")
        })?
        .get("process_pid")?
        .as_u64()?;
    state
        .get("tracked_processes")?
        .as_array()?
        .iter()
        .find(|process| process.get("pid").and_then(Value::as_u64) == Some(lead_pid))?
        .get("tree_rss_mb")?
        .as_f64()
}

/// The session is within 10% of its cap, so new heavy work in it is held.
pub fn near_cap(state: &Value, control: &SessionControl, provider: &str, session: &str) -> bool {
    control.cap_mb.is_some_and(|cap| {
        session_tree_mb(state, provider, session).is_some_and(|used| used >= cap * 0.9)
    })
}

fn gb(mb: f64) -> String {
    format!("{:.1} GB", mb / 1024.0)
}

fn help(prefix: &str) -> String {
    format!(
        "Calando for this session\n\
         {prefix} on        Protect this session again\n\
         {prefix} off       Stop protecting this session (other sessions stay protected)\n\
         {prefix} status    What Calando sees for this session\n\
         {prefix} cap <GB>  Advanced: hard memory cap for this session (asks to confirm)\n\
         {prefix} cap off   Remove this session's cap\n\
         For the whole environment, run `calando help` in a terminal."
    )
}

/// Handles a session command and returns the reply shown to the user.
/// `status_lines` is what the session's agent currently sees from Calando.
pub fn handle(
    directory: &Path,
    provider: &str,
    session: &str,
    words: &[&str],
    status_lines: &str,
    state: &Value,
    locked: bool,
) -> String {
    let prefix = if provider == "codex" {
        "$calando"
    } else {
        "/calando"
    };
    let mut control = control(directory, provider, session);
    let saved = |control: &SessionControl, reply: String| {
        if save(directory, provider, session, control) {
            reply
        } else {
            "Calando could not save the setting; nothing changed.".to_owned()
        }
    };
    match words {
        ["on"] => {
            control.off = false;
            saved(&control, "Calando on for this session.".to_owned())
        }
        ["off"] if locked => {
            "Calando is locked on by an administrator; it cannot be turned off here.".to_owned()
        }
        ["off"] => {
            control.off = true;
            saved(
                &control,
                format!(
                    "Calando off for this session only. Other sessions stay protected. `{prefix} on` turns it back on."
                ),
            )
        }
        [] | ["status"] => {
            let cap = match control.cap_mb {
                Some(cap) => format!(
                    "{} (uses {})",
                    gb(cap),
                    session_tree_mb(state, provider, session)
                        .map(gb)
                        .unwrap_or_else(|| "unknown".to_owned())
                ),
                None => "off".to_owned(),
            };
            format!(
                "Calando for this session: {}\n{status_lines}\nSession cap: {cap}",
                if control.off { "off" } else { "on" }
            )
        }
        ["cap", "off"] => {
            control.cap_mb = None;
            saved(&control, "Session cap removed.".to_owned())
        }
        ["cap", amount] | ["cap", amount, "yes"] => {
            let Some(mb) = amount
                .trim_end_matches(|character: char| character.is_ascii_alphabetic())
                .parse::<f64>()
                .ok()
                .map(|gb| gb * 1024.0)
                .filter(|mb| mb.is_finite() && *mb >= 256.0)
            else {
                return format!("Give the cap in GB, at least 0.25: `{prefix} cap 4`");
            };
            if words.len() == 2 {
                return format!(
                    "Session memory cap (advanced)\n\
                     Not recommended for normal use: Calando already protects memory automatically.\n\
                     Use it only when you need a strict limit, such as research or benchmark runs.\n\
                     Effect: near {} this session's new heavy work is held; above it, one growing subagent is paused.\n\
                     Limit: a pause cannot take back memory already used.\n\
                     To apply: {prefix} cap {amount} yes",
                    gb(mb)
                );
            }
            control.cap_mb = Some(mb);
            saved(&control, format!("Session cap set to {}.", gb(mb)))
        }
        ["help"] => help(prefix),
        _ => format!("Unknown command.\n{}", help(prefix)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_commands_change_only_their_own_session() {
        let directory =
            std::env::temp_dir().join(format!("calando-session-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        assert_eq!(parse_command(" /calando off "), Some(vec!["off"]));
        assert_eq!(parse_command("$calando"), Some(vec![]));
        assert_eq!(parse_command("/calandoff"), None);
        assert_eq!(parse_command("please /calando off"), None);

        let state = json!({});
        handle(&directory, "claude", "a", &["off"], "", &state, false);
        assert!(control(&directory, "claude", "a").off);
        assert!(!control(&directory, "claude", "b").off);
        assert!(handle(&directory, "claude", "b", &["off"], "", &state, true).contains("locked"));
        assert!(!control(&directory, "claude", "b").off);

        let ask = handle(&directory, "codex", "c", &["cap", "4"], "", &state, false);
        assert!(ask.contains("Not recommended") && ask.contains("$calando cap 4 yes"));
        assert_eq!(control(&directory, "codex", "c").cap_mb, None);
        handle(
            &directory,
            "codex",
            "c",
            &["cap", "4", "yes"],
            "",
            &state,
            false,
        );
        assert_eq!(control(&directory, "codex", "c").cap_mb, Some(4096.0));

        handle(&directory, "claude", "a", &["on"], "", &state, false);
        assert_eq!(all_controls(&directory).len(), 1);
        fs::remove_dir_all(directory).unwrap();
    }
}
