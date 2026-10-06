use crate::action::{Action, ActionKind};
use crate::focus::FocusedProcess;
#[cfg(not(target_arch = "wasm32"))]
use crate::focus::BROWSER_PROCESS_PATTERNS;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A profile is a named set of actions (like Quicker's "scenes").
/// You can have a default profile and app-specific profiles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// If set, this profile activates when one of these process names is focused.
    #[serde(default)]
    pub match_processes: Vec<String>,
    pub actions: Vec<Action>,
}

/// Top-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Global hotkey to toggle the panel (e.g. "Super+Space")
    #[serde(default = "default_toggle_hotkey")]
    pub toggle_hotkey: String,

    /// Number of columns in the action grid
    #[serde(default = "default_columns")]
    pub columns: usize,

    /// Panel width
    #[serde(default = "default_width")]
    pub panel_width: f32,

    /// Panel height
    #[serde(default = "default_height")]
    pub panel_height: f32,

    /// All profiles
    pub profiles: Vec<Profile>,
}

fn default_toggle_hotkey() -> String {
    "Alt+Space".into()
}
fn default_columns() -> usize {
    4
}
fn default_width() -> f32 {
    600.0
}
fn default_height() -> f32 {
    500.0
}

impl Config {
    /// Path to the config file.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn config_path() -> PathBuf {
        let dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("quicker-rs");
        dir.join("config.toml")
    }

    #[cfg(target_arch = "wasm32")]
    pub fn config_path() -> PathBuf {
        PathBuf::from("browser-preview://config.toml")
    }

    /// Load without modifying an existing file, including an invalid one.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load() -> Result<Self, String> {
        Self::load_from(&Self::config_path())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_from(path: &std::path::Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let cfg: Self = toml::from_str(&content)
                    .map_err(|err| format!("Invalid config {}: {err}", path.display()))?;
                cfg.validate()?;
                Ok(cfg)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let cfg = Self::default();
                cfg.save_to(path)?;
                Ok(cfg)
            }
            Err(err) => Err(format!("Cannot read config {}: {err}", path.display())),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.profiles.is_empty() {
            return Err("At least one profile is required".into());
        }
        if !(1..=12).contains(&self.columns) {
            return Err("Grid columns must be between 1 and 12".into());
        }
        if !self.panel_width.is_finite()
            || !(300.0..=2400.0).contains(&self.panel_width)
            || !self.panel_height.is_finite()
            || !(200.0..=1600.0).contains(&self.panel_height)
        {
            return Err("Panel dimensions must be finite: width 300–2400, height 200–1600".into());
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.toggle_hotkey
            .parse::<global_hotkey::hotkey::HotKey>()
            .map_err(|err| format!("Invalid toggle hotkey: {err}"))?;
        for profile in &self.profiles {
            if profile.name.trim().is_empty() {
                return Err("Profile names cannot be empty".into());
            }
        }
        Ok(())
    }

    pub fn matching_profile_index(&self, process: &FocusedProcess) -> Option<usize> {
        self.profiles
            .iter()
            .enumerate()
            .find(|(_, profile)| profile.matches_process(process))
            .map(|(idx, _)| idx)
    }

    /// Replace the file atomically. Errors are returned to the caller.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save(&self) -> Result<(), String> {
        self.save_to(&Self::config_path())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_to(&self, path: &std::path::Path) -> Result<(), String> {
        self.validate()?;
        let content = toml::to_string_pretty(self).map_err(|err| err.to_string())?;
        crate::storage::atomic_write(path, content.as_bytes())
            .map_err(|err| format!("Cannot save config {}: {err}", path.display()))
    }

    #[cfg(target_arch = "wasm32")]
    pub fn save(&self) -> Result<(), String> {
        self.validate()
    }
}

impl Profile {
    pub fn matches_process(&self, process: &FocusedProcess) -> bool {
        !self.match_processes.is_empty()
            && self
                .match_processes
                .iter()
                .any(|pattern| process.matches_pattern(pattern))
    }
}

