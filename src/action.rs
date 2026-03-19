use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::Path;
use std::process::Command;

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
use arboard::{GetExtLinux, LinuxClipboardKind};
#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::collections::VecDeque;

/// Represents what happens when an action is triggered.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum ActionKind {
    /// Launch a program (with optional arguments)
    RunProgram {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        working_dir: Option<String>,
    },
    /// Open a file with the system default handler
    OpenFile { path: String },
    /// Open a URL in the default browser
    OpenUrl { url: String },
    /// Run a shell command / script
    RunShell {
        script: String,
        /// "bash", "sh", "powershell", "cmd", etc.
        #[serde(default = "default_shell")]
        shell: String,
    },
    /// Copy text to clipboard
    CopyText { text: String },
    /// Open a directory in the file manager
    OpenFolder { path: String },
    /// Search using text currently in the clipboard
    SearchClipboardText {
        #[serde(default = "default_search_url")]
        url_template: String,
    },
    /// Open a URL or file path from clipboard text, with optional fallback search
    OpenClipboardText {
        #[serde(default)]
        fallback_search_url: Option<String>,
    },
    /// Run the clipboard text as a shell command
    RunClipboardText {
        #[serde(default = "default_shell")]
        shell: String,
    },
    /// Store a native Quicker plugin document.
    PluginPipeline {
        #[serde(flatten)]
        plugin: PluginPipelineStorage,
    },
    /// A group/folder that contains sub-actions
    Group { actions: Vec<Action> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginPipelineStorage {
    pub quicker_json: String,
}

fn default_shell() -> String {
    if cfg!(target_os = "windows") {
        "powershell".into()
    } else {
        "sh".into()
    }
}

fn default_search_url() -> String {
    "https://www.google.com/search?q={query}".into()
}

/// A single action in the launcher panel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Action {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub hotkey: Option<String>,
    pub kind: ActionKind,
}

/// Result of executing an action
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecResult {
    Ok,
    OkWithMessage(String),
    Err(String),
}

impl Action {
    pub fn to_quicker_plugin_json(&self) -> Result<String, String> {
        let quicker_json = match &self.kind {
            ActionKind::PluginPipeline { plugin } => plugin.to_quicker_json()?,
            _ => return Err("Only plugin pipeline actions can be exported as Quicker JSON".into()),
        };
        let document: QuickerPluginDocument = serde_json::from_str(&quicker_json)
            .map_err(|err| format!("Failed to parse Quicker plugin JSON: {err}"))?;
        serde_json::to_string_pretty(&document)
            .map_err(|err| format!("Failed to serialize Quicker plugin JSON: {err}"))
    }

    pub fn from_quicker_plugin_json(input: &str) -> Result<Self, String> {
        let document: QuickerPluginDocument = serde_json::from_str(input)
            .map_err(|err| format!("Failed to parse Quicker plugin JSON: {err}"))?;

        if document.action_type != QUICKER_PLUGIN_ACTION_TYPE {
            return Err(format!(
                "Unsupported Quicker action type {}. Expected {} for plugins.",
                document.action_type, QUICKER_PLUGIN_ACTION_TYPE
            ));
        }

        document.data_payload()?;
        let quicker_json = serde_json::to_string_pretty(&document)
            .map_err(|err| format!("Failed to serialize Quicker plugin JSON: {err}"))?;

        Ok(Self {
            name: document.title,
            description: document.description,
            icon: document.icon.clone(),
            tags: vec![],
            hotkey: None,
            kind: ActionKind::PluginPipeline {
                plugin: PluginPipelineStorage { quicker_json },
            },
        })
    }

