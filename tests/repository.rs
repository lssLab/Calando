use memory_supervisor::integration::desired_hooks;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn visit(path: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        let child = entry.path();
        let name = child
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if child.is_dir() {
            if !matches!(name, ".git" | "target" | "__pycache__") {
                visit(&child, files);
            }
        } else {
            files.push(child);
        }
    }
}

#[test]
fn repository_json_and_skill_metadata_are_valid_without_a_script_runtime() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    visit(&root, &mut files);
    for path in files
        .iter()
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
    {
        let source = fs::read_to_string(path).unwrap();
        serde_json::from_str::<Value>(&source)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    }
    assert!(
        files
            .iter()
            .all(|path| path.extension().and_then(|value| value.to_str()) != Some("py")),
        "the active Rust branch must not ship a Python implementation"
    );

    let skill = fs::read_to_string(root.join("SKILL.md")).unwrap();
    assert!(skill.starts_with("---\n"));
    let frontmatter = skill.split("---").nth(1).unwrap_or_default();
    assert!(
        frontmatter
            .lines()
            .any(|line| line == "name: memory-supervisor")
    );
}

#[test]
fn repository_text_uses_only_neutral_public_identifiers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    visit(&root, &mut files);
    let text_extensions = [
        "cmd", "json", "md", "ps1", "rs", "sh", "toml", "txt", "yaml", "yml",
    ];
    let allowed_users = [
        "$USER",
        "<u>",
        "<user>",
        "<windows-user>",
        "OWNER",
        "O'Owner",
        "O''Owner",
        "Owner",
        "local",
        "owner",
    ];
    let allowed_ipv4 = ["127.0.0.1", "2.7.10.0", "6.18.33.2"];

    for path in files.iter().filter(|path| {
        path.extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| text_extensions.contains(&extension))
    }) {
        let source = fs::read_to_string(path).unwrap();

        for marker in ["/home/", "/Users/", "/mnt/c/Users/", r"C:\Users\"] {
            for (offset, _) in source.match_indices(marker) {
                let user = source[offset + marker.len()..]
                    .split(|character: char| {
                        character == '/'
                            || character == '\\'
                            || character == '`'
                            || character == '"'
                            || character.is_whitespace()
                    })
                    .next()
                    .unwrap_or("");
                if !user.is_empty() {
                    assert!(
                        allowed_users.contains(&user),
                        "{} publishes a non-neutral user path after {marker}: {user}",
                        path.display()
                    );
                }
            }
        }

        for token in source.split(|character: char| {
            character.is_whitespace()
                || matches!(
                    character,
                    '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '"' | '\'' | '`' | ',' | ';'
                )
        }) {
            if let Some((local, domain)) = token.rsplit_once('@') {
                let domain = domain.trim_matches(|character: char| {
                    !character.is_ascii_alphanumeric() && character != '.' && character != '-'
                });
                let top_level_domain = domain.rsplit('.').next().unwrap_or("");
                let valid_local = !local.is_empty()
                    && local.chars().all(|character| {
                        character.is_ascii_alphanumeric()
                            || matches!(character, '.' | '_' | '%' | '+' | '-')
                    });
                let valid_domain = domain.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '.' | '-')
                });
                assert!(
                    !valid_local
                        || !valid_domain
                        || !domain.contains('.')
                        || top_level_domain.len() < 2
                        || !top_level_domain
                            .chars()
                            .all(|character| character.is_ascii_alphabetic()),
                    "{} publishes an email address",
                    path.display()
                );
            }
        }

        for token in source.split(|character: char| !character.is_ascii_digit() && character != '.')
        {
            let octets = token
                .split('.')
                .map(str::parse::<u8>)
                .collect::<Result<Vec<_>, _>>();
            if octets.as_ref().is_ok_and(|octets| octets.len() == 4) {
                assert!(
                    allowed_ipv4.contains(&token),
                    "{} publishes a raw IPv4 address: {token}",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn public_tree_uses_only_product_documentation_directories() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for entry in fs::read_dir(root.join("docs")).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let name = name.to_str().unwrap_or("");
        let allowed = if entry.path().is_dir() {
            matches!(name, "guides" | "testing")
        } else {
            matches!(name, "README.md" | "README.ko.md")
        };
        assert!(allowed, "unexpected public documentation path: {name}");
    }

    let mut files = Vec::new();
    visit(&root.join("docs"), &mut files);
    for path in &files {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let bytes = name.as_bytes();
        let contains_iso_date = bytes.windows(10).any(|window| {
            window[0..4].iter().all(u8::is_ascii_digit)
                && window[4] == b'-'
                && window[5..7].iter().all(u8::is_ascii_digit)
                && window[7] == b'-'
                && window[8..10].iter().all(u8::is_ascii_digit)
        });
        assert!(
            !contains_iso_date,
            "public documentation filename contains a dated work-record suffix: {}",
            path.display()
        );
    }
}

#[test]
fn public_documentation_has_korean_and_english_pairs() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs");
    let mut files = Vec::new();
    visit(&root, &mut files);
    for path in files
        .iter()
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("md"))
    {
        let name = path.file_name().and_then(|value| value.to_str()).unwrap();
        let counterpart = if let Some(stem) = name.strip_suffix(".ko.md") {
            path.with_file_name(format!("{stem}.md"))
        } else {
            path.with_file_name(format!("{}.ko.md", name.trim_end_matches(".md")))
        };
        assert!(
            counterpart.is_file(),
            "public document has no language counterpart: {}",
            path.display()
        );
    }
}