impl Default for Config {
    fn default() -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            return Self {
                toggle_hotkey: default_toggle_hotkey(),
                columns: default_columns(),
                panel_width: 1100.0,
                panel_height: 760.0,
                profiles: vec![Profile {
                    name: "Preview".into(),
                    description: "Browser-safe actions for the GitHub Pages demo".into(),
                    match_processes: vec![],
                    actions: example_actions(),
                }],
            };
        }

        #[cfg(not(target_arch = "wasm32"))]
        Self {
            toggle_hotkey: default_toggle_hotkey(),
            columns: default_columns(),
            panel_width: default_width(),
            panel_height: default_height(),
            profiles: vec![
                Profile {
                    name: "Default".into(),
                    description: "General-purpose actions".into(),
                    match_processes: vec![],
                    actions: example_actions(),
                },
                default_browser_profile(),
            ],
        }
    }
}

/// Starter actions so the panel isn't empty on first launch.
#[cfg(target_arch = "wasm32")]
fn example_actions() -> Vec<Action> {
    vec![
        Action {
            name: "Project Repo".into(),
            description: "Open the GitHub repository in a new tab".into(),
            icon: Some("🐙".into()),
            tags: vec!["github".into(), "repo".into(), "source".into()],
            hotkey: None,
            kind: ActionKind::OpenUrl {
                url: "https://github.com/asukaminato0721/quicker-rs".into(),
            },
        },
        Action {
            name: "Rust + egui".into(),
            description: "Open the egui project page".into(),
            icon: Some("🦀".into()),
            tags: vec!["rust".into(), "egui".into(), "ui".into()],
            hotkey: None,
            kind: ActionKind::OpenUrl {
                url: "https://github.com/emilk/egui".into(),
            },
        },
        Action {
            name: "Copy Demo Text".into(),
            description: "Exercise the clipboard path that still works in the browser preview".into(),
            icon: Some("📋".into()),
            tags: vec!["copy".into(), "clipboard".into(), "demo".into()],
            hotkey: None,
            kind: ActionKind::CopyText {
                text: "Hello from the Quicker-RS web preview".into(),
            },
        },
        Action {
            name: "Preview Notes".into(),
            description: "What the GitHub Pages build can and cannot demonstrate".into(),
            icon: Some("🧭".into()),
            tags: vec!["preview".into(), "notes".into(), "web".into()],
            hotkey: None,
            kind: ActionKind::Group {
                actions: vec![
                    Action {
                        name: "Native Build".into(),
                        description: "Desktop-only integrations stay in the native binary".into(),
                        icon: Some("🖥".into()),
                        tags: vec!["desktop".into(), "native".into()],
                        hotkey: None,
                        kind: ActionKind::CopyText {
                            text: "Desktop build keeps hotkeys, shelling out, file access, and focused-window matching.".into(),
                        },
                    },
                    Action {
                        name: "Web Build".into(),
                        description: "Browser preview focuses on layout and safe interactions".into(),
                        icon: Some("🌐".into()),
                        tags: vec!["web".into(), "wasm".into()],
                        hotkey: None,
                        kind: ActionKind::CopyText {
                            text: "Web preview supports browsing the UI, opening links, editing plugins, and copying text.".into(),
                        },
                    },
                ],
            },
        },
    ]
}