    /// Execute this action.
    pub fn execute(&self) -> ExecResult {
        match &self.kind {
            ActionKind::RunProgram {
                command,
                args,
                working_dir,
            } => spawn_program(command, args, working_dir.as_deref()),

            ActionKind::OpenFile { path } | ActionKind::OpenFolder { path } => {
                match open_target(path) {
                    Ok(_) => ExecResult::Ok,
                    Err(e) => ExecResult::Err(format!("Failed to open '{}': {}", path, e)),
                }
            }

            ActionKind::OpenUrl { url } => match open_target(url) {
                Ok(_) => ExecResult::Ok,
                Err(e) => ExecResult::Err(format!("Failed to open URL '{}': {}", url, e)),
            },

            ActionKind::RunShell { script, shell } => run_shell_command(script, shell),

            ActionKind::CopyText { text } => match write_clipboard_text(text) {
                Ok(_) => ExecResult::OkWithMessage("Copied to clipboard".into()),
                Err(err) => ExecResult::Err(err),
            },

            ActionKind::SearchClipboardText { url_template } => {
                let clipboard_text = match read_clipboard_text() {
                    Ok(text) => text,
                    Err(err) => return ExecResult::Err(err),
                };
                let encoded = urlencoding::encode(&clipboard_text);
                let url = if url_template.contains("{query}") {
                    url_template.replace("{query}", encoded.as_ref())
                } else {
                    format!("{url_template}{encoded}")
                };
                match open_target(&url) {
                    Ok(_) => ExecResult::OkWithMessage(format!("Searched for: {}", clipboard_text)),
                    Err(e) => {
                        ExecResult::Err(format!("Failed to open search URL '{}': {}", url, e))
                    }
                }
            }

            ActionKind::OpenClipboardText {
                fallback_search_url,
            } => {
                let clipboard_text = match read_clipboard_text() {
                    Ok(text) => text,
                    Err(err) => return ExecResult::Err(err),
                };

                if let Some(target) = clipboard_target(&clipboard_text) {
                    match open_target(&target) {
                        Ok(_) => ExecResult::OkWithMessage(format!("Opened: {}", clipboard_text)),
                        Err(e) => ExecResult::Err(format!("Failed to open '{}': {}", target, e)),
                    }
                } else if let Some(url_template) = fallback_search_url {
                    let encoded = urlencoding::encode(&clipboard_text);
                    let url = if url_template.contains("{query}") {
                        url_template.replace("{query}", encoded.as_ref())
                    } else {
                        format!("{url_template}{encoded}")
                    };
                    match open_target(&url) {
                        Ok(_) => {
                            ExecResult::OkWithMessage(format!("Searched for: {}", clipboard_text))
                        }
                        Err(e) => ExecResult::Err(format!(
                            "Failed to open fallback search '{}': {}",
                            url, e
                        )),
                    }
                } else {
                    ExecResult::Err(
                        "Clipboard does not contain a URL or existing path, and no fallback search URL is configured"
                            .into(),
                    )
                }
            }

            ActionKind::RunClipboardText { shell } => {
                let clipboard_text = match read_clipboard_text() {
                    Ok(text) => text,
                    Err(err) => return ExecResult::Err(err),
                };
                run_shell_command(&clipboard_text, shell)
            }

            ActionKind::PluginPipeline { .. } => ExecResult::Err(
                "Native Quicker plugin execution is not implemented in quicker-rs yet".into(),
            ),

            ActionKind::Group { .. } => ExecResult::Ok,
        }
    }

    /// Return searchable text for fuzzy matching.
    pub fn search_text(&self) -> String {
        let mut parts = vec![self.name.clone(), self.description.clone()];
        parts.extend(self.tags.clone());
        match &self.kind {
            ActionKind::Group { actions } => {
                parts.extend(actions.iter().map(Action::search_text));
            }
            ActionKind::PluginPipeline { plugin } => {
                parts.push("plugin quicker".into());
                parts.push(plugin.quicker_json.clone());
            }
            _ => {}
        }
        parts.join(" ")
    }
}

impl PluginPipelineStorage {
    fn to_quicker_json(&self) -> Result<String, String> {
        let document: QuickerPluginDocument = serde_json::from_str(&self.quicker_json)
            .map_err(|err| format!("Failed to parse Quicker plugin JSON: {err}"))?;
        serde_json::to_string_pretty(&document)
            .map_err(|err| format!("Failed to serialize Quicker plugin JSON: {err}"))
    }
}