#[test]
fn manual_hook_templates_follow_the_canonical_event_contract() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for (path, provider) in [
        ("adapters/cli/claude/settings.json.template", "claude"),
        ("adapters/cli/codex/hooks.json.template", "codex"),
    ] {
        let template: Value = serde_json::from_slice(&fs::read(root.join(path)).unwrap()).unwrap();
        let actual = template["hooks"].as_object().unwrap();
        let expected = desired_hooks(
            provider,
            Path::new("/memory-supervisor"),
            Path::new("/home/owner/.codex/hooks.json"),
        )
        .unwrap();
        assert_eq!(actual.len(), expected.len(), "{path}");
        for (event, expected_groups) in expected {
            let actual_groups = actual
                .get(&event)
                .unwrap_or_else(|| panic!("{path}: {event}"));
            assert_eq!(
                actual_groups[0].get("matcher"),
                expected_groups[0].get("matcher"),
                "{path}: {event} matcher"
            );
            assert_eq!(
                actual_groups[0]["hooks"][0]["timeout"], expected_groups[0]["hooks"][0]["timeout"],
                "{path}: {event} timeout"
            );
            if provider == "codex" {
                let command = actual_groups[0]["hooks"][0]["command"].as_str().unwrap();
                assert!(command.contains("/gate.sh codex "), "{path}: {event}");
                assert!(command.contains("--hook-source"), "{path}: {event}");
            }
        }
    }
}

#[test]
fn installers_separate_codex_cli_and_app_reload_instructions() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for installer in ["install.sh", "install.ps1"] {
        let source = fs::read_to_string(root.join(installer)).unwrap();
        assert!(source.contains("Codex CLI:"), "{installer}");
        assert!(source.contains("Codex App:"), "{installer}");
        assert!(
            source.contains("Confirm that all seven Memory Supervisor entries are trusted and on")
                && source.contains("Then close /hooks and continue the current work"),
            "{installer} must give the concise current-CLI procedure"
        );
        assert!(
            source.contains("Settings > Hooks, not /hooks"),
            "{installer}"
        );
        assert!(
            source.contains("continue an existing task with its next request"),
            "{installer}"
        );
        assert!(
            source.contains("USER ACTION REQUIRED")
                && source.contains("must personally")
                && source.contains("Restarting cannot grant trust"),
            "{installer} must keep Codex trust explicitly user-owned"
        );
        assert!(
            source.contains("Claude Code: USER ACTION REQUIRED for an untrusted workspace")
                && source.contains("interactive Claude holds every settings-file hook")
                && source.contains("/hooks is read-only"),
            "{installer} must explain the Claude interactive workspace-trust gate"
        );
        assert!(
            source.contains("integration resolve-claude")
                && source.contains("any existing Memory Supervisor hook was preserved"),
            "{installer} must resolve supported Claude installations without deleting a hook after a failed version probe"
        );
        assert!(
            source.contains("Run memory-status --connections anyway"),
            "{installer} must not treat unchanged wiring as proof of healthy trust"
        );
        assert!(
            !source.contains("Codex: open a new session and review"),
            "{installer}"
        );
        assert!(
            !source.contains("complete SessionStart contract"),
            "{installer} must not add SessionStart replay to user installation guidance"
        );
    }
    let powershell = fs::read_to_string(root.join("install.ps1")).unwrap();
    assert!(
        powershell.contains("function Resolve-ClaudeExecutable")
            && powershell.contains("$ErrorActionPreference = \"Continue\"")
            && powershell.contains("$ErrorActionPreference = $PreviousErrorActionPreference"),
        "install.ps1 must treat an absent Claude resolver result as a non-fatal probe"
    );
}

#[test]
fn public_bootstrap_uses_verified_release_bundles_without_a_git_prerequisite() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let unix = fs::read_to_string(root.join("bootstrap.sh")).unwrap();
    let windows = fs::read_to_string(root.join("bootstrap.ps1")).unwrap();
    for (name, source) in [("bootstrap.sh", unix), ("bootstrap.ps1", windows)] {
        assert!(
            source.contains("memory-supervisor-source")
                && source.contains(".sha256")
                && source.contains(".memory-supervisor-release-source"),
            "{name} must verify and own the version-matched release source bundle"
        );
        assert!(
            !source.contains("Git is required for one-command install"),
            "{name} must not require Git for the public one-line install"
        );
    }
    for installer in ["install.sh", "install.ps1"] {
        let source = fs::read_to_string(root.join(installer)).unwrap();
        assert!(
            source.contains(".memory-supervisor-release-source"),
            "{installer} must download the matching release binary for a release bundle"
        );
    }
    let package = fs::read_to_string(root.join("scripts/package-release-source.sh")).unwrap();
    assert!(package.contains("memory-supervisor-source.tar.gz"));
    assert!(package.contains("memory-supervisor-source.zip"));
    let verify = fs::read_to_string(root.join("scripts/verify-release-assets.sh")).unwrap();
    for target in [
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc.exe",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ] {
        assert!(verify.contains(target), "missing public asset: {target}");
    }
}