#[cfg(not(target_arch = "wasm32"))]
fn example_actions() -> Vec<Action> {
    let terminal = Action {
        name: "Terminal".into(),
        description: "Open a terminal emulator".into(),
        icon: Some("🖥".into()),
        tags: vec!["shell".into(), "console".into(), "term".into()],
        hotkey: None,
        kind: ActionKind::RunProgram {
            command: if cfg!(target_os = "windows") {
                "wt".into() // Windows Terminal
            } else if cfg!(target_os = "macos") {
                "/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal".into()
            } else {
                // Try common Linux terminals
                which::which("kitty")
                    .or_else(|_| which::which("alacritty"))
                    .or_else(|_| which::which("gnome-terminal"))
                    .or_else(|_| which::which("konsole"))
                    .or_else(|_| which::which("xterm"))
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| "xterm".into())
            },
            args: vec![],
            working_dir: None,
        },
    };

    let file_manager = Action {
        name: "File Manager".into(),
        description: "Open home directory".into(),
        icon: Some("📁".into()),
        tags: vec!["files".into(), "explorer".into(), "nautilus".into()],
        hotkey: None,
        kind: ActionKind::OpenFolder {
            path: dirs::home_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
        },
    };

    let web_browser = Action {
        name: "Web Browser".into(),
        description: "Open default browser".into(),
        icon: Some("🌐".into()),
        tags: vec!["browser".into(), "firefox".into(), "chrome".into()],
        hotkey: None,
        kind: ActionKind::OpenUrl {
            url: "https://google.com".into(),
        },
    };

    let system_info = Action {
        name: "System Info".into(),
        description: "Show basic system information".into(),
        icon: Some("ℹ️".into()),
        tags: vec!["system".into(), "info".into(), "uname".into()],
        hotkey: None,
        kind: ActionKind::RunShell {
            script: if cfg!(target_os = "windows") {
                "systeminfo | Select-Object -First 20".into()
            } else {
                "uname -a && echo '---' && uptime && echo '---' && free -h 2>/dev/null || vm_stat 2>/dev/null".into()
            },
            shell: default_shell(),
        },
    };

    let ip_address = Action {
        name: "IP Address".into(),
        description: "Show network IP addresses".into(),
        icon: Some("📡".into()),
        tags: vec!["ip".into(), "network".into(), "address".into()],
        hotkey: None,
        kind: ActionKind::RunShell {
            script: if cfg!(target_os = "windows") {
                "ipconfig | findstr IPv4".into()
            } else {
                "ip -brief addr 2>/dev/null || ifconfig 2>/dev/null | grep inet".into()
            },
            shell: default_shell(),
        },
    };

    let clipboard = Action {
        name: "Copy Greeting".into(),
        description: "Copy a useful snippet".into(),
        icon: Some("📋".into()),
        tags: vec!["clipboard".into(), "copy".into()],
        hotkey: None,
        kind: ActionKind::CopyText {
            text: "Hello from Quicker-RS!".into(),
        },
    };

    let mut pdf_demo = pdf_demo_actions().into_iter();
    let quick_search = pdf_demo.next().unwrap();
    let smart_open_clipboard = pdf_demo.next().unwrap();
    let run_clipboard_text = pdf_demo.next().unwrap();

    let mut desktop_tools = vec![terminal.clone(), file_manager.clone()];
    if let Some(editor) = default_text_editor_action() {
        desktop_tools.push(editor);
    }
    if let Some(calculator) = default_calculator_action() {
        desktop_tools.push(calculator);
    }

    let web_shortcuts = vec![
        web_browser.clone(),
        Action {
            name: "GitHub".into(),
            description: "Open GitHub".into(),
            icon: Some("🐙".into()),
            tags: vec!["git".into(), "code".into(), "repo".into()],
            hotkey: None,
            kind: ActionKind::OpenUrl {
                url: "https://github.com".into(),
            },
        },
        Action {
            name: "Rust Docs".into(),
            description: "Open the Rust standard library docs".into(),
            icon: Some("🦀".into()),
            tags: vec!["rust".into(), "docs".into(), "std".into()],
            hotkey: None,
            kind: ActionKind::OpenUrl {
                url: "https://doc.rust-lang.org/std/".into(),
            },
        },
        Action {
            name: "Crates.io".into(),
            description: "Browse Rust crates".into(),
            icon: Some("📦".into()),
            tags: vec!["rust".into(), "crate".into(), "packages".into()],
            hotkey: None,
            kind: ActionKind::OpenUrl {
                url: "https://crates.io".into(),
            },
        },
    ];

    vec![
        Action {
            name: "Desktop Tools".into(),
            description: "Grouped desktop utilities".into(),
            icon: Some("🧰".into()),
            tags: vec!["tools".into(), "group".into(), "desktop".into()],
            hotkey: None,
            kind: ActionKind::Group {
                actions: desktop_tools,
            },
        },
        Action {
            name: "Web Shortcuts".into(),
            description: "Grouped browser and web shortcuts".into(),
            icon: Some("🌍".into()),
            tags: vec!["web".into(), "browser".into(), "group".into()],
            hotkey: None,
            kind: ActionKind::Group {
                actions: web_shortcuts,
            },
        },
        quick_search,
        smart_open_clipboard,
        run_clipboard_text,
        system_info,
        ip_address,
        clipboard,
    ]
}