const QUICKER_PLUGIN_ACTION_TYPE: u32 = 24;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct QuickerPluginDocument {
    row: u32,
    col: u32,
    action_type: u32,
    title: String,
    description: String,
    icon: Option<String>,
    path: Option<String>,
    delay_ms: u32,
    data: String,
    data2: String,
    data3: String,
    children: Option<Value>,
    id: String,
    template_id: Option<String>,
    template_revision: u32,
    use_template: bool,
    last_edit_time_utc: String,
    shared_action_id: String,
    share_time_utc: String,
    create_time_utc: String,
    as_sub_program: bool,
    skip_when_stop_running_actions: bool,
    skip_check_update: bool,
    auto_update: bool,
    keep_info_when_update: bool,
    min_quicker_version: String,
    context_menu_data: String,
    allow_scroll_trigger: bool,
    enable_evaluate_variable: bool,
    is_text_processor: bool,
    is_image_processor: bool,
    association: QuickerAssociation,
    do_not_close_panel: bool,
    user_limitation: Option<Value>,
}

impl QuickerPluginDocument {
    fn data_payload(&self) -> Result<QuickerPluginData, String> {
        serde_json::from_str(&self.data)
            .map_err(|err| format!("Failed to parse Quicker plugin data payload: {err}"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct QuickerPluginData {
    limit_single_instance: bool,
    summary_expression: String,
    sub_programs: Vec<Value>,
    variables: Vec<QuickerPluginVariable>,
    steps: Vec<QuickerPluginStepDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
struct QuickerPluginVariable {
    key: String,
    is_locked: bool,
    #[serde(rename = "Type")]
    value_type: u8,
    desc: String,
    default_value: String,
    save_state: bool,
    is_input: bool,
    is_output: bool,
    param_name: String,
    input_param_info: Option<Value>,
    output_param_info: Option<Value>,
    table_def: Option<Value>,
    custom_type: Option<Value>,
    group: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct QuickerPluginStepDocument {
    step_runner_key: String,
    input_params: Map<String, Value>,
    output_params: Map<String, Value>,
    if_steps: Option<Vec<QuickerPluginStepDocument>>,
    else_steps: Option<Vec<QuickerPluginStepDocument>>,
    note: Option<String>,
    disabled: bool,
    collapsed: bool,
    delay_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
struct QuickerAssociation {
    match_process: Option<String>,
    is_image_processor: bool,
    return_image_from_first_screen_shot_step: bool,
    is_text_processor: bool,
    return_text_from_get_selected_text_step: bool,
    text_match_expression: String,
    text_min_length: u32,
    text_max_length: u32,
    is_html_processor: bool,
    is_file_processor: bool,
    file_min_count: u32,
    file_max_count: u32,
    allowed_file_extensions: String,
    require_all_file_match_ext: bool,
    search_box_placeholder: String,
    is_window_processor: bool,
    enable_realtime_search: bool,
    browser_context_menu: Option<Value>,
    url_pattern: Option<String>,
}

impl Default for QuickerAssociation {
    fn default() -> Self {
        Self {
            match_process: None,
            is_image_processor: false,
            return_image_from_first_screen_shot_step: true,
            is_text_processor: false,
            return_text_from_get_selected_text_step: true,
            text_match_expression: String::new(),
            text_min_length: 0,
            text_max_length: 0,
            is_html_processor: false,
            is_file_processor: false,
            file_min_count: 0,
            file_max_count: 0,
            allowed_file_extensions: String::new(),
            require_all_file_match_ext: false,
            search_box_placeholder: String::new(),
            is_window_processor: false,
            enable_realtime_search: false,
            browser_context_menu: None,
            url_pattern: None,
        }
    }
}

fn spawn_program(command: &str, args: &[String], working_dir: Option<&str>) -> ExecResult {
    #[cfg(test)]
    if let Some(result) = test_spawn_program(command, args, working_dir) {
        return result;
    }

    let mut cmd = Command::new(command);
    cmd.args(args);
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    match cmd.spawn() {
        Ok(_) => ExecResult::Ok,
        Err(e) => ExecResult::Err(format!("Failed to run '{}': {}", command, e)),
    }
}

fn open_target(target: &str) -> Result<(), String> {
    #[cfg(test)]
    if let Some(result) = test_open_target(target) {
        return result;
    }

    open::that(target).map_err(|e| e.to_string())
}

fn write_clipboard_text(text: &str) -> Result<(), String> {
    #[cfg(test)]
    if let Some(result) = test_write_clipboard_text(text) {
        return result;
    }

    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("Clipboard error: {}", e))?;
    clipboard
        .set_text(text)
        .map_err(|e| format!("Clipboard error: {}", e))
}

fn read_clipboard_text() -> Result<String, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("Clipboard error: {}", e))?;

    if let Some(text) = read_standard_clipboard_text(&mut clipboard) {
        return Ok(text);
    }

    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
    ))]
    {
        if let Some(text) = read_primary_clipboard_text(&mut clipboard) {
            return Ok(text);
        }
    }

    Err(
        "No usable text was found in the clipboard. On Linux, select text first or copy it explicitly."
            .into(),
    )
}

fn read_standard_clipboard_text(clipboard: &mut arboard::Clipboard) -> Option<String> {
    #[cfg(test)]
    if let Some(text) = test_read_standard_clipboard_text() {
        return text;
    }

    normalize_clipboard_text(clipboard.get_text().ok())
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
fn read_primary_clipboard_text(clipboard: &mut arboard::Clipboard) -> Option<String> {
    #[cfg(test)]
    if let Some(text) = test_read_primary_clipboard_text() {
        return text;
    }

    normalize_clipboard_text(
        clipboard
            .get()
            .clipboard(LinuxClipboardKind::Primary)
            .text()
            .ok(),
    )
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
fn normalize_clipboard_text(text: Option<String>) -> Option<String> {
    let trimmed = text?.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn clipboard_target(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Some(trimmed.into());
    }
    if Path::new(trimmed).exists() {
        return Some(trimmed.into());
    }
    if !trimmed.contains(char::is_whitespace) && trimmed.contains('.') {
        return Some(format!("https://{}", trimmed));
    }
    None
}

fn run_shell_command(script: &str, shell: &str) -> ExecResult {
    #[cfg(test)]
    if let Some(result) = test_run_shell_command(script, shell) {
        return result;
    }

    let (sh, flag) = if cfg!(target_os = "windows") {
        match shell {
            "cmd" => ("cmd", "/C"),
            _ => ("powershell", "-Command"),
        }
    } else {
        (shell, "-c")
    };

    match Command::new(sh).arg(flag).arg(script).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if output.status.success() {
                let message = match (stdout.trim(), stderr.trim()) {
                    ("", "") => None,
                    ("", stderr) => Some(stderr.to_string()),
                    (stdout, "") => Some(stdout.to_string()),
                    (stdout, stderr) => Some(format!("{}\n{}", stdout, stderr)),
                };
                match message {
                    Some(message) => ExecResult::OkWithMessage(message),
                    None => ExecResult::Ok,
                }
            } else {
                ExecResult::Err(format!(
                    "Script exited with {}\n{}{}",
                    output.status, stdout, stderr
                ))
            }
        }
        Err(e) => ExecResult::Err(format!("Failed to run shell '{}': {}", shell, e)),
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct SpawnCall {
    command: String,
    args: Vec<String>,
    working_dir: Option<String>,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct ActionTestRuntime {
    spawn_calls: Vec<SpawnCall>,
    spawn_results: VecDeque<ExecResult>,
    opened_targets: Vec<String>,
    open_results: VecDeque<Result<(), String>>,
    clipboard_writes: Vec<String>,
    clipboard_write_results: VecDeque<Result<(), String>>,
    standard_clipboard_reads: VecDeque<Option<String>>,
    primary_clipboard_reads: VecDeque<Option<String>>,
    shell_calls: Vec<(String, String)>,
    shell_results: VecDeque<ExecResult>,
}

#[cfg(test)]
thread_local! {
    static ACTION_TEST_RUNTIME: RefCell<ActionTestRuntime> = RefCell::new(ActionTestRuntime::default());
}

#[cfg(test)]
fn with_action_test_runtime<R>(f: impl FnOnce(&mut ActionTestRuntime) -> R) -> R {
    ACTION_TEST_RUNTIME.with(|runtime| f(&mut runtime.borrow_mut()))
}

#[cfg(test)]
fn reset_action_test_runtime() {
    with_action_test_runtime(|runtime| *runtime = ActionTestRuntime::default());
}

#[cfg(test)]
fn test_spawn_program(
    command: &str,
    args: &[String],
    working_dir: Option<&str>,
) -> Option<ExecResult> {
    with_action_test_runtime(|runtime| {
        runtime.spawn_calls.push(SpawnCall {
            command: command.into(),
            args: args.to_vec(),
            working_dir: working_dir.map(str::to_string),
        });
        runtime.spawn_results.pop_front()
    })
}

#[cfg(test)]
fn test_open_target(target: &str) -> Option<Result<(), String>> {
    with_action_test_runtime(|runtime| {
        runtime.opened_targets.push(target.into());
        runtime.open_results.pop_front()
    })
}

#[cfg(test)]
fn test_write_clipboard_text(text: &str) -> Option<Result<(), String>> {
    with_action_test_runtime(|runtime| {
        runtime.clipboard_writes.push(text.into());
        runtime.clipboard_write_results.pop_front()
    })
}

#[cfg(test)]
fn test_read_standard_clipboard_text() -> Option<Option<String>> {
    with_action_test_runtime(|runtime| runtime.standard_clipboard_reads.pop_front())
}

#[cfg(test)]
fn test_read_primary_clipboard_text() -> Option<Option<String>> {
    with_action_test_runtime(|runtime| runtime.primary_clipboard_reads.pop_front())
}

#[cfg(test)]
fn test_run_shell_command(script: &str, shell: &str) -> Option<ExecResult> {
    with_action_test_runtime(|runtime| {
        runtime.shell_calls.push((shell.into(), script.into()));
        runtime.shell_results.pop_front()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn action(kind: ActionKind) -> Action {
        Action {
            name: "Test".into(),
            description: "desc".into(),
            icon: None,
            tags: vec!["tag".into()],
            hotkey: None,
            kind,
        }
    }

    #[test]
    fn quicker_plugin_document_parses_sample_json() {
        let sample = fs::read_to_string("sample.json").expect("sample.json should exist");
        let document: QuickerPluginDocument =
            serde_json::from_str(&sample).expect("sample should match Quicker schema");
        let data = document
            .data_payload()
            .expect("sample data payload should parse");

        assert_eq!(document.action_type, QUICKER_PLUGIN_ACTION_TYPE);
        assert_eq!(document.title, "统一格式");
        assert_eq!(data.variables.len(), 6);
        assert_eq!(data.steps.len(), 17);
        assert_eq!(data.steps[0].step_runner_key, "sys:keyInput");
        assert_eq!(data.steps.last().unwrap().step_runner_key, "sys:keyInput");
    }

    #[test]
    fn plugin_pipeline_exports_quicker_json_with_sample_shape() {
        let action = Action {
            name: "Clipboard Uppercase".into(),
            description: "Uppercase clipboard text".into(),
            icon: Some("icon.png".into()),
            tags: vec!["plugin".into()],
            hotkey: None,
            kind: ActionKind::PluginPipeline {
                plugin: PluginPipelineStorage {
                    quicker_json: fs::read_to_string("sample.json").unwrap(),
                },
            },
        };

        let json = action
            .to_quicker_plugin_json()
            .expect("plugin export should serialize");
        let document: QuickerPluginDocument =
            serde_json::from_str(&json).expect("export should be valid Quicker JSON");
        let data = document
            .data_payload()
            .expect("exported data payload should parse");

        assert_eq!(document.action_type, QUICKER_PLUGIN_ACTION_TYPE);
        assert_eq!(document.title, "统一格式");
        assert_eq!(document.description, "将粘贴/导入内容的自带样式去除");
        assert!(document.enable_evaluate_variable);
        assert_eq!(data.variables.len(), 6);
        assert_eq!(data.steps.len(), 17);
    }

    #[test]
    fn quicker_plugin_round_trips_as_native_json() {
        let sample = fs::read_to_string("sample.json").unwrap();

        let parsed = Action::from_quicker_plugin_json(&sample).expect("sample should parse");

        assert_eq!(parsed.name, "统一格式");
        assert_eq!(parsed.description, "将粘贴/导入内容的自带样式去除");
        assert_eq!(parsed.icon.as_deref(), Some("https://files.getquicker.net/_icons/2D62F4E62FD40AC3F99CB7ABE05B9E2FAE141A3B.png"));
        assert_eq!(
            parsed.to_quicker_plugin_json().unwrap(),
            Action::from_quicker_plugin_json(&sample)
                .unwrap()
                .to_quicker_plugin_json()
                .unwrap()
        );
    }

    #[test]
    fn plugin_pipeline_execution_is_not_supported() {
        let result = action(ActionKind::PluginPipeline {
            plugin: PluginPipelineStorage {
                quicker_json: fs::read_to_string("sample.json").unwrap(),
            },
        })
        .execute();

        assert_eq!(
            result,
            ExecResult::Err("Native Quicker plugin execution is not implemented in quicker-rs yet".into())
        );
    }

    #[test]
    fn run_program_executes_with_expected_arguments() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| runtime.spawn_results.push_back(ExecResult::Ok));

        let result = action(ActionKind::RunProgram {
            command: "demo".into(),
            args: vec!["--flag".into()],
            working_dir: Some("/tmp".into()),
        })
        .execute();

        assert_eq!(result, ExecResult::Ok);
        with_action_test_runtime(|runtime| {
            assert_eq!(
                runtime.spawn_calls,
                vec![SpawnCall {
                    command: "demo".into(),
                    args: vec!["--flag".into()],
                    working_dir: Some("/tmp".into()),
                }]
            );
        });
    }

    #[test]
    fn open_file_uses_open_target() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| runtime.open_results.push_back(Ok(())));

        let result = action(ActionKind::OpenFile {
            path: "/tmp/file.txt".into(),
        })
        .execute();

        assert_eq!(result, ExecResult::Ok);
        with_action_test_runtime(|runtime| {
            assert_eq!(runtime.opened_targets, vec!["/tmp/file.txt"]);
        });
    }

    #[test]
    fn open_url_uses_open_target() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| runtime.open_results.push_back(Ok(())));

        let result = action(ActionKind::OpenUrl {
            url: "https://example.com".into(),
        })
        .execute();

        assert_eq!(result, ExecResult::Ok);
        with_action_test_runtime(|runtime| {
            assert_eq!(runtime.opened_targets, vec!["https://example.com"]);
        });
    }

