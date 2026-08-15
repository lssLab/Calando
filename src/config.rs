use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::storage::write_atomic_text;

pub const PREFIX: &str = "MEMORY_SUPERVISOR_";
pub(crate) const PRETOOL_HOLD_DEFAULT_S: f64 = 12.0;
pub const MANDATORY_NOTIFICATION_CHANNELS: [&str; 2] = ["hook", "terminal"];
pub const CONFIGURABLE_NOTIFICATION_CHANNELS: [&str; 3] = ["os", "discord", "telegram"];
pub const NOTIFICATION_CONFIG_KEYS: [&str; 7] = [
    "MEMORY_SUPERVISOR_NOTIFICATION_CHANNELS",
    "MEMORY_SUPERVISOR_DISCORD_WEBHOOK",
    "MEMORY_SUPERVISOR_DISCORD_BOT_TOKEN",
    "MEMORY_SUPERVISOR_DISCORD_CHANNEL_ID",
    "MEMORY_SUPERVISOR_DISCORD_OWNER_USER_ID",
    "MEMORY_SUPERVISOR_TELEGRAM_BOT_TOKEN",
    "MEMORY_SUPERVISOR_TELEGRAM_CHAT_ID",
];

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn power_off_path() -> PathBuf {
    home_dir().join(".memory-supervisor").join("power-off")
}

pub fn power_is_off() -> bool {
    fs::symlink_metadata(power_off_path()).is_ok()
}

pub fn expand_user(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path == Path::new("~") {
        return home_dir();
    }
    if let Ok(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    path.to_path_buf()
}

pub fn config_path() -> PathBuf {
    env::var_os("MEMORY_SUPERVISOR_CONFIG")
        .map(|path| expand_user(PathBuf::from(path)))
        .unwrap_or_else(|| {
            home_dir()
                .join(".config")
                .join("memory-supervisor")
                .join("config.json")
        })
}

pub fn notification_config_path() -> PathBuf {
    env::var_os("MEMORY_SUPERVISOR_NOTIFICATION_CONFIG")
        .map(|path| expand_user(PathBuf::from(path)))
        .unwrap_or_else(|| {
            home_dir()
                .join(".config")
                .join("memory-supervisor")
                .join("notifications.conf")
        })
}

pub fn state_dir() -> PathBuf {
    state_dir_from(&home_dir(), env::var_os("MEMORY_SUPERVISOR_DIR"))
}

fn state_dir_from(home: &Path, configured: Option<std::ffi::OsString>) -> PathBuf {
    if let Some(path) = configured {
        return expand_user(path);
    }
    let pointer = home.join(".memory-supervisor").join("state-dir");
    if let Ok(value) = fs::read_to_string(pointer) {
        let value = value.trim();
        if !value.is_empty() {
            return expand_user(value);
        }
    }
    home.join(".cache").join("memory-supervisor")
}

#[derive(Debug, Default)]
pub struct Config {
    values: Map<String, Value>,
    load_error: Option<String>,
    validation_errors: BTreeMap<String, String>,
}

impl Config {
    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(source) => match serde_json::from_str::<Value>(&source) {
                Ok(Value::Object(values)) => Self {
                    values,
                    ..Self::default()
                },
                Ok(_) => Self {
                    load_error: Some(format!(
                        "{}: top-level JSON value must be an object",
                        path.display()
                    )),
                    ..Self::default()
                },
                Err(error) => Self {
                    load_error: Some(format!("{}: JSON error: {error}", path.display())),
                    ..Self::default()
                },
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(error) => Self {
                load_error: Some(format!("{}: IO error: {error}", path.display())),
                ..Self::default()
            },
        }
    }

    pub fn current() -> Self {
        Self::load(&config_path())
    }

    pub fn values(&self) -> &Map<String, Value> {
        &self.values
    }

    pub fn setting(&self, name: &str) -> Option<Value> {
        env::var(name)
            .ok()
            .map(Value::String)
            .or_else(|| self.values.get(name).cloned())
    }

    pub fn validated_number(
        &mut self,
        name: &str,
        default: f64,
        minimum: Option<f64>,
        maximum: Option<f64>,
    ) -> f64 {
        let Some(setting) = self.setting(name) else {
            self.validation_errors.remove(name);
            return default;
        };
        let value = match setting {
            Value::Number(number) => number.as_f64(),
            Value::String(value) => value.parse().ok(),
            Value::Bool(value) => Some(if value { 1.0 } else { 0.0 }),
            _ => None,
        };
        let error = match value {
            None => Some("must be a number".to_owned()),
            Some(value) if !value.is_finite() => Some("must be finite".to_owned()),
            Some(value) if minimum.is_some_and(|minimum| value < minimum) => {
                Some(format!("must be >= {}", minimum.unwrap()))
            }
            Some(value) if maximum.is_some_and(|maximum| value > maximum) => {
                Some(format!("must be <= {}", maximum.unwrap()))
            }
            Some(value) => {
                self.validation_errors.remove(name);
                return value;
            }
        };
        self.validation_errors.insert(
            name.to_owned(),
            format!("{name}: {}; using {default}", error.unwrap()),
        );
        default
    }

    pub fn validated_choice(&mut self, name: &str, default: &str, choices: &[&str]) -> String {
        let value = self
            .setting(name)
            .map(|value| match value {
                Value::String(value) => value,
                value => value.to_string(),
            })
            .unwrap_or_else(|| default.to_owned())
            .trim()
            .to_lowercase();
        if choices.contains(&value.as_str()) {
            self.validation_errors.remove(name);
            value
        } else {
            self.validation_errors.insert(
                name.to_owned(),
                format!(
                    "{name}: must be one of {}; using {default}",
                    choices.join(", ")
                ),
            );
            default.to_owned()
        }
    }

    pub fn configuration_error(&self) -> Option<String> {
        let mut errors = Vec::new();
        if let Some(error) = &self.load_error {
            errors.push(error.clone());
        }
        errors.extend(self.validation_errors.values().cloned());
        (!errors.is_empty()).then(|| errors.join("; "))
    }

    pub fn has_validation_error(&self, name: &str) -> bool {
        self.validation_errors.contains_key(name)
    }

    pub fn record_validation_error(&mut self, name: &str, message: impl Into<String>) {
        self.validation_errors
            .insert(name.to_owned(), message.into());
    }

    pub fn clear_validation_error(&mut self, name: &str) {
        self.validation_errors.remove(name);
    }
}