fn default_shell() -> String {
    if cfg!(target_os = "windows") {
        "powershell".into()
    } else {
        "sh".into()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn detect_command(candidates: &[&str]) -> Option<String> {
    candidates.iter().find_map(|command| {
        which::which(command)
            .ok()
            .map(|path| path.to_string_lossy().to_string())
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn default_text_editor_action() -> Option<Action> {
    let command = if cfg!(target_os = "windows") {
        Some("notepad".into())
    } else if cfg!(target_os = "macos") {
        Some("/Applications/TextEdit.app/Contents/MacOS/TextEdit".into())
    } else {
        detect_command(&[
            "gedit", "xed", "kate", "mousepad", "pluma", "leafpad", "code",
        ])
    }?;

    Some(Action {
        name: "Notepad".into(),
        description: "Open a text editor".into(),
        icon: Some("📝".into()),
        tags: vec!["notes".into(), "editor".into(), "text".into()],
        hotkey: None,
        kind: ActionKind::RunProgram {
            command,
            args: vec![],
            working_dir: None,
        },
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn default_calculator_action() -> Option<Action> {
    let command = if cfg!(target_os = "windows") {
        Some("calc".into())
    } else if cfg!(target_os = "macos") {
        Some("/System/Applications/Calculator.app/Contents/MacOS/Calculator".into())
    } else {
        detect_command(&["gnome-calculator", "kcalc", "galculator", "qalculate-gtk"])
    }?;

    Some(Action {
        name: "Calculator".into(),
        description: "Open the system calculator".into(),
        icon: Some("🧮".into()),
        tags: vec!["calc".into(), "math".into(), "desktop".into()],
        hotkey: None,
        kind: ActionKind::RunProgram {
            command,
            args: vec![],
            working_dir: None,
        },
    })
}

fn pdf_demo_actions() -> Vec<Action> {
    vec![
        Action {
            name: "Quick Search".into(),
            description: "Search the current clipboard text in your browser".into(),
            icon: Some("🔎".into()),
            tags: vec!["search".into(), "clipboard".into(), "selected text".into()],
            hotkey: None,
            kind: ActionKind::SearchClipboardText {
                url_template: "https://www.google.com/search?q={query}".into(),
            },
        },
        Action {
            name: "Smart Open Clipboard".into(),
            description: "Open the clipboard as a URL/path, or search for it if needed".into(),
            icon: Some("🧠".into()),
            tags: vec![
                "clipboard".into(),
                "url".into(),
                "link".into(),
                "smart".into(),
            ],
            hotkey: None,
            kind: ActionKind::OpenClipboardText {
                fallback_search_url: Some("https://www.google.com/search?q={query}".into()),
            },
        },
        Action {
            name: "Run Clipboard Text".into(),
            description: "Run the current clipboard text as a shell command".into(),
            icon: Some("▶".into()),
            tags: vec![
                "clipboard".into(),
                "run".into(),
                "command".into(),
                "selected text".into(),
            ],
            hotkey: None,
            kind: ActionKind::RunClipboardText {
                shell: default_shell(),
            },
        },
    ]
}

#[cfg(not(target_arch = "wasm32"))]
fn default_browser_profile() -> Profile {
    Profile {
        name: "Browser".into(),
        description: "Actions shown when a web browser is focused".into(),
        match_processes: BROWSER_PROCESS_PATTERNS
            .iter()
            .map(|pattern| (*pattern).to_string())
            .collect(),
        actions: browser_actions(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn browser_actions() -> Vec<Action> {
    vec![
        Action {
            name: "Quick Search".into(),
            description: "Search the selected or copied text on Google".into(),
            icon: Some("🔎".into()),
            tags: vec!["search".into(), "clipboard".into(), "browser".into()],
            hotkey: None,
            kind: ActionKind::SearchClipboardText {
                url_template: "https://www.google.com/search?q={query}".into(),
            },
        },
        Action {
            name: "Search YouTube".into(),
            description: "Search the selected or copied text on YouTube".into(),
            icon: Some("▶".into()),
            tags: vec!["search".into(), "youtube".into(), "video".into()],
            hotkey: None,
            kind: ActionKind::SearchClipboardText {
                url_template: "https://www.youtube.com/results?search_query={query}".into(),
            },
        },
        Action {
            name: "Search Wikipedia".into(),
            description: "Search the selected or copied text on Wikipedia".into(),
            icon: Some("📚".into()),
            tags: vec!["search".into(), "wiki".into(), "knowledge".into()],
            hotkey: None,
            kind: ActionKind::SearchClipboardText {
                url_template: "https://en.wikipedia.org/w/index.php?search={query}".into(),
            },
        },
        Action {
            name: "Translate Clipboard".into(),
            description: "Translate the selected or copied text in Google Translate".into(),
            icon: Some("🌍".into()),
            tags: vec!["translate".into(), "clipboard".into(), "browser".into()],
            hotkey: None,
            kind: ActionKind::SearchClipboardText {
                url_template:
                    "https://translate.google.com/?sl=auto&tl=auto&text={query}&op=translate".into(),
            },
        },
        Action {
            name: "Smart Open Clipboard".into(),
            description: "Open the clipboard as a URL/path, or search for it if needed".into(),
            icon: Some("🧠".into()),
            tags: vec![
                "clipboard".into(),
                "url".into(),
                "link".into(),
                "smart".into(),
            ],
            hotkey: None,
            kind: ActionKind::OpenClipboardText {
                fallback_search_url: Some("https://www.google.com/search?q={query}".into()),
            },
        },
        Action {
            name: "Web Shortcuts".into(),
            description: "Grouped browser and web shortcuts".into(),
            icon: Some("🌐".into()),
            tags: vec!["web".into(), "browser".into(), "group".into()],
            hotkey: None,
            kind: ActionKind::Group {
                actions: vec![
                    Action {
                        name: "Open Browser Home".into(),
                        description: "Open your default browser home page".into(),
                        icon: Some("🏠".into()),
                        tags: vec!["browser".into(), "home".into()],
                        hotkey: None,
                        kind: ActionKind::OpenUrl {
                            url: "https://google.com".into(),
                        },
                    },
                    Action {
                        name: "GitHub".into(),
                        description: "Open GitHub".into(),
                        icon: Some("🐙".into()),
                        tags: vec!["git".into(), "code".into(), "repo".into()],
                        hotkey: None,
                        kind: ActionKind::OpenUrl {
                            url: "https://github.com".into(),
                        },
                    },
                    Action {
                        name: "MDN Web Docs".into(),
                        description: "Open MDN documentation".into(),
                        icon: Some("📘".into()),
                        tags: vec!["docs".into(), "web".into(), "mdn".into()],
                        hotkey: None,
                        kind: ActionKind::OpenUrl {
                            url: "https://developer.mozilla.org".into(),
                        },
                    },
                    Action {
                        name: "YouTube".into(),
                        description: "Open YouTube".into(),
                        icon: Some("🎬".into()),
                        tags: vec!["video".into(), "youtube".into(), "watch".into()],
                        hotkey: None,
                        kind: ActionKind::OpenUrl {
                            url: "https://www.youtube.com".into(),
                        },
                    },
                ],
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus::FocusedProcess;

    #[test]
    fn invalid_config_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let invalid = "my broken config = [";
        std::fs::write(&path, invalid).unwrap();
        assert!(Config::load_from(&path).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), invalid);
    }

    #[test]
    fn load_preserves_deleted_defaults_and_custom_actions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut cfg = Config::default();
        cfg.profiles.truncate(1);
        cfg.profiles[0].actions.clear();
        cfg.save_to(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.profiles.len(), 1);
        assert!(loaded.profiles[0].actions.is_empty());
        assert_eq!(std::fs::read(path).unwrap(), original);
    }

    #[test]
    fn invalid_settings_cannot_replace_saved_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/config.toml");
        let mut cfg = Config::load_from(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        cfg.columns = 0;
        assert!(cfg.save_to(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        cfg.columns = 4;
        cfg.panel_width = f32::NAN;
        assert!(cfg.save_to(&path).is_err());
        cfg.panel_width = 600.0;
        cfg.profiles.clear();
        assert!(cfg.save_to(&path).is_err());
    }

    fn focused_process(name: &str, path: &str) -> FocusedProcess {
        FocusedProcess {
            app_name: name.into(),
            process_id: 123,
            process_path: path.into(),
        }
    }

    #[test]
    fn example_actions_contains_pdf_demo_actions() {
        let names: Vec<_> = example_actions()
            .into_iter()
            .map(|action| action.name)
            .collect();

        assert!(names.contains(&"Quick Search".into()));
        assert!(names.contains(&"Smart Open Clipboard".into()));
        assert!(names.contains(&"Run Clipboard Text".into()));
    }

    #[test]
    fn default_config_includes_browser_profile() {
        let cfg = Config::default();

        assert_eq!(cfg.profiles.len(), 2);
        assert_eq!(cfg.profiles[1].name, "Browser");
        assert!(cfg.profiles[1].matches_process(&focused_process("Firefox", "/usr/bin/firefox")));
    }

    #[test]
    fn profile_matches_process_against_configured_names() {
        let profile = Profile {
            name: "Dev".into(),
            description: String::new(),
            match_processes: vec!["code".into(), "zed.exe".into()],
            actions: vec![],
        };

        assert!(profile.matches_process(&focused_process("Code", "/usr/bin/code")));
        assert!(profile.matches_process(&focused_process("Zed", "C:/Program Files/Zed/zed.exe")));
        assert!(!profile.matches_process(&focused_process("Firefox", "/usr/bin/firefox")));
    }

    #[test]
    fn browser_profile_matches_browser_variants() {
        let profile = default_browser_profile();

        assert!(profile.matches_process(&focused_process(
            "Google Chrome",
            "/usr/bin/google-chrome-stable"
        )));
        assert!(
            profile.matches_process(&focused_process("Brave Browser", "/usr/bin/brave-browser"))
        );
    }

    #[test]
    fn matching_profile_index_returns_first_profile_match() {
        let cfg = Config {
            toggle_hotkey: "Alt+Space".into(),
            columns: 4,
            panel_width: 600.0,
            panel_height: 500.0,
            profiles: vec![
                Profile {
                    name: "Default".into(),
                    description: String::new(),
                    match_processes: vec![],
                    actions: vec![],
                },
                Profile {
                    name: "Code".into(),
                    description: String::new(),
                    match_processes: vec!["code".into()],
                    actions: vec![],
                },
                Profile {
                    name: "Browser".into(),
                    description: String::new(),
                    match_processes: vec!["firefox".into()],
                    actions: vec![],
                },
            ],
        };

        assert_eq!(
            cfg.matching_profile_index(&focused_process("Code", "/usr/bin/code")),
            Some(1)
        );
        assert_eq!(
            cfg.matching_profile_index(&focused_process("Firefox", "/usr/bin/firefox")),
            Some(2)
        );
        assert_eq!(
            cfg.matching_profile_index(&focused_process("Slack", "/usr/bin/slack")),
            None
        );
    }
}