    #[test]
    fn run_shell_returns_hooked_output() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime
                .shell_results
                .push_back(ExecResult::OkWithMessage("done".into()))
        });

        let result = action(ActionKind::RunShell {
            script: "echo hi".into(),
            shell: "sh".into(),
        })
        .execute();

        assert_eq!(result, ExecResult::OkWithMessage("done".into()));
        with_action_test_runtime(|runtime| {
            assert_eq!(runtime.shell_calls, vec![("sh".into(), "echo hi".into())]);
        });
    }

    #[test]
    fn copy_text_writes_clipboard() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| runtime.clipboard_write_results.push_back(Ok(())));

        let result = action(ActionKind::CopyText {
            text: "hello".into(),
        })
        .execute();

        assert_eq!(
            result,
            ExecResult::OkWithMessage("Copied to clipboard".into())
        );
        with_action_test_runtime(|runtime| {
            assert_eq!(runtime.clipboard_writes, vec!["hello"]);
        });
    }

    #[test]
    fn open_folder_uses_open_target() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| runtime.open_results.push_back(Ok(())));

        let result = action(ActionKind::OpenFolder {
            path: "/tmp".into(),
        })
        .execute();

        assert_eq!(result, ExecResult::Ok);
        with_action_test_runtime(|runtime| {
            assert_eq!(runtime.opened_targets, vec!["/tmp"]);
        });
    }

    #[test]
    fn search_clipboard_text_builds_query_url() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime
                .standard_clipboard_reads
                .push_back(Some("hello world".into()));
            runtime.open_results.push_back(Ok(()));
        });

        let result = action(ActionKind::SearchClipboardText {
            url_template: "https://search.example/?q={query}".into(),
        })
        .execute();

        assert_eq!(
            result,
            ExecResult::OkWithMessage("Searched for: hello world".into())
        );
        with_action_test_runtime(|runtime| {
            assert_eq!(
                runtime.opened_targets,
                vec!["https://search.example/?q=hello%20world"]
            );
        });
    }

    #[test]
    fn open_clipboard_text_opens_direct_url() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime
                .standard_clipboard_reads
                .push_back(Some("https://example.com".into()));
            runtime.open_results.push_back(Ok(()));
        });

        let result = action(ActionKind::OpenClipboardText {
            fallback_search_url: Some("https://search.example/?q={query}".into()),
        })
        .execute();

        assert_eq!(
            result,
            ExecResult::OkWithMessage("Opened: https://example.com".into())
        );
        with_action_test_runtime(|runtime| {
            assert_eq!(runtime.opened_targets, vec!["https://example.com"]);
        });
    }

    #[test]
    fn open_clipboard_text_uses_existing_path() {
        reset_action_test_runtime();
        let temp_path = std::env::temp_dir().join("quicker-rs-open-clipboard-test.txt");
        fs::write(&temp_path, "demo").unwrap();
        with_action_test_runtime(|runtime| {
            runtime
                .standard_clipboard_reads
                .push_back(Some(temp_path.to_string_lossy().to_string()));
            runtime.open_results.push_back(Ok(()));
        });

        let result = action(ActionKind::OpenClipboardText {
            fallback_search_url: None,
        })
        .execute();

        assert_eq!(
            result,
            ExecResult::OkWithMessage(format!("Opened: {}", temp_path.display()))
        );
        with_action_test_runtime(|runtime| {
            assert_eq!(
                runtime.opened_targets,
                vec![temp_path.to_string_lossy().to_string()]
            );
        });
        let _ = fs::remove_file(temp_path);
    }

    #[test]
    fn open_clipboard_text_uses_fallback_search() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime
                .standard_clipboard_reads
                .push_back(Some("need search".into()));
            runtime.open_results.push_back(Ok(()));
        });

        let result = action(ActionKind::OpenClipboardText {
            fallback_search_url: Some("https://search.example/?q={query}".into()),
        })
        .execute();

        assert_eq!(
            result,
            ExecResult::OkWithMessage("Searched for: need search".into())
        );
        with_action_test_runtime(|runtime| {
            assert_eq!(
                runtime.opened_targets,
                vec!["https://search.example/?q=need%20search"]
            );
        });
    }

    #[test]
    fn open_clipboard_text_errors_without_fallback() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime
                .standard_clipboard_reads
                .push_back(Some("not a target".into()));
        });

        let result = action(ActionKind::OpenClipboardText {
            fallback_search_url: None,
        })
        .execute();

        assert_eq!(
            result,
            ExecResult::Err(
                "Clipboard does not contain a URL or existing path, and no fallback search URL is configured"
                    .into()
            )
        );
    }

    #[test]
    fn run_clipboard_text_reads_primary_selection_when_standard_clipboard_is_empty() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime.standard_clipboard_reads.push_back(None);
            runtime
                .primary_clipboard_reads
                .push_back(Some("echo selected".into()));
            runtime.shell_results.push_back(ExecResult::Ok);
        });

        let result = action(ActionKind::RunClipboardText { shell: "sh".into() }).execute();

        assert_eq!(result, ExecResult::Ok);
        with_action_test_runtime(|runtime| {
            assert_eq!(
                runtime.shell_calls,
                vec![("sh".into(), "echo selected".into())]
            );
        });
    }

    #[test]
    fn group_actions_are_not_executed() {
        reset_action_test_runtime();
        let result = action(ActionKind::Group { actions: vec![] }).execute();
        assert_eq!(result, ExecResult::Ok);
    }

    #[test]
    fn search_text_includes_group_children() {
        let grouped = action(ActionKind::Group {
            actions: vec![Action {
                name: "Child".into(),
                description: "Nested".into(),
                icon: None,
                tags: vec!["inside".into()],
                hotkey: None,
                kind: ActionKind::CopyText {
                    text: "copy".into(),
                },
            }],
        });

        let text = grouped.search_text();

        assert!(text.contains("Child"));
        assert!(text.contains("Nested"));
        assert!(text.contains("inside"));
    }
}