pub fn load_notification_file(path: &Path) -> BTreeMap<String, String> {
    let source = fs::read_to_string(path).unwrap_or_default();
    source
        .lines()
        .filter_map(|raw| {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            NOTIFICATION_CONFIG_KEYS.contains(&key).then(|| {
                (
                    key.to_owned(),
                    value.trim().trim_matches(['\'', '"']).to_owned(),
                )
            })
        })
        .collect()
}

pub fn load_notification_config(path: &Path) -> BTreeMap<String, String> {
    let mut values = load_notification_file(path);
    for key in NOTIFICATION_CONFIG_KEYS {
        if let Ok(value) = env::var(key)
            && !value.is_empty()
        {
            values.insert(key.to_owned(), value);
        }
    }
    values
}

pub fn save_notification_config(path: &Path, values: &BTreeMap<String, String>) -> io::Result<()> {
    let mut lines = vec![
        "# Managed by `memory-supervisor notifications`; do not commit this file.".to_owned(),
        "# Changes are read automatically at the next notification event.".to_owned(),
    ];
    for key in NOTIFICATION_CONFIG_KEYS {
        let value = values
            .get(key)
            .map(|value| value.trim())
            .unwrap_or_default();
        if value.contains(['\n', '\r']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{key} must be one line"),
            ));
        }
        if !value.is_empty() {
            lines.push(format!("{key}={value}"));
        }
    }
    write_atomic_text(path, &(lines.join("\n") + "\n"), 0o600)
}

pub fn notification_channels(values: &BTreeMap<String, String>) -> BTreeSet<String> {
    let raw = values
        .get("MEMORY_SUPERVISOR_NOTIFICATION_CHANNELS")
        .map(String::as_str)
        .unwrap_or("all")
        .trim()
        .to_lowercase();
    let mut channels: BTreeSet<String> = MANDATORY_NOTIFICATION_CHANNELS
        .into_iter()
        .map(str::to_owned)
        .collect();
    if raw.is_empty() || raw == "all" {
        channels.extend(
            CONFIGURABLE_NOTIFICATION_CHANNELS
                .into_iter()
                .map(str::to_owned),
        );
    } else if raw != "none" && raw != "off" {
        for item in raw.split(',').map(str::trim) {
            if CONFIGURABLE_NOTIFICATION_CHANNELS.contains(&item) {
                channels.insert(item.to_owned());
            }
        }
    }
    channels
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{LazyLock, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn temporary_directory() -> PathBuf {
        let path = env::temp_dir().join(format!(
            "memory-supervisor-config-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn environment_precedence_validation_and_mandatory_channels_match_python() {
        let _guard = ENV_LOCK.lock().unwrap();
        let root = temporary_directory();
        let path = root.join("config.json");
        fs::write(&path, r#"{"MEMORY_SUPERVISOR_TICK_S":2.5}"#).unwrap();
        let mut config = Config::load(&path);
        assert_eq!(
            config.validated_number("MEMORY_SUPERVISOR_TICK_S", 1.0, Some(0.1), None),
            2.5
        );

        // SAFETY: the test serializes all environment mutation with ENV_LOCK.
        unsafe { env::set_var("MEMORY_SUPERVISOR_TICK_S", "4") };
        assert_eq!(
            config.validated_number("MEMORY_SUPERVISOR_TICK_S", 1.0, Some(0.1), None),
            4.0
        );
        // SAFETY: the test serializes all environment mutation with ENV_LOCK.
        unsafe { env::set_var("MEMORY_SUPERVISOR_TICK_S", "nan") };
        assert_eq!(
            config.validated_number("MEMORY_SUPERVISOR_TICK_S", 1.0, Some(0.1), None),
            1.0
        );
        assert!(
            config
                .configuration_error()
                .unwrap()
                .contains("MEMORY_SUPERVISOR_TICK_S")
        );
        // SAFETY: the test serializes all environment mutation with ENV_LOCK.
        unsafe { env::remove_var("MEMORY_SUPERVISOR_TICK_S") };

        let values = BTreeMap::from([(
            "MEMORY_SUPERVISOR_NOTIFICATION_CHANNELS".to_owned(),
            "none".to_owned(),
        )]);
        assert_eq!(
            notification_channels(&values),
            BTreeSet::from(["hook".to_owned(), "terminal".to_owned()])
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_configuration_pointer_and_notification_roundtrip_are_safe() {
        let root = temporary_directory();
        let malformed = root.join("malformed.json");
        fs::write(&malformed, "not-json").unwrap();
        let config = Config::load(&malformed);
        assert!(config.values().is_empty());
        assert!(
            config
                .configuration_error()
                .is_some_and(|error| error.contains("JSON error"))
        );

        let non_object = root.join("array.json");
        fs::write(&non_object, "[]").unwrap();
        assert!(
            Config::load(&non_object)
                .configuration_error()
                .is_some_and(|error| error.contains("top-level JSON value must be an object"))
        );

        let pointer = root.join(".memory-supervisor/state-dir");
        fs::create_dir_all(pointer.parent().unwrap()).unwrap();
        fs::write(&pointer, "/tmp/memory-supervisor-custom-state\n").unwrap();
        assert_eq!(
            state_dir_from(&root, None),
            PathBuf::from("/tmp/memory-supervisor-custom-state")
        );
        assert_eq!(
            state_dir_from(&root, Some(root.join("override").into_os_string())),
            root.join("override")
        );

        let notifications = root.join("notifications.conf");
        let values = BTreeMap::from([
            (
                "MEMORY_SUPERVISOR_NOTIFICATION_CHANNELS".to_owned(),
                "os,telegram".to_owned(),
            ),
            (
                "MEMORY_SUPERVISOR_TELEGRAM_CHAT_ID".to_owned(),
                "-10042".to_owned(),
            ),
            ("UNRECOGNIZED".to_owned(), "ignored".to_owned()),
        ]);
        save_notification_config(&notifications, &values).unwrap();
        let loaded = load_notification_file(&notifications);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded["MEMORY_SUPERVISOR_TELEGRAM_CHAT_ID"], "-10042");
        let invalid = BTreeMap::from([(
            "MEMORY_SUPERVISOR_TELEGRAM_CHAT_ID".to_owned(),
            "one\ntwo".to_owned(),
        )]);
        assert_eq!(
            save_notification_config(&notifications, &invalid)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&notifications).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}
