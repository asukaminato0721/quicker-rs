use fancy_regex::Regex;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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
    /// Store a native Quicker action document.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LowCodePluginDraft {
    pub kind: LowCodePluginKind,
    pub title: String,
    pub description: String,
    pub icon: Option<String>,
    pub key_macro_steps: Vec<LowCodeKeyMacroStep>,
    pub launch_path: String,
    pub launch_arguments: String,
    pub launch_set_working_dir: bool,
    pub steps: Vec<LowCodePluginStep>,
}

impl Default for LowCodePluginDraft {
    fn default() -> Self {
        Self {
            kind: LowCodePluginKind::PluginFlow,
            title: String::new(),
            description: String::new(),
            icon: None,
            key_macro_steps: Vec::new(),
            launch_path: String::new(),
            launch_arguments: String::new(),
            launch_set_working_dir: false,
            steps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LowCodePluginKind {
    KeyMacro,
    OpenApp,
    PluginFlow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowCodeKeyMacroStep {
    SendKeys { modifiers: String, key: String },
    TypeText { text: String },
    Delay { delay_ms: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowCodePluginStep {
    OpenUrl {
        url: String,
    },
    Delay {
        delay_ms: u32,
    },
    SimpleIf {
        condition: String,
        if_steps: Vec<LowCodePluginStep>,
        else_steps: Vec<LowCodePluginStep>,
    },
    StateStorageRead {
        key: String,
        default_value: String,
        output_value: String,
        output_is_empty: String,
    },
    StateStorageWrite {
        key: String,
        value: String,
    },
    MsgBox {
        title: String,
        message: String,
    },
    SelectFolder {
        prompt: String,
        output: String,
    },
    UserInput {
        prompt: String,
        default_value: String,
        multiline: bool,
        output: String,
    },
    DownloadFile {
        url: String,
        save_path: String,
        save_name: String,
        output_success: String,
    },
    ReadFileImage {
        path: String,
        output: String,
    },
    ImageInfo {
        source: String,
        width_output: String,
        height_output: String,
    },
    ImageToBase64 {
        source: String,
        output: String,
    },
    FileDelete {
        path: String,
        disabled: bool,
    },
    KeyInput {
        modifiers: String,
        key: String,
    },
    GetClipboard {
        format: LowCodeClipboardFormat,
        output: String,
    },
    WriteClipboard {
        clipboard_type: LowCodeWriteClipboardKind,
        source: String,
        alt_text: String,
    },
    RegexExtract {
        input: String,
        pattern: String,
        output: String,
    },
    StringProcess {
        input: String,
        method: LowCodeStringProcessMethod,
        output: String,
    },
    SplitString {
        input: String,
        separator: String,
        output: String,
    },
    Assign {
        expression: String,
        output: String,
    },
    StrReplace {
        input: String,
        pattern: String,
        replacement: String,
        use_regex: bool,
        output: String,
    },
    FormatString {
        template: String,
        p0: String,
        p1: String,
        p2: String,
        p3: String,
        p4: String,
        output: String,
    },
    Notify {
        message: String,
    },
    OutputText {
        content: String,
        append_return: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LowCodeClipboardFormat {
    Text,
    Html,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LowCodeWriteClipboardKind {
    Auto,
    Text,
    Html,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LowCodeStringProcessMethod {
    ToLower,
    UrlEncode,
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

#[derive(Debug, Clone, Default)]
pub struct ActionExecutionControl {
    cancelled: Arc<AtomicBool>,
}

impl ActionExecutionControl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

impl Action {
    pub fn to_quicker_plugin_json(&self) -> Result<String, String> {
        let quicker_json = match &self.kind {
            ActionKind::PluginPipeline { plugin } => plugin.to_quicker_json()?,
            _ => return Err("Only plugin pipeline actions can be exported as Quicker JSON".into()),
        };
        let document = parse_quicker_action_document(&quicker_json)?;
        serde_json::to_string_pretty(&document)
            .map_err(|err| format!("Failed to serialize Quicker plugin JSON: {err}"))
    }

    pub fn from_quicker_plugin_json(input: &str) -> Result<Self, String> {
        let document = parse_quicker_action_document(input)?;

        match document.action_type {
            QUICKER_KEYS_ACTION_TYPE => {
                if document.data_text().trim().is_empty() {
                    return Err("Quicker key macro action is missing Data".into());
                }
            }
            QUICKER_OPEN_ACTION_TYPE => {
                document.launch_payload()?;
            }
            QUICKER_PLUGIN_ACTION_TYPE => {
                if !document.use_template.unwrap_or(false) && document.has_data() {
                    document.data_payload()?;
                }
            }
            action_type => {
                return Err(format!(
                    "Unsupported Quicker action type {action_type}. Supported sample types are 7, 11, and 24."
                ));
            }
        }

        let quicker_json = serde_json::to_string_pretty(&document)
            .map_err(|err| format!("Failed to serialize Quicker plugin JSON: {err}"))?;

        Ok(Self {
            name: document.title.clone(),
            description: document.description.clone(),
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
        self.execute_with_control(None)
    }

    pub fn execute_with_control(&self, control: Option<&ActionExecutionControl>) -> ExecResult {
        if let Err(err) = ensure_not_cancelled(control) {
            return ExecResult::Err(err);
        }

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

            ActionKind::RunShell { script, shell } => run_shell_command(script, shell, control),

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
                run_shell_command(&clipboard_text, shell, control)
            }

            ActionKind::PluginPipeline { plugin } => {
                execute_quicker_action_document(&plugin.quicker_json, control)
            }

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

impl LowCodePluginDraft {
    pub fn from_quicker_plugin_json(input: &str) -> Result<Self, String> {
        let document = parse_quicker_action_document(input)?;

        match document.action_type {
            QUICKER_KEYS_ACTION_TYPE => {
                let key_macro_steps = parse_quicker_key_macro_script(document.data_text())?;
                Ok(Self {
                    kind: LowCodePluginKind::KeyMacro,
                    title: document.title,
                    description: document.description,
                    icon: document.icon,
                    key_macro_steps,
                    launch_path: String::new(),
                    launch_arguments: String::new(),
                    launch_set_working_dir: false,
                    steps: Vec::new(),
                })
            }
            QUICKER_OPEN_ACTION_TYPE => {
                let launch = document.launch_payload()?;
                Ok(Self {
                    kind: LowCodePluginKind::OpenApp,
                    title: document.title,
                    description: document.description,
                    icon: document.icon,
                    key_macro_steps: Vec::new(),
                    launch_path: launch.file_name,
                    launch_arguments: launch.arguments,
                    launch_set_working_dir: launch.set_working_dir,
                    steps: Vec::new(),
                })
            }
            QUICKER_PLUGIN_ACTION_TYPE => {
                if document.use_template.unwrap_or(false) && !document.has_data() {
                    return Err(
                        "Template-based Quicker actions cannot be opened in the low-code editor because the template body is not embedded"
                            .into(),
                    );
                }

                let data = document.data_payload()?;
                let steps = data
                    .steps
                    .iter()
                    .map(low_code_step_from_document)
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(Self {
                    kind: LowCodePluginKind::PluginFlow,
                    title: document.title,
                    description: document.description,
                    icon: document.icon,
                    key_macro_steps: Vec::new(),
                    launch_path: String::new(),
                    launch_arguments: String::new(),
                    launch_set_working_dir: false,
                    steps,
                })
            }
            action_type => Err(format!(
                "Unsupported Quicker action type {action_type}. Supported sample types are 7, 11, and 24."
            )),
        }
    }

    pub fn to_quicker_json(&self) -> Result<String, String> {
        let document = match self.kind {
            LowCodePluginKind::KeyMacro => QuickerActionDocument {
                row: Some(0),
                col: Some(0),
                action_type: QUICKER_KEYS_ACTION_TYPE,
                title: self.title.clone(),
                description: self.description.clone(),
                icon: self.icon.clone(),
                path: None,
                delay_ms: Some(0),
                data: Some(serialize_quicker_key_macro_steps(&self.key_macro_steps)?),
                data2: None,
                data3: None,
                children: None,
                id: None,
                template_id: None,
                template_revision: Some(0),
                use_template: Some(false),
                last_edit_time_utc: None,
                shared_action_id: None,
                share_time_utc: None,
                create_time_utc: None,
                as_sub_program: Some(false),
                skip_when_stop_running_actions: Some(false),
                skip_check_update: Some(false),
                auto_update: Some(false),
                keep_info_when_update: Some(false),
                min_quicker_version: None,
                context_menu_data: None,
                allow_scroll_trigger: Some(false),
                enable_evaluate_variable: Some(true),
                is_text_processor: Some(false),
                is_image_processor: Some(false),
                association: None,
                do_not_close_panel: None,
                user_limitation: None,
            },
            LowCodePluginKind::OpenApp => QuickerActionDocument {
                row: Some(0),
                col: Some(0),
                action_type: QUICKER_OPEN_ACTION_TYPE,
                title: self.title.clone(),
                description: self.description.clone(),
                icon: self.icon.clone(),
                path: None,
                delay_ms: Some(0),
                data: Some(format!(
                    "json:{}",
                    serde_json::to_string(&QuickerLaunchData {
                        file_name: self.launch_path.clone(),
                        arguments: self.launch_arguments.clone(),
                        run_as_admin: false,
                        wait_for_exit: false,
                        window_style: None,
                        set_working_dir: self.launch_set_working_dir,
                        alternative_paths: String::new(),
                    })
                    .map_err(|err| format!("Failed to serialize launcher payload: {err}"))?
                )),
                data2: Some(String::new()),
                data3: Some(String::new()),
                children: None,
                id: None,
                template_id: None,
                template_revision: Some(0),
                use_template: Some(false),
                last_edit_time_utc: None,
                shared_action_id: None,
                share_time_utc: None,
                create_time_utc: None,
                as_sub_program: Some(false),
                skip_when_stop_running_actions: Some(false),
                skip_check_update: Some(false),
                auto_update: Some(false),
                keep_info_when_update: Some(false),
                min_quicker_version: None,
                context_menu_data: None,
                allow_scroll_trigger: Some(false),
                enable_evaluate_variable: Some(true),
                is_text_processor: Some(false),
                is_image_processor: Some(false),
                association: None,
                do_not_close_panel: None,
                user_limitation: None,
            },
            LowCodePluginKind::PluginFlow => {
                let mut variable_names = BTreeSet::new();
                let steps = self
                    .steps
                    .iter()
                    .map(|step| step.to_step_document(&mut variable_names))
                    .collect::<Result<Vec<_>, _>>()?;

                let variables = variable_names
                    .into_iter()
                    .map(|name| QuickerPluginVariable {
                        key: name,
                        value_type: Some(0),
                        default_value: Some(String::new()),
                        save_state: Some(false),
                    })
                    .collect();

                let data = QuickerPluginData {
                    limit_single_instance: false,
                    summary_expression: Some(String::new()),
                    sub_programs: Vec::new(),
                    variables,
                    steps,
                };

                QuickerActionDocument {
                    row: Some(0),
                    col: Some(0),
                    action_type: QUICKER_PLUGIN_ACTION_TYPE,
                    title: self.title.clone(),
                    description: self.description.clone(),
                    icon: self.icon.clone(),
                    path: None,
                    delay_ms: Some(0),
                    data: Some(
                        serde_json::to_string(&data)
                            .map_err(|err| format!("Failed to serialize plugin data: {err}"))?,
                    ),
                    data2: Some(String::new()),
                    data3: Some(String::new()),
                    children: None,
                    id: None,
                    template_id: None,
                    template_revision: Some(0),
                    use_template: Some(false),
                    last_edit_time_utc: None,
                    shared_action_id: None,
                    share_time_utc: None,
                    create_time_utc: None,
                    as_sub_program: Some(false),
                    skip_when_stop_running_actions: Some(false),
                    skip_check_update: Some(false),
                    auto_update: Some(false),
                    keep_info_when_update: Some(false),
                    min_quicker_version: Some(String::new()),
                    context_menu_data: Some(String::new()),
                    allow_scroll_trigger: Some(false),
                    enable_evaluate_variable: Some(true),
                    is_text_processor: Some(false),
                    is_image_processor: Some(false),
                    association: Some(QuickerAssociation {
                        match_process: None,
                        is_image_processor: Some(false),
                        return_image_from_first_screen_shot_step: Some(true),
                        is_text_processor: Some(false),
                        return_text_from_get_selected_text_step: Some(true),
                        text_match_expression: Some(String::new()),
                        text_min_length: Some(0),
                        text_max_length: Some(0),
                        is_html_processor: Some(false),
                        is_file_processor: Some(false),
                        file_min_count: Some(0),
                        file_max_count: Some(0),
                        allowed_file_extensions: Some(String::new()),
                        require_all_file_match_ext: Some(false),
                        search_box_placeholder: Some(String::new()),
                        is_window_processor: Some(false),
                        enable_realtime_search: Some(false),
                        browser_context_menu: None,
                        url_pattern: None,
                    }),
                    do_not_close_panel: Some(false),
                    user_limitation: None,
                }
            }
        };

        serde_json::to_string_pretty(&document)
            .map_err(|err| format!("Failed to serialize Quicker plugin JSON: {err}"))
    }

    pub fn to_action(&self) -> Result<Action, String> {
        let quicker_json = self.to_quicker_json()?;
        Ok(Action {
            name: self.title.clone(),
            description: self.description.clone(),
            icon: self.icon.clone(),
            tags: Vec::new(),
            hotkey: None,
            kind: ActionKind::PluginPipeline {
                plugin: PluginPipelineStorage { quicker_json },
            },
        })
    }
}

impl LowCodePluginStep {
    pub fn label(&self) -> &'static str {
        match self {
            Self::OpenUrl { .. } => "Open URL",
            Self::Delay { .. } => "Delay",
            Self::SimpleIf { .. } => "If",
            Self::StateStorageRead { .. } => "State Read",
            Self::StateStorageWrite { .. } => "State Write",
            Self::MsgBox { .. } => "Message Box",
            Self::SelectFolder { .. } => "Select Folder",
            Self::UserInput { .. } => "User Input",
            Self::DownloadFile { .. } => "Download File",
            Self::ReadFileImage { .. } => "Read File",
            Self::ImageInfo { .. } => "Image Info",
            Self::ImageToBase64 { .. } => "Image To Base64",
            Self::FileDelete { .. } => "Delete File",
            Self::KeyInput { .. } => "Key Input",
            Self::GetClipboard { .. } => "Get Clipboard",
            Self::WriteClipboard { .. } => "Write Clipboard",
            Self::RegexExtract { .. } => "Regex Extract",
            Self::StringProcess { .. } => "String Process",
            Self::SplitString { .. } => "Split String",
            Self::Assign { .. } => "Assign",
            Self::StrReplace { .. } => "Replace Text",
            Self::FormatString { .. } => "Format String",
            Self::Notify { .. } => "Notify",
            Self::OutputText { .. } => "Output Text",
        }
    }

    fn to_step_document(
        &self,
        variable_names: &mut BTreeSet<String>,
    ) -> Result<QuickerPluginStepDocument, String> {
        match self {
            Self::OpenUrl { url } => Ok(step_document(
                "sys:openUrl",
                map_with_binding([("url", url.as_str())]),
                Map::new(),
            )),
            Self::Delay { delay_ms } => Ok(step_document(
                "sys:delay",
                map_with_binding([("delayMs", &delay_ms.to_string())]),
                Map::new(),
            )),
            Self::SimpleIf {
                condition,
                if_steps,
                else_steps,
            } => Ok(QuickerPluginStepDocument {
                step_runner_key: "sys:simpleIf".into(),
                input_params: map_with_binding([("condition", condition.as_str())]),
                output_params: Map::new(),
                if_steps: Some(
                    if_steps
                        .iter()
                        .map(|step| step.to_step_document(variable_names))
                        .collect::<Result<Vec<_>, _>>()?,
                ),
                else_steps: (!else_steps.is_empty())
                    .then(|| {
                        else_steps
                            .iter()
                            .map(|step| step.to_step_document(variable_names))
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?,
                note: None,
                disabled: false,
                collapsed: false,
                delay_ms: 0,
            }),
            Self::StateStorageRead {
                key,
                default_value,
                output_value,
                output_is_empty,
            } => {
                track_variable_name(variable_names, output_value);
                track_variable_name(variable_names, output_is_empty);
                Ok(step_document(
                    "sys:stateStorage",
                    map_with_binding([
                        ("type", "readActionState"),
                        ("key", key.as_str()),
                        ("defaultValue", default_value.as_str()),
                        ("inputIfEmpty", "0"),
                        ("prompt", ""),
                    ]),
                    map_with_output([
                        ("isSuccess", ""),
                        ("value", output_value.as_str()),
                        ("isEmpty", output_is_empty.as_str()),
                    ]),
                ))
            }
            Self::StateStorageWrite { key, value } => Ok(step_document(
                "sys:stateStorage",
                map_with_binding([
                    ("type", "saveActionState"),
                    ("key", key.as_str()),
                    ("value", value.as_str()),
                ]),
                Map::new(),
            )),
            Self::MsgBox { title, message } => Ok(step_document(
                "sys:MsgBox",
                map_with_binding([
                    ("message", message.as_str()),
                    ("title", title.as_str()),
                    ("icon", "Asterisk"),
                    ("buttons", "OK"),
                ]),
                Map::new(),
            )),
            Self::SelectFolder { prompt, output } => {
                track_variable_name(variable_names, output);
                Ok(step_document(
                    "sys:selectFolder",
                    map_with_binding([
                        ("prompt", prompt.as_str()),
                        ("initDir", ""),
                        ("showOpenedDirs", "1"),
                        ("stopIfFail", "1"),
                    ]),
                    map_with_output([("isSuccess", ""), ("path", output.as_str())]),
                ))
            }
            Self::UserInput {
                prompt,
                default_value,
                multiline,
                output,
            } => {
                track_variable_name(variable_names, output);
                Ok(step_document(
                    "sys:userInput",
                    map_with_binding([
                        ("type", if *multiline { "multiline" } else { "text" }),
                        ("prompt", prompt.as_str()),
                        ("defaultValue", default_value.as_str()),
                        ("texttools", ""),
                        ("pattern", ""),
                        ("helpLink", ""),
                        ("isRequired", "0"),
                        ("fontfamily", ""),
                        ("winLocation", "CenterScreen"),
                        ("imeState", "NO_CONTROL"),
                        ("submitWithReturn", "0"),
                        ("restoreFocus", "1"),
                        ("closeOnDeactivated", "0"),
                        ("stopIfFail", "1"),
                    ]),
                    map_with_output([
                        ("isSuccess", ""),
                        ("textValue", output.as_str()),
                        ("isEmpty", ""),
                    ]),
                ))
            }
            Self::DownloadFile {
                url,
                save_path,
                save_name,
                output_success,
            } => {
                track_variable_name(variable_names, output_success);
                Ok(step_document(
                    "sys:download",
                    map_with_binding([
                        ("url", url.as_str()),
                        ("savePath", save_path.as_str()),
                        ("saveName", save_name.as_str()),
                        ("ua", ""),
                        ("header", ""),
                        ("cookie", ""),
                        ("showProgress", "0"),
                        ("stopIfFail", "1"),
                    ]),
                    map_with_output([("isSuccess", output_success.as_str()), ("savedPath", "")]),
                ))
            }
            Self::ReadFileImage { path, output } => {
                track_variable_name(variable_names, output);
                Ok(step_document(
                    "sys:readFile",
                    map_with_binding([
                        ("path", path.as_str()),
                        ("type", "image"),
                        ("stopIfFail", "1"),
                    ]),
                    map_with_output([("image", output.as_str()), ("isSuccess", "")]),
                ))
            }
            Self::ImageInfo {
                source,
                width_output,
                height_output,
            } => {
                track_variable_name(variable_names, width_output);
                track_variable_name(variable_names, height_output);
                Ok(step_document(
                    "sys:imageinfo",
                    map_with_binding([("sourceType", "var"), ("bmpVar", source.as_str())]),
                    map_with_output([
                        ("width", width_output.as_str()),
                        ("height", height_output.as_str()),
                        ("dateTimeOriginal", ""),
                        ("exifData", ""),
                        ("rawExifData", ""),
                    ]),
                ))
            }
            Self::ImageToBase64 { source, output } => {
                track_variable_name(variable_names, output);
                Ok(step_document(
                    "sys:imgToBase64",
                    map_with_binding([("type", "imgToBase64"), ("img", source.as_str())]),
                    map_with_output([("code", output.as_str())]),
                ))
            }
            Self::FileDelete { path, disabled } => {
                let mut document = step_document(
                    "sys:fileOperation",
                    map_with_binding([
                        ("type", "deleteFile"),
                        ("path", path.as_str()),
                        ("stopIfFail", "1"),
                    ]),
                    map_with_output([("isSuccess", "")]),
                );
                document.disabled = *disabled;
                Ok(document)
            }
            Self::KeyInput { modifiers, key } => Ok(step_document(
                "sys:keyInput",
                map_with_binding([(
                    "keys",
                    &serde_json::to_string(&QuickerKeyInput {
                        ctrl_keys: parse_low_code_modifiers(modifiers)?,
                        keys: vec![parse_low_code_key(key)?],
                    })
                    .map_err(|err| format!("Failed to serialize keyInput payload: {err}"))?,
                )]),
                Map::new(),
            )),
            Self::GetClipboard { format, output } => {
                track_variable_name(variable_names, output);
                Ok(step_document(
                    "sys:getClipboardText",
                    {
                        let mut input = map_with_binding([(
                            "format",
                            match format {
                                LowCodeClipboardFormat::Text => "UnicodeText",
                                LowCodeClipboardFormat::Html => "Html",
                            },
                        )]);
                        input.insert("stopIfFail".into(), low_code_literal("1"));
                        input
                    },
                    map_with_output([("output", output.as_str()), ("isSuccess", "")]),
                ))
            }
            Self::WriteClipboard {
                clipboard_type,
                source,
                alt_text,
            } => {
                let type_name = match clipboard_type {
                    LowCodeWriteClipboardKind::Auto => "auto",
                    LowCodeWriteClipboardKind::Text => "text",
                    LowCodeWriteClipboardKind::Html => "html",
                };
                let mut input = map_with_binding([("type", type_name)]);
                match clipboard_type {
                    LowCodeWriteClipboardKind::Text => {
                        input.insert("text".into(), low_code_binding(source));
                    }
                    LowCodeWriteClipboardKind::Html => {
                        input.insert("html".into(), low_code_binding(source));
                        input.insert("text".into(), low_code_binding(alt_text));
                    }
                    LowCodeWriteClipboardKind::Auto => {
                        input.insert("input".into(), low_code_binding(source));
                    }
                }
                Ok(step_document("sys:writeClipboard", input, Map::new()))
            }
            Self::RegexExtract {
                input,
                pattern,
                output,
            } => {
                track_variable_name(variable_names, output);
                let mut input_params =
                    map_with_binding([("data", input.as_str()), ("pattern", pattern.as_str())]);
                input_params.insert("getGroup".into(), low_code_literal("0"));
                input_params.insert("stopIfFail".into(), low_code_literal("1"));
                Ok(step_document(
                    "sys:regexExtract",
                    input_params,
                    map_with_output([("match1", output.as_str()), ("isSuccess", "")]),
                ))
            }
            Self::StringProcess {
                input,
                method,
                output,
            } => {
                track_variable_name(variable_names, output);
                Ok(step_document(
                    "sys:stringProcess",
                    map_with_binding([
                        ("data", input.as_str()),
                        (
                            "method",
                            match method {
                                LowCodeStringProcessMethod::ToLower => "toLower",
                                LowCodeStringProcessMethod::UrlEncode => "urlEncode",
                            },
                        ),
                    ]),
                    map_with_output([("output", output.as_str()), ("isSuccess", "")]),
                ))
            }
            Self::SplitString {
                input,
                separator,
                output,
            } => {
                track_variable_name(variable_names, output);
                let mut input_params =
                    map_with_binding([("data", input.as_str()), ("separator", separator.as_str())]);
                input_params.insert("escapeSeparator".into(), low_code_literal("1"));
                input_params.insert("removeEmpty".into(), low_code_literal("1"));
                Ok(step_document(
                    "sys:splitString",
                    input_params,
                    map_with_output([("output", output.as_str())]),
                ))
            }
            Self::Assign { expression, output } => {
                track_variable_name(variable_names, output);
                let mut input_params = map_with_binding([("input", expression.as_str())]);
                input_params.insert("stopIfFail".into(), low_code_literal("1"));
                Ok(step_document(
                    "sys:assign",
                    input_params,
                    map_with_output([("output", output.as_str()), ("isSuccess", "")]),
                ))
            }
            Self::StrReplace {
                input,
                pattern,
                replacement,
                use_regex,
                output,
            } => {
                track_variable_name(variable_names, output);
                let mut input_params = map_with_binding([
                    ("input", input.as_str()),
                    ("old", pattern.as_str()),
                    ("new", replacement.as_str()),
                ]);
                input_params.insert(
                    "useRegex".into(),
                    low_code_literal(if *use_regex { "1" } else { "0" }),
                );
                input_params.insert("replaceEscapes".into(), low_code_literal("1"));
                Ok(step_document(
                    "sys:strReplace",
                    input_params,
                    map_with_output([("output", output.as_str())]),
                ))
            }
            Self::FormatString {
                template,
                p0,
                p1,
                p2,
                p3,
                p4,
                output,
            } => {
                track_variable_name(variable_names, output);
                Ok(step_document(
                    "sys:formatString",
                    map_with_binding([
                        ("formatString", template.as_str()),
                        ("p0", p0.as_str()),
                        ("p1", p1.as_str()),
                        ("p2", p2.as_str()),
                        ("p3", p3.as_str()),
                        ("p4", p4.as_str()),
                    ]),
                    map_with_output([("output", output.as_str())]),
                ))
            }
            Self::Notify { message } => Ok(step_document(
                "sys:notify",
                map_with_binding([("msg", message.as_str())]),
                Map::new(),
            )),
            Self::OutputText {
                content,
                append_return,
            } => {
                let mut input_params = map_with_binding([("content", content.as_str())]);
                input_params.insert("method".into(), low_code_literal("paste"));
                input_params.insert(
                    "appendReturn".into(),
                    low_code_literal(if *append_return { "1" } else { "0" }),
                );
                Ok(step_document("sys:outputText", input_params, Map::new()))
            }
        }
    }
}

impl LowCodeKeyMacroStep {
    pub fn label(&self) -> &'static str {
        match self {
            Self::SendKeys { .. } => "Send Keys",
            Self::TypeText { .. } => "Type Text",
            Self::Delay { .. } => "Delay",
        }
    }
}

impl PluginPipelineStorage {
    fn to_quicker_json(&self) -> Result<String, String> {
        let document = parse_quicker_action_document(&self.quicker_json)?;
        serde_json::to_string_pretty(&document)
            .map_err(|err| format!("Failed to serialize Quicker plugin JSON: {err}"))
    }
}

const QUICKER_KEYS_ACTION_TYPE: u32 = 7;
const QUICKER_OPEN_ACTION_TYPE: u32 = 11;
const QUICKER_PLUGIN_ACTION_TYPE: u32 = 24;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct QuickerActionDocument {
    #[serde(default)]
    row: Option<u32>,
    #[serde(default)]
    col: Option<u32>,
    action_type: u32,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    delay_ms: Option<u32>,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    data2: Option<String>,
    #[serde(default)]
    data3: Option<String>,
    #[serde(default)]
    children: Option<Value>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    template_id: Option<String>,
    #[serde(default)]
    template_revision: Option<u32>,
    #[serde(default)]
    use_template: Option<bool>,
    #[serde(default)]
    last_edit_time_utc: Option<String>,
    #[serde(default)]
    shared_action_id: Option<String>,
    #[serde(default)]
    share_time_utc: Option<String>,
    #[serde(default)]
    create_time_utc: Option<String>,
    #[serde(default)]
    as_sub_program: Option<bool>,
    #[serde(default)]
    skip_when_stop_running_actions: Option<bool>,
    #[serde(default)]
    skip_check_update: Option<bool>,
    #[serde(default)]
    auto_update: Option<bool>,
    #[serde(default)]
    keep_info_when_update: Option<bool>,
    #[serde(default)]
    min_quicker_version: Option<String>,
    #[serde(default)]
    context_menu_data: Option<String>,
    #[serde(default)]
    allow_scroll_trigger: Option<bool>,
    #[serde(default)]
    enable_evaluate_variable: Option<bool>,
    #[serde(default)]
    is_text_processor: Option<bool>,
    #[serde(default)]
    is_image_processor: Option<bool>,
    #[serde(default)]
    association: Option<QuickerAssociation>,
    #[serde(default)]
    do_not_close_panel: Option<bool>,
    #[serde(default)]
    user_limitation: Option<Value>,
}

impl QuickerActionDocument {
    fn has_data(&self) -> bool {
        !self.data_text().trim().is_empty()
    }

    fn data_text(&self) -> &str {
        self.data.as_deref().unwrap_or("")
    }

    fn data_payload(&self) -> Result<QuickerPluginData, String> {
        parse_json_lenient(
            self.data_text(),
            "Failed to parse Quicker plugin data payload",
        )
    }

    fn launch_payload(&self) -> Result<QuickerLaunchData, String> {
        let payload = self
            .data_text()
            .strip_prefix("json:")
            .unwrap_or(self.data_text());
        parse_json_lenient(payload, "Failed to parse Quicker launcher payload")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct QuickerPluginData {
    #[serde(default)]
    limit_single_instance: bool,
    #[serde(default)]
    summary_expression: Option<String>,
    #[serde(default)]
    sub_programs: Vec<Value>,
    #[serde(default)]
    variables: Vec<QuickerPluginVariable>,
    #[serde(default)]
    steps: Vec<QuickerPluginStepDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
struct QuickerPluginVariable {
    key: String,
    #[serde(rename = "Type", default)]
    value_type: Option<u8>,
    #[serde(default)]
    default_value: Option<String>,
    #[serde(default)]
    save_state: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct QuickerPluginStepDocument {
    step_runner_key: String,
    #[serde(default)]
    input_params: Map<String, Value>,
    #[serde(default)]
    output_params: Map<String, Value>,
    #[serde(default)]
    if_steps: Option<Vec<QuickerPluginStepDocument>>,
    #[serde(default)]
    else_steps: Option<Vec<QuickerPluginStepDocument>>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    collapsed: bool,
    #[serde(default)]
    delay_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
struct QuickerAssociation {
    #[serde(default)]
    match_process: Option<String>,
    #[serde(default)]
    is_image_processor: Option<bool>,
    #[serde(default)]
    return_image_from_first_screen_shot_step: Option<bool>,
    #[serde(default)]
    is_text_processor: Option<bool>,
    #[serde(default)]
    return_text_from_get_selected_text_step: Option<bool>,
    #[serde(default)]
    text_match_expression: Option<String>,
    #[serde(default)]
    text_min_length: Option<u32>,
    #[serde(default)]
    text_max_length: Option<u32>,
    #[serde(default)]
    is_html_processor: Option<bool>,
    #[serde(default)]
    is_file_processor: Option<bool>,
    #[serde(default)]
    file_min_count: Option<u32>,
    #[serde(default)]
    file_max_count: Option<u32>,
    #[serde(default)]
    allowed_file_extensions: Option<String>,
    #[serde(default)]
    require_all_file_match_ext: Option<bool>,
    #[serde(default)]
    search_box_placeholder: Option<String>,
    #[serde(default)]
    is_window_processor: Option<bool>,
    #[serde(default)]
    enable_realtime_search: Option<bool>,
    #[serde(default)]
    browser_context_menu: Option<Value>,
    #[serde(default)]
    url_pattern: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
struct QuickerLaunchData {
    file_name: String,
    #[serde(default)]
    arguments: String,
    #[serde(default)]
    run_as_admin: bool,
    #[serde(default)]
    wait_for_exit: bool,
    #[serde(default)]
    window_style: Option<String>,
    #[serde(default)]
    set_working_dir: bool,
    #[serde(default)]
    alternative_paths: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct QuickerKeyInput {
    #[serde(rename = "CtrlKeys", default)]
    ctrl_keys: Vec<u32>,
    #[serde(rename = "Keys", default)]
    keys: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct QuickerValueBinding {
    #[serde(default)]
    var_key: Option<String>,
    #[serde(default)]
    value: Option<Value>,
}

fn step_document(
    runner: &str,
    input_params: Map<String, Value>,
    output_params: Map<String, Value>,
) -> QuickerPluginStepDocument {
    QuickerPluginStepDocument {
        step_runner_key: runner.into(),
        input_params,
        output_params,
        if_steps: None,
        else_steps: None,
        note: None,
        disabled: false,
        collapsed: false,
        delay_ms: 0,
    }
}

fn map_with_binding<const N: usize>(pairs: [(&str, &str); N]) -> Map<String, Value> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), low_code_binding(value)))
        .collect()
}

fn map_with_output<const N: usize>(pairs: [(&str, &str); N]) -> Map<String, Value> {
    pairs
        .into_iter()
        .map(|(key, value)| {
            (
                key.to_string(),
                if value.trim().is_empty() {
                    Value::Null
                } else {
                    Value::String(value.to_string())
                },
            )
        })
        .collect()
}

fn low_code_binding(value: &str) -> Value {
    if let Some(var_name) = value.strip_prefix('$') {
        serde_json::json!({
            "VarKey": var_name.trim(),
            "Value": Value::Null,
        })
    } else {
        low_code_literal(value)
    }
}

fn low_code_literal(value: &str) -> Value {
    serde_json::json!({
        "VarKey": Value::Null,
        "Value": value,
    })
}

fn binding_string(params: &Map<String, Value>, key: &str) -> Option<String> {
    let raw = params.get(key)?;
    let binding: QuickerValueBinding = serde_json::from_value(raw.clone()).ok()?;
    if let Some(var_key) = binding.var_key {
        return Some(format!("${var_key}"));
    }

    binding.value.map(|value| value_to_string(&value))
}

fn binding_bool(params: &Map<String, Value>, key: &str) -> bool {
    let Some(raw) = params.get(key) else {
        return false;
    };
    let Ok(binding) = serde_json::from_value::<QuickerValueBinding>(raw.clone()) else {
        return false;
    };
    binding
        .value
        .as_ref()
        .map(|value| truthy(Some(value)))
        .unwrap_or(false)
}

fn low_code_step_from_document(
    step: &QuickerPluginStepDocument,
) -> Result<LowCodePluginStep, String> {
    match step.step_runner_key.as_str() {
        "sys:openUrl" => Ok(LowCodePluginStep::OpenUrl {
            url: binding_string(&step.input_params, "url").unwrap_or_default(),
        }),
        "sys:delay" => Ok(LowCodePluginStep::Delay {
            delay_ms: binding_string(&step.input_params, "delayMs")
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(0),
        }),
        "sys:simpleIf" => Ok(LowCodePluginStep::SimpleIf {
            condition: binding_string(&step.input_params, "condition").unwrap_or_default(),
            if_steps: step
                .if_steps
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .map(low_code_step_from_document)
                .collect::<Result<Vec<_>, _>>()?,
            else_steps: step
                .else_steps
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .map(low_code_step_from_document)
                .collect::<Result<Vec<_>, _>>()?,
        }),
        "sys:stateStorage" => match binding_string(&step.input_params, "type").as_deref() {
            Some("readActionState") => Ok(LowCodePluginStep::StateStorageRead {
                key: binding_string(&step.input_params, "key").unwrap_or_default(),
                default_value: binding_string(&step.input_params, "defaultValue")
                    .unwrap_or_default(),
                output_value: output_var_name(&step.output_params, "value").unwrap_or_default(),
                output_is_empty: output_var_name(&step.output_params, "isEmpty")
                    .unwrap_or_default(),
            }),
            Some("saveActionState") => Ok(LowCodePluginStep::StateStorageWrite {
                key: binding_string(&step.input_params, "key").unwrap_or_default(),
                value: binding_string(&step.input_params, "value").unwrap_or_default(),
            }),
            other => Err(format!(
                "The low-code editor does not support stateStorage type {:?} yet.",
                other
            )),
        },
        "sys:MsgBox" => Ok(LowCodePluginStep::MsgBox {
            title: binding_string(&step.input_params, "title").unwrap_or_default(),
            message: binding_string(&step.input_params, "message").unwrap_or_default(),
        }),
        "sys:selectFolder" => Ok(LowCodePluginStep::SelectFolder {
            prompt: binding_string(&step.input_params, "prompt").unwrap_or_default(),
            output: output_var_name(&step.output_params, "path").unwrap_or_default(),
        }),
        "sys:userInput" => Ok(LowCodePluginStep::UserInput {
            prompt: binding_string(&step.input_params, "prompt").unwrap_or_default(),
            default_value: binding_string(&step.input_params, "defaultValue").unwrap_or_default(),
            multiline: matches!(
                binding_string(&step.input_params, "type").as_deref(),
                Some("multiline")
            ),
            output: output_var_name(&step.output_params, "textValue").unwrap_or_default(),
        }),
        "sys:download" => Ok(LowCodePluginStep::DownloadFile {
            url: binding_string(&step.input_params, "url").unwrap_or_default(),
            save_path: binding_string(&step.input_params, "savePath").unwrap_or_default(),
            save_name: binding_string(&step.input_params, "saveName").unwrap_or_default(),
            output_success: output_var_name(&step.output_params, "isSuccess").unwrap_or_default(),
        }),
        "sys:readFile" => match binding_string(&step.input_params, "type").as_deref() {
            Some("image") => Ok(LowCodePluginStep::ReadFileImage {
                path: binding_string(&step.input_params, "path").unwrap_or_default(),
                output: output_var_name(&step.output_params, "image").unwrap_or_default(),
            }),
            other => Err(format!(
                "The low-code editor does not support readFile type {:?} yet.",
                other
            )),
        },
        "sys:imageinfo" => Ok(LowCodePluginStep::ImageInfo {
            source: binding_string(&step.input_params, "bmpVar").unwrap_or_default(),
            width_output: output_var_name(&step.output_params, "width").unwrap_or_default(),
            height_output: output_var_name(&step.output_params, "height").unwrap_or_default(),
        }),
        "sys:imgToBase64" => Ok(LowCodePluginStep::ImageToBase64 {
            source: binding_string(&step.input_params, "img").unwrap_or_default(),
            output: output_var_name(&step.output_params, "code").unwrap_or_default(),
        }),
        "sys:fileOperation" => match binding_string(&step.input_params, "type").as_deref() {
            Some("deleteFile") => Ok(LowCodePluginStep::FileDelete {
                path: binding_string(&step.input_params, "path").unwrap_or_default(),
                disabled: step.disabled,
            }),
            other => Err(format!(
                "The low-code editor does not support fileOperation type {:?} yet.",
                other
            )),
        },
        "sys:keyInput" => {
            let payload: QuickerKeyInput = serde_json::from_str(
                &binding_string(&step.input_params, "keys").unwrap_or_default(),
            )
            .map_err(|err| format!("Failed to parse keyInput payload: {err}"))?;
            Ok(LowCodePluginStep::KeyInput {
                modifiers: payload
                    .ctrl_keys
                    .into_iter()
                    .filter_map(low_code_modifier_name)
                    .collect::<Vec<_>>()
                    .join("+"),
                key: payload
                    .keys
                    .first()
                    .and_then(|code| low_code_key_name(*code))
                    .unwrap_or_default(),
            })
        }
        "sys:getClipboardText" => Ok(LowCodePluginStep::GetClipboard {
            format: match binding_string(&step.input_params, "format").as_deref() {
                Some("Html") => LowCodeClipboardFormat::Html,
                _ => LowCodeClipboardFormat::Text,
            },
            output: output_var_name(&step.output_params, "output").unwrap_or_default(),
        }),
        "sys:writeClipboard" => Ok(LowCodePluginStep::WriteClipboard {
            clipboard_type: match binding_string(&step.input_params, "type")
                .unwrap_or_else(|| "auto".into())
                .to_ascii_lowercase()
                .as_str()
            {
                "text" => LowCodeWriteClipboardKind::Text,
                "html" => LowCodeWriteClipboardKind::Html,
                _ => LowCodeWriteClipboardKind::Auto,
            },
            source: binding_string(&step.input_params, "html")
                .or_else(|| binding_string(&step.input_params, "text"))
                .or_else(|| binding_string(&step.input_params, "input"))
                .unwrap_or_default(),
            alt_text: binding_string(&step.input_params, "text").unwrap_or_default(),
        }),
        "sys:regexExtract" => Ok(LowCodePluginStep::RegexExtract {
            input: binding_string(&step.input_params, "data").unwrap_or_default(),
            pattern: binding_string(&step.input_params, "pattern").unwrap_or_default(),
            output: output_var_name(&step.output_params, "match1")
                .or_else(|| output_var_name(&step.output_params, "output"))
                .or_else(|| output_var_name(&step.output_params, "matches"))
                .unwrap_or_default(),
        }),
        "sys:stringProcess" => Ok(LowCodePluginStep::StringProcess {
            input: binding_string(&step.input_params, "data").unwrap_or_default(),
            method: match binding_string(&step.input_params, "method").as_deref() {
                Some("urlEncode") => LowCodeStringProcessMethod::UrlEncode,
                _ => LowCodeStringProcessMethod::ToLower,
            },
            output: output_var_name(&step.output_params, "output").unwrap_or_default(),
        }),
        "sys:splitString" => Ok(LowCodePluginStep::SplitString {
            input: binding_string(&step.input_params, "data").unwrap_or_default(),
            separator: binding_string(&step.input_params, "separator").unwrap_or_default(),
            output: output_var_name(&step.output_params, "output").unwrap_or_default(),
        }),
        "sys:assign" => Ok(LowCodePluginStep::Assign {
            expression: binding_string(&step.input_params, "input").unwrap_or_default(),
            output: output_var_name(&step.output_params, "output").unwrap_or_default(),
        }),
        "sys:strReplace" => Ok(LowCodePluginStep::StrReplace {
            input: binding_string(&step.input_params, "input").unwrap_or_default(),
            pattern: binding_string(&step.input_params, "old").unwrap_or_default(),
            replacement: binding_string(&step.input_params, "new").unwrap_or_default(),
            use_regex: binding_bool(&step.input_params, "useRegex"),
            output: output_var_name(&step.output_params, "output").unwrap_or_default(),
        }),
        "sys:formatString" => Ok(LowCodePluginStep::FormatString {
            template: binding_string(&step.input_params, "formatString").unwrap_or_default(),
            p0: binding_string(&step.input_params, "p0").unwrap_or_default(),
            p1: binding_string(&step.input_params, "p1").unwrap_or_default(),
            p2: binding_string(&step.input_params, "p2").unwrap_or_default(),
            p3: binding_string(&step.input_params, "p3").unwrap_or_default(),
            p4: binding_string(&step.input_params, "p4").unwrap_or_default(),
            output: output_var_name(&step.output_params, "output").unwrap_or_default(),
        }),
        "sys:notify" => Ok(LowCodePluginStep::Notify {
            message: binding_string(&step.input_params, "msg").unwrap_or_default(),
        }),
        "sys:outputText" => Ok(LowCodePluginStep::OutputText {
            content: binding_string(&step.input_params, "content").unwrap_or_default(),
            append_return: binding_bool(&step.input_params, "appendReturn"),
        }),
        other => Err(format!(
            "The low-code editor does not support step '{}' yet. Use the raw JSON path for that action.",
            other
        )),
    }
}

fn track_variable_name(variable_names: &mut BTreeSet<String>, name: &str) {
    let trimmed = name.trim();
    if !trimmed.is_empty() {
        variable_names.insert(trimmed.to_string());
    }
}

fn parse_low_code_modifiers(value: &str) -> Result<Vec<u32>, String> {
    value
        .split(['+', ',', ' '])
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| match token.to_ascii_lowercase().as_str() {
            "shift" => Ok(160),
            "ctrl" | "control" => Ok(162),
            "alt" => Ok(164),
            "super" | "win" | "meta" => Ok(91),
            _ => Err(format!("Unsupported low-code modifier: {token}")),
        })
        .collect()
}

fn parse_low_code_key(value: &str) -> Result<u32, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "backspace" | "back" => Ok(8),
        "tab" => Ok(9),
        "return" | "enter" => Ok(13),
        "escape" | "esc" => Ok(27),
        "space" => Ok(32),
        "left" => Ok(37),
        "up" => Ok(38),
        "right" => Ok(39),
        "down" => Ok(40),
        key if key.len() == 1 => {
            let ch = key.chars().next().unwrap();
            if ch.is_ascii_digit() {
                Ok(ch as u32)
            } else if ch.is_ascii_alphabetic() {
                Ok(ch.to_ascii_uppercase() as u32)
            } else {
                Err(format!("Unsupported low-code key: {value}"))
            }
        }
        _ => Err(format!("Unsupported low-code key: {value}")),
    }
}

fn low_code_modifier_name(code: u32) -> Option<String> {
    match code {
        16 | 160 | 161 => Some("shift".into()),
        17 | 162 | 163 => Some("ctrl".into()),
        18 | 164 | 165 => Some("alt".into()),
        91 | 92 => Some("super".into()),
        _ => None,
    }
}

fn low_code_key_name(code: u32) -> Option<String> {
    Some(match code {
        8 => "Backspace".into(),
        9 => "Tab".into(),
        13 => "Return".into(),
        27 => "Escape".into(),
        32 => "Space".into(),
        37 => "Left".into(),
        38 => "Up".into(),
        39 => "Right".into(),
        40 => "Down".into(),
        48..=57 | 65..=90 => char::from_u32(code)?.to_string(),
        _ => return None,
    })
}

fn low_code_modifier_macro_token(code: u32) -> Option<&'static str> {
    match code {
        16 | 160 | 161 => Some("SHIFT"),
        17 | 162 | 163 => Some("CTRL"),
        18 | 164 | 165 => Some("ALT"),
        91 | 92 => Some("WIN"),
        _ => None,
    }
}

fn low_code_key_macro_token(code: u32) -> Option<String> {
    Some(match code {
        8 => "BACK".into(),
        9 => "TAB".into(),
        13 => "RETURN".into(),
        27 => "ESC".into(),
        32 => "SPACE".into(),
        37 => "LEFT".into(),
        38 => "UP".into(),
        39 => "RIGHT".into(),
        40 => "DOWN".into(),
        48..=57 | 65..=90 => format!("VK_{}", char::from_u32(code)?.to_ascii_uppercase()),
        _ => return None,
    })
}

fn parse_quicker_key_macro_script(script: &str) -> Result<Vec<LowCodeKeyMacroStep>, String> {
    let mut steps = Vec::new();

    for (idx, raw_line) in script.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(delay) = line.strip_prefix(';') {
            let delay_ms = delay
                .trim()
                .parse::<u32>()
                .map_err(|_| format!("Invalid macro delay on line {}: {line}", idx + 1))?;
            steps.push(LowCodeKeyMacroStep::Delay { delay_ms });
            continue;
        }

        if let Some(text) = line.strip_prefix('%') {
            steps.push(LowCodeKeyMacroStep::TypeText {
                text: text.to_string(),
            });
            continue;
        }

        if let Some(keys) = line.strip_prefix('@') {
            let mut modifiers = Vec::new();
            let mut key = None;

            for token in keys
                .split('+')
                .map(str::trim)
                .filter(|token| !token.is_empty())
            {
                if let Some(modifier) = quicker_macro_modifier_label(token) {
                    modifiers.push(modifier.to_string());
                    continue;
                }
                if let Some(label) = quicker_macro_key_label(token) {
                    if key.replace(label).is_some() {
                        return Err(format!(
                            "Multiple macro keys on line {} are not supported by the visual editor: {line}",
                            idx + 1
                        ));
                    }
                    continue;
                }
                return Err(format!(
                    "Unsupported macro token on line {}: {token}",
                    idx + 1
                ));
            }

            let Some(key) = key else {
                return Err(format!("Missing macro key on line {}: {line}", idx + 1));
            };

            steps.push(LowCodeKeyMacroStep::SendKeys {
                modifiers: modifiers.join("+"),
                key,
            });
            continue;
        }

        return Err(format!(
            "Unsupported macro instruction on line {}: {line}",
            idx + 1
        ));
    }

    Ok(steps)
}

fn serialize_quicker_key_macro_steps(steps: &[LowCodeKeyMacroStep]) -> Result<String, String> {
    let mut lines = Vec::with_capacity(steps.len());

    for step in steps {
        match step {
            LowCodeKeyMacroStep::Delay { delay_ms } => lines.push(format!(";{delay_ms}")),
            LowCodeKeyMacroStep::TypeText { text } => lines.push(format!("%{text}")),
            LowCodeKeyMacroStep::SendKeys { modifiers, key } => {
                let mut tokens = parse_low_code_modifiers(modifiers)?
                    .into_iter()
                    .filter_map(low_code_modifier_macro_token)
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                let key_code = parse_low_code_key(key)?;
                let key_token = low_code_key_macro_token(key_code)
                    .ok_or_else(|| format!("Unsupported key macro key: {key}"))?;
                tokens.push(key_token);
                lines.push(format!("@{}", tokens.join("+")));
            }
        }
    }

    Ok(lines.join("\n"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StepFlow {
    Continue,
    Stop(Option<String>),
}

struct QuickerRuntime {
    vars: HashMap<String, Value>,
    last_message: Option<String>,
    state_scope: String,
    action_state: HashMap<String, String>,
    control: Option<ActionExecutionControl>,
}

impl QuickerRuntime {
    fn new(
        data: &QuickerPluginData,
        state_scope: String,
        control: Option<ActionExecutionControl>,
    ) -> Self {
        let mut vars = HashMap::new();
        for variable in &data.variables {
            vars.insert(
                variable.key.clone(),
                Value::String(variable.default_value.clone().unwrap_or_default()),
            );
        }
        let action_state = load_action_state_scope(&state_scope);

        Self {
            vars,
            last_message: None,
            state_scope,
            action_state,
            control,
        }
    }

    fn run_steps(&mut self, steps: &[QuickerPluginStepDocument]) -> Result<StepFlow, String> {
        for step in steps {
            ensure_not_cancelled(self.control.as_ref())?;
            if step.disabled {
                continue;
            }

            if step.delay_ms > 0 {
                sleep_millis(step.delay_ms as u64, self.control.as_ref())?;
            }

            match self.run_step(step)? {
                StepFlow::Continue => {}
                stop => return Ok(stop),
            }
        }

        Ok(StepFlow::Continue)
    }

    fn run_step(&mut self, step: &QuickerPluginStepDocument) -> Result<StepFlow, String> {
        match step.step_runner_key.as_str() {
            "sys:openUrl" => {
                let url = self.input_string(&step.input_params, "url")?;
                open_target(&url)
                    .map_err(|err| format!("Failed to open URL '{}': {}", url, err))?;
                Ok(StepFlow::Continue)
            }
            "sys:stateStorage" => {
                let mode = self
                    .input_string_opt(&step.input_params, "type")
                    .unwrap_or_default();
                let key = self.input_string(&step.input_params, "key")?;
                match mode.as_str() {
                    "readActionState" => {
                        let default_value = self
                            .input_string_opt(&step.input_params, "defaultValue")
                            .unwrap_or_default();
                        let value = self
                            .action_state
                            .get(&key)
                            .cloned()
                            .unwrap_or(default_value);
                        let is_empty = value.trim().is_empty();
                        self.assign_output(&step.output_params, "value", Value::String(value));
                        self.assign_output(&step.output_params, "isEmpty", Value::Bool(is_empty));
                        self.assign_output(&step.output_params, "isSuccess", Value::Bool(true));
                        Ok(StepFlow::Continue)
                    }
                    "saveActionState" => {
                        let value = self
                            .input_string_opt(&step.input_params, "value")
                            .unwrap_or_default();
                        self.action_state.insert(key.clone(), value.clone());
                        save_action_state_scope(&self.state_scope, &self.action_state)?;
                        self.assign_output(&step.output_params, "isSuccess", Value::Bool(true));
                        Ok(StepFlow::Continue)
                    }
                    other => Err(format!("Unsupported stateStorage type: {other}")),
                }
            }
            "sys:MsgBox" => {
                let title = self
                    .input_string_opt(&step.input_params, "title")
                    .unwrap_or_default();
                let message = self.input_string(&step.input_params, "message")?;
                show_message_box(&title, &message)?;
                self.assign_output(&step.output_params, "okOrYes", Value::Bool(true));
                Ok(StepFlow::Continue)
            }
            "sys:selectFolder" => {
                let prompt = self
                    .input_string_opt(&step.input_params, "prompt")
                    .unwrap_or_default();
                let init_dir = self.input_string_opt(&step.input_params, "initDir");
                let stop_if_fail = self.input_bool(&step.input_params, "stopIfFail");
                match select_folder_dialog(&prompt, init_dir.as_deref()) {
                    Ok(path) => {
                        self.assign_output(&step.output_params, "path", Value::String(path));
                        self.assign_output(&step.output_params, "isSuccess", Value::Bool(true));
                        Ok(StepFlow::Continue)
                    }
                    Err(err) => {
                        self.assign_output(&step.output_params, "isSuccess", Value::Bool(false));
                        if stop_if_fail {
                            Err(err)
                        } else {
                            Ok(StepFlow::Continue)
                        }
                    }
                }
            }
            "sys:userInput" => {
                let prompt = self
                    .input_string_opt(&step.input_params, "prompt")
                    .unwrap_or_default();
                let default_value = self
                    .input_string_opt(&step.input_params, "defaultValue")
                    .unwrap_or_default();
                let multiline = matches!(
                    self.input_string_opt(&step.input_params, "type").as_deref(),
                    Some("multiline")
                );
                let stop_if_fail = self.input_bool(&step.input_params, "stopIfFail");
                match prompt_user_input_dialog(&prompt, &default_value, multiline) {
                    Ok(text) => {
                        let is_empty = text.trim().is_empty();
                        self.assign_output(&step.output_params, "textValue", Value::String(text));
                        self.assign_output(&step.output_params, "isEmpty", Value::Bool(is_empty));
                        self.assign_output(&step.output_params, "isSuccess", Value::Bool(true));
                        Ok(StepFlow::Continue)
                    }
                    Err(err) => {
                        self.assign_output(&step.output_params, "isSuccess", Value::Bool(false));
                        if stop_if_fail {
                            Err(err)
                        } else {
                            Ok(StepFlow::Continue)
                        }
                    }
                }
            }
            "sys:delay" => {
                let delay_ms = self
                    .input_string_opt(&step.input_params, "delayMs")
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0);
                sleep_millis(delay_ms, self.control.as_ref())?;
                Ok(StepFlow::Continue)
            }
            "sys:keyInput" => {
                let keys = self.input_string(&step.input_params, "keys")?;
                let payload: QuickerKeyInput = serde_json::from_str(&keys)
                    .map_err(|err| format!("Failed to parse keyInput payload: {err}"))?;
                for key in payload.keys {
                    let key_name = virtual_key_name(key)
                        .ok_or_else(|| format!("Unsupported virtual key code: {key}"))?;
                    let modifiers = payload
                        .ctrl_keys
                        .iter()
                        .copied()
                        .filter_map(virtual_key_modifier)
                        .map(str::to_string)
                        .collect::<Vec<_>>();
                    send_key_combo(&modifiers, key_name)?;
                }
                Ok(StepFlow::Continue)
            }
            "sys:getClipboardText" => {
                let format = self
                    .input_string_opt(&step.input_params, "format")
                    .unwrap_or_else(|| "UnicodeText".into());
                let stop_if_fail = self.input_bool(&step.input_params, "stopIfFail");
                let result = match format.as_str() {
                    "Html" => read_clipboard_html(),
                    _ => read_clipboard_text(),
                };

                match result {
                    Ok(text) => {
                        self.assign_output(&step.output_params, "output", Value::String(text));
                        self.assign_output(&step.output_params, "isSuccess", Value::Bool(true));
                        Ok(StepFlow::Continue)
                    }
                    Err(err) => {
                        self.assign_output(&step.output_params, "isSuccess", Value::Bool(false));
                        if stop_if_fail {
                            Err(err)
                        } else {
                            Ok(StepFlow::Continue)
                        }
                    }
                }
            }
            "sys:writeClipboard" => {
                let clipboard_type = self
                    .input_string_opt(&step.input_params, "type")
                    .unwrap_or_else(|| "auto".into())
                    .to_ascii_lowercase();
                let success_msg = self.input_string_opt(&step.input_params, "successMsg");

                match clipboard_type.as_str() {
                    "html" => {
                        let html = self.input_string(&step.input_params, "html")?;
                        let alt_text = self.input_string_opt(&step.input_params, "text");
                        write_clipboard_html(&html, alt_text.as_deref())?;
                    }
                    "text" => {
                        let text = self.input_string(&step.input_params, "text")?;
                        write_clipboard_text(&text)?;
                    }
                    _ => {
                        let text = self
                            .input_string_opt(&step.input_params, "input")
                            .or_else(|| self.input_string_opt(&step.input_params, "text"))
                            .unwrap_or_default();
                        write_clipboard_text(&text)?;
                    }
                }

                self.assign_output(&step.output_params, "isSuccess", Value::Bool(true));
                if let Some(message) = success_msg.filter(|value| !value.is_empty()) {
                    self.last_message = Some(message);
                }
                Ok(StepFlow::Continue)
            }
            "sys:regexExtract" => {
                let input = self.input_string(&step.input_params, "data")?;
                let pattern = self.input_string(&step.input_params, "pattern")?;
                let get_group = self
                    .input_string_opt(&step.input_params, "getGroup")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                let stop_if_fail = self.input_bool(&step.input_params, "stopIfFail");
                let regex = compile_step_regex(
                    &pattern,
                    self.input_bool(&step.input_params, "ignoreCase"),
                    self.input_bool(&step.input_params, "singleLine"),
                    self.input_bool(&step.input_params, "multiLine"),
                )?;

                let captures = regex
                    .captures(&input)
                    .map_err(|err| format!("Regex failed: {err}"))?;

                match captures {
                    Some(captures) => {
                        let matched = captures
                            .get(get_group)
                            .map(|capture| capture.as_str().to_string())
                            .unwrap_or_default();
                        self.assign_regex_outputs(&step.output_params, &matched);
                        self.assign_output(&step.output_params, "isSuccess", Value::Bool(true));
                        Ok(StepFlow::Continue)
                    }
                    None => {
                        self.assign_output(&step.output_params, "isSuccess", Value::Bool(false));
                        if stop_if_fail {
                            Err(format!("Regex did not match pattern: {pattern}"))
                        } else {
                            Ok(StepFlow::Continue)
                        }
                    }
                }
            }
            "sys:stringProcess" => {
                let input = self.input_string(&step.input_params, "data")?;
                let method = self
                    .input_string_opt(&step.input_params, "method")
                    .unwrap_or_default();
                let output = match method.as_str() {
                    "toLower" => input.to_lowercase(),
                    "urlEncode" => urlencoding::encode(&input).into_owned(),
                    other => return Err(format!("Unsupported stringProcess method: {other}")),
                };
                self.assign_output(&step.output_params, "output", Value::String(output));
                self.assign_output(&step.output_params, "isSuccess", Value::Bool(true));
                Ok(StepFlow::Continue)
            }
            "sys:download" => {
                let url = self.input_string(&step.input_params, "url")?;
                let save_path = self.input_string(&step.input_params, "savePath")?;
                let save_name = self
                    .input_string_opt(&step.input_params, "saveName")
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| derive_download_file_name(&url));
                let options = DownloadRequestOptions::from_inputs(
                    self.input_string_opt(&step.input_params, "ua")
                        .filter(|value| !value.trim().is_empty()),
                    self.input_string_opt(&step.input_params, "header")
                        .filter(|value| !value.trim().is_empty()),
                    self.input_string_opt(&step.input_params, "cookie")
                        .filter(|value| !value.trim().is_empty()),
                );
                let stop_if_fail = self.input_bool(&step.input_params, "stopIfFail");
                match download_to_file(
                    &url,
                    &save_path,
                    &save_name,
                    &options,
                    self.control.as_ref(),
                ) {
                    Ok(saved_path) => {
                        self.assign_output(&step.output_params, "isSuccess", Value::Bool(true));
                        self.assign_output(
                            &step.output_params,
                            "savedPath",
                            Value::String(saved_path),
                        );
                        Ok(StepFlow::Continue)
                    }
                    Err(err) => {
                        self.assign_output(&step.output_params, "isSuccess", Value::Bool(false));
                        if stop_if_fail {
                            Err(err)
                        } else {
                            Ok(StepFlow::Continue)
                        }
                    }
                }
            }
            "sys:readFile" => {
                let path = normalize_runtime_path(&self.input_string(&step.input_params, "path")?);
                let stop_if_fail = self.input_bool(&step.input_params, "stopIfFail");
                let file_type = self
                    .input_string_opt(&step.input_params, "type")
                    .unwrap_or_default();
                match file_type.as_str() {
                    "image" => match read_file_path_reference(&path) {
                        Ok(value) => {
                            self.assign_output(&step.output_params, "image", Value::String(value));
                            self.assign_output(&step.output_params, "isSuccess", Value::Bool(true));
                            Ok(StepFlow::Continue)
                        }
                        Err(err) => {
                            self.assign_output(
                                &step.output_params,
                                "isSuccess",
                                Value::Bool(false),
                            );
                            if stop_if_fail {
                                Err(err)
                            } else {
                                Ok(StepFlow::Continue)
                            }
                        }
                    },
                    other => Err(format!("Unsupported readFile type: {other}")),
                }
            }
            "sys:imageinfo" => {
                let path =
                    normalize_runtime_path(&self.input_string(&step.input_params, "bmpVar")?);
                let bytes = read_binary_file(&path)?;
                let (width, height) = image_dimensions(&bytes)?;
                self.assign_output(
                    &step.output_params,
                    "width",
                    Value::Number(serde_json::Number::from(width)),
                );
                self.assign_output(
                    &step.output_params,
                    "height",
                    Value::Number(serde_json::Number::from(height)),
                );
                Ok(StepFlow::Continue)
            }
            "sys:imgToBase64" => {
                let path = normalize_runtime_path(&self.input_string(&step.input_params, "img")?);
                let bytes = read_binary_file(&path)?;
                self.assign_output(
                    &step.output_params,
                    "code",
                    Value::String(base64_encode(&bytes)),
                );
                Ok(StepFlow::Continue)
            }
            "sys:fileOperation" => {
                let op = self
                    .input_string_opt(&step.input_params, "type")
                    .unwrap_or_default();
                match op.as_str() {
                    "deleteFile" => {
                        let path =
                            normalize_runtime_path(&self.input_string(&step.input_params, "path")?);
                        let stop_if_fail = self.input_bool(&step.input_params, "stopIfFail");
                        match delete_file_path(&path) {
                            Ok(()) => {
                                self.assign_output(
                                    &step.output_params,
                                    "isSuccess",
                                    Value::Bool(true),
                                );
                                Ok(StepFlow::Continue)
                            }
                            Err(err) => {
                                self.assign_output(
                                    &step.output_params,
                                    "isSuccess",
                                    Value::Bool(false),
                                );
                                if stop_if_fail {
                                    Err(err)
                                } else {
                                    Ok(StepFlow::Continue)
                                }
                            }
                        }
                    }
                    other => Err(format!("Unsupported fileOperation type: {other}")),
                }
            }
            "sys:splitString" => {
                let input = self.input_string(&step.input_params, "data")?;
                let separator = self
                    .input_string_opt(&step.input_params, "separator")
                    .unwrap_or_default();
                let separator = if self.input_bool(&step.input_params, "escapeSeparator") {
                    unescape_basic(&separator)
                } else {
                    separator
                };
                let remove_empty = self.input_bool(&step.input_params, "removeEmpty");
                let values = input
                    .split(&separator)
                    .filter(|part| !remove_empty || !part.is_empty())
                    .map(|part| Value::String(part.to_string()))
                    .collect::<Vec<_>>();
                self.assign_output(&step.output_params, "output", Value::Array(values));
                Ok(StepFlow::Continue)
            }
            "sys:assign" => {
                let input = self.input_string(&step.input_params, "input")?;
                let stop_if_fail = self.input_bool(&step.input_params, "stopIfFail");
                match self.eval_assign_expression(&input) {
                    Some(value) => {
                        self.assign_output(&step.output_params, "output", value);
                        self.assign_output(&step.output_params, "isSuccess", Value::Bool(true));
                        Ok(StepFlow::Continue)
                    }
                    None => {
                        self.assign_output(&step.output_params, "isSuccess", Value::Bool(false));
                        if stop_if_fail {
                            Err(format!("Failed to evaluate assign input: {input}"))
                        } else {
                            Ok(StepFlow::Continue)
                        }
                    }
                }
            }
            "sys:strReplace" => {
                let input = self.input_string(&step.input_params, "input")?;
                let old = self
                    .input_string_opt(&step.input_params, "old")
                    .unwrap_or_default();
                let new = self
                    .input_string_opt(&step.input_params, "new")
                    .unwrap_or_default();
                let replace_escapes = self.input_bool(&step.input_params, "replaceEscapes");
                let old = if replace_escapes {
                    unescape_basic(&old)
                } else {
                    old
                };
                let new = if replace_escapes {
                    unescape_basic(&new)
                } else {
                    new
                };
                let output = if self.input_bool(&step.input_params, "useRegex") {
                    let regex = compile_step_regex(
                        &old,
                        self.input_bool(&step.input_params, "ignoreCase"),
                        self.input_bool(&step.input_params, "singleLine"),
                        self.input_bool(&step.input_params, "multiLine"),
                    )?;
                    regex.replace_all(&input, new.as_str()).into_owned()
                } else {
                    input.replace(&old, &new)
                };
                self.assign_output(&step.output_params, "output", Value::String(output));
                Ok(StepFlow::Continue)
            }
            "sys:simpleIf" => {
                let condition = self.input_value(&step.input_params, "condition");
                let branch = if truthy(condition.as_ref()) {
                    step.if_steps.as_deref().unwrap_or(&[])
                } else {
                    step.else_steps.as_deref().unwrap_or(&[])
                };
                self.run_steps(branch)
            }
            "sys:group" => self.run_steps(step.if_steps.as_deref().unwrap_or(&[])),
            "sys:stop" => {
                let is_error = self.input_bool(&step.input_params, "isError");
                let message = self.input_string_opt(&step.input_params, "showMessage");
                if is_error {
                    Err(message.unwrap_or_else(|| "Quicker action stopped with an error".into()))
                } else {
                    Ok(StepFlow::Stop(message))
                }
            }
            "sys:formatString" => {
                let format_string = self
                    .input_string_opt(&step.input_params, "formatString")
                    .unwrap_or_default();
                let mut output = format_string;
                for idx in 0..=4 {
                    let value = self
                        .input_string_opt(&step.input_params, &format!("p{idx}"))
                        .unwrap_or_default();
                    output = output.replace(&format!("{{{idx}}}"), &value);
                }
                self.assign_output(&step.output_params, "output", Value::String(output));
                Ok(StepFlow::Continue)
            }
            "sys:notify" => {
                if let Some(message) = self.input_string_opt(&step.input_params, "msg") {
                    self.last_message = Some(message);
                }
                Ok(StepFlow::Continue)
            }
            "sys:reportProgress" => Ok(StepFlow::Continue),
            "sys:outputText" => {
                let content = self.input_string(&step.input_params, "content")?;
                let method = self
                    .input_string_opt(&step.input_params, "method")
                    .unwrap_or_else(|| "paste".into());
                let before = self
                    .input_string_opt(&step.input_params, "delayBeforePaste")
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0);
                let after = self
                    .input_string_opt(&step.input_params, "delayAfterPaste")
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0);
                let append_return = self.input_bool(&step.input_params, "appendReturn");

                match method.as_str() {
                    "paste" => {
                        write_clipboard_text(&content)?;
                        sleep_millis(before, self.control.as_ref())?;
                        send_key_combo(&["ctrl".into()], "v")?;
                        if append_return {
                            send_key_combo(&[], "Return")?;
                        }
                        sleep_millis(after, self.control.as_ref())?;
                    }
                    other => return Err(format!("Unsupported outputText method: {other}")),
                }

                Ok(StepFlow::Continue)
            }
            other => Err(format!("Unsupported Quicker step: {other}")),
        }
    }

    fn input_value(&self, params: &Map<String, Value>, key: &str) -> Option<Value> {
        let raw = params.get(key)?;
        let binding: QuickerValueBinding = serde_json::from_value(raw.clone()).ok()?;
        if let Some(var_key) = binding.var_key.as_deref() {
            return self.vars.get(var_key).cloned();
        }

        binding.value.map(|value| match value {
            Value::String(text) => Value::String(expand_runtime_vars(&text, &self.vars)),
            other => other,
        })
    }

    fn input_string(&self, params: &Map<String, Value>, key: &str) -> Result<String, String> {
        self.input_string_opt(params, key)
            .ok_or_else(|| format!("Missing input param: {key}"))
    }

    fn input_string_opt(&self, params: &Map<String, Value>, key: &str) -> Option<String> {
        self.input_value(params, key)
            .map(|value| value_to_string(&value))
    }

    fn input_bool(&self, params: &Map<String, Value>, key: &str) -> bool {
        truthy(self.input_value(params, key).as_ref())
    }

    fn assign_output(&mut self, params: &Map<String, Value>, key: &str, value: Value) {
        let Some(name) = output_var_name(params, key) else {
            return;
        };
        self.vars.insert(name, value);
    }

    fn assign_regex_outputs(&mut self, params: &Map<String, Value>, matched: &str) {
        for candidate in ["match1", "matches", "output"] {
            self.assign_output(params, candidate, Value::String(matched.to_string()));
        }
    }

    fn eval_assign_expression(&self, input: &str) -> Option<Value> {
        let trimmed = input.trim();
        if let Some(captures) = Regex::new(r"^\$=\{([^}]+)\}\[(\d+)\]$")
            .ok()
            .and_then(|regex| regex.captures(trimmed).ok().flatten())
        {
            let name = captures.get(1)?.as_str();
            let index = captures.get(2)?.as_str().parse::<usize>().ok()?;
            let values = self.vars.get(name)?.as_array()?;
            return values.get(index).cloned();
        }

        if let Some(captures) = Regex::new(r"^\$=\{([^}]+)\}$")
            .ok()
            .and_then(|regex| regex.captures(trimmed).ok().flatten())
        {
            let name = captures.get(1)?.as_str();
            return self.vars.get(name).cloned();
        }

        Some(Value::String(expand_runtime_vars(trimmed, &self.vars)))
    }
}

fn execute_quicker_action_document(
    quicker_json: &str,
    control: Option<&ActionExecutionControl>,
) -> ExecResult {
    let document = match parse_quicker_action_document(quicker_json) {
        Ok(document) => document,
        Err(err) => {
            return ExecResult::Err(err);
        }
    };

    if let Some(delay_ms) = document.delay_ms.filter(|delay| *delay > 0) {
        if let Err(err) = sleep_millis(delay_ms as u64, control) {
            return ExecResult::Err(err);
        }
    }

    match document.action_type {
        QUICKER_PLUGIN_ACTION_TYPE => execute_quicker_plugin_steps(&document, control),
        QUICKER_OPEN_ACTION_TYPE => execute_quicker_launch(&document),
        QUICKER_KEYS_ACTION_TYPE => execute_quicker_key_macro(&document, control),
        action_type => ExecResult::Err(format!(
            "Unsupported Quicker action type {action_type}. Supported sample types are 7, 11, and 24."
        )),
    }
}

fn execute_quicker_plugin_steps(
    document: &QuickerActionDocument,
    control: Option<&ActionExecutionControl>,
) -> ExecResult {
    if document.use_template.unwrap_or(false) && !document.has_data() {
        return ExecResult::Err(
            "Template-based Quicker actions cannot run yet because the template body is not embedded in the sample JSON"
                .into(),
        );
    }

    let data = match document.data_payload() {
        Ok(data) => data,
        Err(err) => return ExecResult::Err(err),
    };

    let state_scope = document
        .id
        .clone()
        .unwrap_or_else(|| document.title.clone());
    let mut runtime = QuickerRuntime::new(&data, state_scope, control.cloned());
    match runtime.run_steps(&data.steps) {
        Ok(StepFlow::Continue) => match runtime.last_message {
            Some(message) if !message.is_empty() => ExecResult::OkWithMessage(message),
            _ => ExecResult::Ok,
        },
        Ok(StepFlow::Stop(message)) => match message.or(runtime.last_message) {
            Some(message) if !message.is_empty() => ExecResult::OkWithMessage(message),
            _ => ExecResult::Ok,
        },
        Err(err) => ExecResult::Err(err),
    }
}

fn execute_quicker_launch(document: &QuickerActionDocument) -> ExecResult {
    let launch = match document.launch_payload() {
        Ok(launch) => launch,
        Err(err) => return ExecResult::Err(err),
    };

    if launch.arguments.trim().is_empty() {
        return match open_target(&launch.file_name) {
            Ok(_) => ExecResult::Ok,
            Err(err) => {
                ExecResult::Err(format!("Failed to launch '{}': {}", launch.file_name, err))
            }
        };
    }

    let args = launch
        .arguments
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let working_dir = launch
        .set_working_dir
        .then(|| {
            Path::new(&launch.file_name)
                .parent()
                .map(|path| path.to_string_lossy().to_string())
        })
        .flatten();

    spawn_program(&launch.file_name, &args, working_dir.as_deref())
}

fn execute_quicker_key_macro(
    document: &QuickerActionDocument,
    control: Option<&ActionExecutionControl>,
) -> ExecResult {
    let script = document.data_text();
    if script.trim().is_empty() {
        return ExecResult::Err("Quicker key macro action is missing Data".into());
    }

    for line in script
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Err(err) = ensure_not_cancelled(control) {
            return ExecResult::Err(err);
        }
        if let Some(delay) = line.strip_prefix(';') {
            let Ok(delay_ms) = delay.trim().parse::<u64>() else {
                return ExecResult::Err(format!("Invalid macro delay: {line}"));
            };
            if let Err(err) = sleep_millis(delay_ms, control) {
                return ExecResult::Err(err);
            }
            continue;
        }

        if let Some(text) = line.strip_prefix('%') {
            if let Err(err) = type_input_text(text) {
                return ExecResult::Err(err);
            }
            continue;
        }

        if let Some(keys) = line.strip_prefix('@') {
            let mut modifiers = Vec::new();
            let mut key_name = None;

            for token in keys.split('+').filter(|token| !token.is_empty()) {
                if let Some(modifier) = quicker_macro_modifier(token) {
                    modifiers.push(modifier.to_string());
                } else if let Some(key) = quicker_macro_key(token) {
                    key_name = Some(key.to_string());
                } else {
                    return ExecResult::Err(format!("Unsupported macro token: {token}"));
                }
            }

            let Some(key_name) = key_name else {
                return ExecResult::Err(format!("Missing macro key in line: {line}"));
            };

            if let Err(err) = send_key_combo(&modifiers, &key_name) {
                return ExecResult::Err(err);
            }
            continue;
        }

        return ExecResult::Err(format!("Unsupported macro instruction: {line}"));
    }

    ExecResult::Ok
}

fn compile_step_regex(
    pattern: &str,
    ignore_case: bool,
    single_line: bool,
    multi_line: bool,
) -> Result<Regex, String> {
    let mut prefix = String::new();
    if ignore_case {
        prefix.push_str("(?i)");
    }
    if single_line {
        prefix.push_str("(?s)");
    }
    if multi_line {
        prefix.push_str("(?m)");
    }

    Regex::new(&format!("{prefix}{pattern}"))
        .map_err(|err| format!("Invalid regex '{pattern}': {err}"))
}

fn expand_runtime_vars(input: &str, vars: &HashMap<String, Value>) -> String {
    let mut output = String::new();
    let mut rest = input;

    while let Some(start) = rest.find("$${") {
        output.push_str(&rest[..start]);
        let suffix = &rest[start + 3..];
        if let Some(end) = suffix.find('}') {
            let name = &suffix[..end];
            if let Some(value) = vars.get(name) {
                output.push_str(&value_to_string(value));
            }
            rest = &suffix[end + 1..];
        } else {
            output.push_str(&rest[start..]);
            return output;
        }
    }

    output.push_str(rest);

    let mut final_output = String::new();
    let mut chars = output.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut name = String::new();
            let mut probe = chars.clone();
            let mut found_end = false;
            while let Some(next) = probe.next() {
                if next == '}' {
                    found_end = true;
                    break;
                }
                name.push(next);
            }
            if found_end
                && !name.is_empty()
                && !name.chars().all(|ch| ch.is_ascii_digit())
                && vars.contains_key(&name)
            {
                for _ in 0..name.len() {
                    chars.next();
                }
                chars.next();
                final_output.push_str(&value_to_string(vars.get(&name).unwrap()));
                continue;
            }
        }
        final_output.push(ch);
    }

    final_output
}

fn output_var_name(params: &Map<String, Value>, key: &str) -> Option<String> {
    params.iter().find_map(|(name, value)| {
        (name.trim_end() == key)
            .then(|| value.as_str())
            .flatten()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_i64().unwrap_or(0) != 0,
        Some(Value::String(value)) => {
            let normalized = value.trim();
            !normalized.is_empty()
                && normalized != "0"
                && !normalized.eq_ignore_ascii_case("false")
                && !normalized.eq_ignore_ascii_case("no")
        }
        Some(Value::Array(values)) => !values.is_empty(),
        Some(Value::Null) | None => false,
        Some(_) => true,
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => {
            if *value {
                "1".into()
            } else {
                "0".into()
            }
        }
        Value::Number(value) => value.to_string(),
        Value::Array(values) => values
            .iter()
            .map(value_to_string)
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => value.to_string(),
    }
}

fn unescape_basic(input: &str) -> String {
    let mut output = String::new();
    let mut chars = input.chars();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }

        match chars.next() {
            Some('r') => output.push('\r'),
            Some('n') => output.push('\n'),
            Some('t') => output.push('\t'),
            Some('\\') => output.push('\\'),
            Some('"') => output.push('"'),
            Some('\'') => output.push('\''),
            Some(other) => {
                output.push('\\');
                output.push(other);
            }
            None => output.push('\\'),
        }
    }

    output
}

fn quicker_macro_modifier(token: &str) -> Option<&'static str> {
    match token {
        "LMENU" | "MENU" | "RMENU" | "ALT" => Some("alt"),
        "CTRL" | "CONTROL" | "LCONTROL" | "RCONTROL" => Some("ctrl"),
        "SHIFT" | "LSHIFT" | "RSHIFT" => Some("shift"),
        "LWIN" | "RWIN" | "WIN" => Some("super"),
        _ => None,
    }
}

fn quicker_macro_modifier_label(token: &str) -> Option<&'static str> {
    match quicker_macro_modifier(token)? {
        "alt" => Some("alt"),
        "ctrl" => Some("ctrl"),
        "shift" => Some("shift"),
        "super" => Some("super"),
        _ => None,
    }
}

fn quicker_macro_key(token: &str) -> Option<&'static str> {
    match token {
        "RETURN" => Some("Return"),
        "DOWN" => Some("Down"),
        "UP" => Some("Up"),
        "LEFT" => Some("Left"),
        "RIGHT" => Some("Right"),
        "ESC" | "ESCAPE" => Some("Escape"),
        "TAB" => Some("Tab"),
        "SPACE" => Some("space"),
        "BACK" | "BACKSPACE" => Some("BackSpace"),
        _ => token.strip_prefix("VK_").and_then(macro_virtual_key_name),
    }
}

fn quicker_macro_key_label(token: &str) -> Option<String> {
    Some(match quicker_macro_key(token)? {
        "space" => "Space".into(),
        "BackSpace" => "Backspace".into(),
        key if key.len() == 1 => key.to_ascii_uppercase(),
        key => key.to_string(),
    })
}

fn macro_virtual_key_name(token: &str) -> Option<&'static str> {
    match token {
        "A" => Some("a"),
        "C" => Some("c"),
        "P" => Some("p"),
        "T" => Some("t"),
        "V" => Some("v"),
        "X" => Some("x"),
        "0" => Some("0"),
        "1" => Some("1"),
        "2" => Some("2"),
        "3" => Some("3"),
        "4" => Some("4"),
        "5" => Some("5"),
        "6" => Some("6"),
        "7" => Some("7"),
        "8" => Some("8"),
        "9" => Some("9"),
        _ => None,
    }
}

fn virtual_key_modifier(code: u32) -> Option<&'static str> {
    match code {
        16 | 160 | 161 => Some("shift"),
        17 | 162 | 163 => Some("ctrl"),
        18 | 164 | 165 => Some("alt"),
        91 | 92 => Some("super"),
        _ => None,
    }
}

fn virtual_key_name(code: u32) -> Option<&'static str> {
    match code {
        13 => Some("Return"),
        27 => Some("Escape"),
        37 => Some("Left"),
        38 => Some("Up"),
        39 => Some("Right"),
        40 => Some("Down"),
        48 => Some("0"),
        49 => Some("1"),
        50 => Some("2"),
        51 => Some("3"),
        52 => Some("4"),
        53 => Some("5"),
        54 => Some("6"),
        55 => Some("7"),
        56 => Some("8"),
        57 => Some("9"),
        65 => Some("a"),
        66 => Some("b"),
        67 => Some("c"),
        68 => Some("d"),
        69 => Some("e"),
        70 => Some("f"),
        71 => Some("g"),
        72 => Some("h"),
        73 => Some("i"),
        74 => Some("j"),
        75 => Some("k"),
        76 => Some("l"),
        77 => Some("m"),
        78 => Some("n"),
        79 => Some("o"),
        80 => Some("p"),
        81 => Some("q"),
        82 => Some("r"),
        83 => Some("s"),
        84 => Some("t"),
        85 => Some("u"),
        86 => Some("v"),
        87 => Some("w"),
        88 => Some("x"),
        89 => Some("y"),
        90 => Some("z"),
        _ => None,
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

fn write_clipboard_html(html: &str, alt_text: Option<&str>) -> Result<(), String> {
    #[cfg(test)]
    if let Some(result) = test_write_clipboard_html(html, alt_text) {
        return result;
    }

    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("Clipboard error: {}", e))?;
    clipboard
        .set_html(html, alt_text)
        .map_err(|e| format!("Clipboard error: {}", e))
}

fn read_clipboard_html() -> Result<String, String> {
    #[cfg(test)]
    if let Some(result) = test_read_clipboard_html() {
        return result;
    }

    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("Clipboard error: {}", e))?;
    clipboard
        .get()
        .html()
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

type ActionStateStore = HashMap<String, HashMap<String, String>>;

fn action_state_store_path() -> std::path::PathBuf {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("quicker-rs");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("action_state.json")
}

fn load_action_state_scope(scope: &str) -> HashMap<String, String> {
    #[cfg(test)]
    if let Some(state) = test_load_action_state_scope(scope) {
        return state;
    }

    let path = action_state_store_path();
    let Ok(content) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(store) = serde_json::from_str::<ActionStateStore>(&content) else {
        return HashMap::new();
    };
    store.get(scope).cloned().unwrap_or_default()
}

fn save_action_state_scope(scope: &str, state: &HashMap<String, String>) -> Result<(), String> {
    #[cfg(test)]
    if test_save_action_state_scope(scope, state) {
        return Ok(());
    }

    let path = action_state_store_path();
    let mut store = match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str::<ActionStateStore>(&content).unwrap_or_default(),
        Err(_) => HashMap::new(),
    };
    store.insert(scope.to_string(), state.clone());
    let content = serde_json::to_string_pretty(&store)
        .map_err(|err| format!("Failed to serialize action state store: {err}"))?;
    std::fs::write(&path, content)
        .map_err(|err| format!("Failed to save action state store: {err}"))
}

fn show_message_box(title: &str, message: &str) -> Result<(), String> {
    #[cfg(test)]
    if let Some(result) = test_show_message_box(title, message) {
        return result;
    }

    #[cfg(target_os = "windows")]
    {
        return Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(format!(
                "Add-Type -AssemblyName PresentationFramework; [System.Windows.MessageBox]::Show(@'\n{}\n'@, @'\n{}\n'@) | Out-Null",
                message, title
            ))
            .status()
            .map_err(|err| format!("Failed to show message box: {err}"))
            .and_then(|status| {
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("Message box command exited with {status}"))
                }
            });
    }
    #[cfg(target_os = "macos")]
    {
        return Command::new("osascript")
            .arg("-e")
            .arg(format!(
                "display dialog {:?} with title {:?} buttons {{\"OK\"}} default button \"OK\"",
                message, title
            ))
            .status()
            .map_err(|err| format!("Failed to show message box: {err}"))
            .and_then(|status| {
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("Message box command exited with {status}"))
                }
            });
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if which::which("kdialog").is_ok() {
            return Command::new("kdialog")
                .arg("--title")
                .arg(title)
                .arg("--msgbox")
                .arg(message)
                .status()
                .map_err(|err| format!("Failed to show message box: {err}"))
                .and_then(|status| {
                    if status.success() {
                        Ok(())
                    } else {
                        Err(format!("Message box command exited with {status}"))
                    }
                });
        }
        if which::which("zenity").is_ok() {
            return Command::new("zenity")
                .arg("--info")
                .arg("--title")
                .arg(title)
                .arg("--text")
                .arg(message)
                .status()
                .map_err(|err| format!("Failed to show message box: {err}"))
                .and_then(|status| {
                    if status.success() {
                        Ok(())
                    } else {
                        Err(format!("Message box command exited with {status}"))
                    }
                });
        }
    }

    Err("No supported message box backend was found".into())
}

fn select_folder_dialog(prompt: &str, init_dir: Option<&str>) -> Result<String, String> {
    #[cfg(test)]
    if let Some(result) = test_select_folder_dialog(prompt, init_dir) {
        return result;
    }

    #[cfg(target_os = "windows")]
    {
        let script = "[System.Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms') | Out-Null; $dlg = New-Object System.Windows.Forms.FolderBrowserDialog; if ($dlg.ShowDialog() -eq 'OK') { Write-Output $dlg.SelectedPath }";
        let output = Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(script)
            .output()
            .map_err(|err| format!("Failed to open folder dialog: {err}"))?;
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() {
                Err("Folder selection was cancelled".into())
            } else {
                Ok(text)
            }
        } else {
            Err(format!("Folder dialog exited with {}", output.status))
        }
    }
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("osascript")
            .arg("-e")
            .arg(format!(
                "choose folder with prompt {:?}",
                if prompt.is_empty() {
                    "Select folder"
                } else {
                    prompt
                }
            ))
            .output()
            .map_err(|err| format!("Failed to open folder dialog: {err}"))?;
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() {
                Err("Folder selection was cancelled".into())
            } else {
                Ok(text)
            }
        } else {
            Err(format!("Folder dialog exited with {}", output.status))
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if which::which("kdialog").is_ok() {
            let mut command = Command::new("kdialog");
            command.arg("--getexistingdirectory");
            if let Some(init) = init_dir.filter(|value| !value.trim().is_empty()) {
                command.arg(init);
            }
            command.arg("--title").arg(if prompt.is_empty() {
                "Select folder"
            } else {
                prompt
            });
            let output = command
                .output()
                .map_err(|err| format!("Failed to open folder dialog: {err}"))?;
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if text.is_empty() {
                    Err("Folder selection was cancelled".into())
                } else {
                    Ok(text)
                }
            } else {
                Err(format!("Folder dialog exited with {}", output.status))
            }
        } else if which::which("zenity").is_ok() {
            let output = Command::new("zenity")
                .arg("--file-selection")
                .arg("--directory")
                .arg("--title")
                .arg(if prompt.is_empty() {
                    "Select folder"
                } else {
                    prompt
                })
                .output()
                .map_err(|err| format!("Failed to open folder dialog: {err}"))?;
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if text.is_empty() {
                    Err("Folder selection was cancelled".into())
                } else {
                    Ok(text)
                }
            } else {
                Err(format!("Folder dialog exited with {}", output.status))
            }
        } else {
            Err("No supported folder dialog backend was found".into())
        }
    }
}

fn prompt_user_input_dialog(
    prompt: &str,
    default_value: &str,
    multiline: bool,
) -> Result<String, String> {
    #[cfg(test)]
    if let Some(result) = test_prompt_user_input_dialog(prompt, default_value, multiline) {
        return result;
    }

    #[cfg(target_os = "windows")]
    {
        let script = format!(
            "Add-Type -AssemblyName Microsoft.VisualBasic; $v=[Microsoft.VisualBasic.Interaction]::InputBox(@'\n{}\n'@, 'Input', @'\n{}\n'@); Write-Output $v",
            prompt, default_value
        );
        let output = Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(script)
            .output()
            .map_err(|err| format!("Failed to open input dialog: {err}"))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout)
                .trim_end()
                .to_string())
        } else {
            Err(format!("Input dialog exited with {}", output.status))
        }
    }
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("osascript")
            .arg("-e")
            .arg(format!(
                "text returned of (display dialog {:?} default answer {:?} with title \"Input\")",
                prompt, default_value
            ))
            .output()
            .map_err(|err| format!("Failed to open input dialog: {err}"))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout)
                .trim_end()
                .to_string())
        } else {
            Err(format!("Input dialog exited with {}", output.status))
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if which::which("kdialog").is_ok() {
            let mut command = Command::new("kdialog");
            if multiline {
                command.arg("--textinputbox");
            } else {
                command.arg("--inputbox");
            }
            let output = command
                .arg(if prompt.is_empty() { "Input" } else { prompt })
                .arg(default_value)
                .output()
                .map_err(|err| format!("Failed to open input dialog: {err}"))?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout)
                    .trim_end()
                    .to_string())
            } else {
                Err(format!("Input dialog exited with {}", output.status))
            }
        } else if which::which("zenity").is_ok() {
            let output = Command::new("zenity")
                .arg("--entry")
                .arg("--title")
                .arg("Input")
                .arg("--text")
                .arg(if prompt.is_empty() { "Input" } else { prompt })
                .arg("--entry-text")
                .arg(default_value)
                .output()
                .map_err(|err| format!("Failed to open input dialog: {err}"))?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout)
                    .trim_end()
                    .to_string())
            } else {
                Err(format!("Input dialog exited with {}", output.status))
            }
        } else {
            Err("No supported input dialog backend was found".into())
        }
    }
}

fn normalize_runtime_path(path: &str) -> String {
    let expanded = path.trim().replace('\\', std::path::MAIN_SEPARATOR_STR);
    expanded
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DownloadRequestOptions {
    user_agent: Option<String>,
    headers: Vec<(String, String)>,
    cookie: Option<String>,
}

impl DownloadRequestOptions {
    fn from_inputs(
        user_agent: Option<String>,
        header_blob: Option<String>,
        cookie: Option<String>,
    ) -> Self {
        Self {
            user_agent,
            headers: parse_download_headers(header_blob.as_deref()),
            cookie,
        }
    }
}

fn parse_download_headers(header_blob: Option<&str>) -> Vec<(String, String)> {
    header_blob
        .into_iter()
        .flat_map(|blob| blob.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            let name = name.trim();
            let value = value.trim();
            (!name.is_empty()).then(|| (name.to_string(), value.to_string()))
        })
        .collect()
}

#[cfg(target_os = "windows")]
fn ps_single_quote(input: &str) -> String {
    input.replace('\'', "''")
}

fn download_to_file(
    url: &str,
    save_dir: &str,
    save_name: &str,
    options: &DownloadRequestOptions,
    control: Option<&ActionExecutionControl>,
) -> Result<String, String> {
    #[cfg(test)]
    if let Some(result) = test_download_to_file(url, save_dir, save_name, options) {
        return result;
    }

    const DOWNLOAD_CONNECT_TIMEOUT_SECS: &str = "30";
    const DOWNLOAD_MAX_TIME_SECS: &str = "180";
    const DEFAULT_DOWNLOAD_USER_AGENT: &str =
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/123.0.0.0 Safari/537.36";

    let save_dir = normalize_runtime_path(save_dir);
    fs::create_dir_all(&save_dir)
        .map_err(|err| format!("Failed to create download directory '{}': {err}", save_dir))?;
    let target = Path::new(&save_dir).join(save_name);
    let user_agent = options
        .user_agent
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(DEFAULT_DOWNLOAD_USER_AGENT);

    #[cfg(target_os = "windows")]
    let status = {
        let headers_literal = options
            .headers
            .iter()
            .map(|(name, value)| format!("{}={}", ps_single_quote(name), ps_single_quote(value)))
            .collect::<Vec<_>>()
            .join(";");
        let headers_clause = if headers_literal.is_empty() {
            "$headers=@{};".to_string()
        } else {
            format!("$headers=@{{{}}};", headers_literal)
        };
        let cookie_clause = options
            .cookie
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|cookie| format!("$headers['Cookie']='{}';", ps_single_quote(cookie)))
            .unwrap_or_default();

        let mut command = Command::new("powershell");
        command.arg("-NoProfile").arg("-Command").arg(format!(
            "{}{}Invoke-WebRequest -Uri '{}' -OutFile '{}' -TimeoutSec {} -Headers $headers -UserAgent '{}'",
            headers_clause,
            cookie_clause,
            ps_single_quote(url),
            ps_single_quote(&target.to_string_lossy()),
            DOWNLOAD_MAX_TIME_SECS,
            ps_single_quote(user_agent)
        ));
        run_command_for_status(command, control, "download command")?
    };

    #[cfg(not(target_os = "windows"))]
    let status = if which::which("curl").is_ok() {
        let mut command = Command::new("curl");
        command
            .arg("-L")
            .arg("-fsS")
            .arg("--retry")
            .arg("2")
            .arg("--retry-delay")
            .arg("1")
            .arg("--retry-all-errors")
            .arg("--connect-timeout")
            .arg(DOWNLOAD_CONNECT_TIMEOUT_SECS)
            .arg("--max-time")
            .arg(DOWNLOAD_MAX_TIME_SECS)
            .arg("--user-agent")
            .arg(user_agent);
        for (name, value) in &options.headers {
            command.arg("-H").arg(format!("{name}: {value}"));
        }
        if let Some(cookie) = options
            .cookie
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            command.arg("--cookie").arg(cookie);
        }
        command.arg(url).arg("-o").arg(&target);
        run_command_for_status(command, control, "download command")?
    } else if which::which("wget").is_ok() {
        let mut command = Command::new("wget");
        command
            .arg("--timeout")
            .arg(DOWNLOAD_MAX_TIME_SECS)
            .arg("--tries")
            .arg("2")
            .arg("--user-agent")
            .arg(user_agent);
        for (name, value) in &options.headers {
            command.arg("--header").arg(format!("{name}: {value}"));
        }
        if let Some(cookie) = options
            .cookie
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            command.arg("--header").arg(format!("Cookie: {cookie}"));
        }
        command.arg("-O").arg(&target).arg(url);
        run_command_for_status(command, control, "download command")?
    } else {
        return Err("No supported download backend was found".into());
    };

    if status.success() {
        Ok(target.to_string_lossy().to_string())
    } else if status.code() == Some(28) {
        Err(format!(
            "Download timed out after {} seconds: {}",
            DOWNLOAD_MAX_TIME_SECS, url
        ))
    } else {
        Err(format!("Download command exited with {status}"))
    }
}

fn read_file_path_reference(path: &str) -> Result<String, String> {
    let normalized = normalize_runtime_path(path);
    if Path::new(&normalized).exists() {
        Ok(normalized)
    } else {
        Err(format!("File does not exist: {normalized}"))
    }
}

fn read_binary_file(path: &str) -> Result<Vec<u8>, String> {
    let normalized = normalize_runtime_path(path);
    let mut file = fs::File::open(&normalized)
        .map_err(|err| format!("Failed to open file '{}': {err}", normalized))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|err| format!("Failed to read file '{}': {err}", normalized))?;
    Ok(bytes)
}

fn image_dimensions(bytes: &[u8]) -> Result<(u32, u32), String> {
    if bytes.len() >= 24 && bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return Ok((width, height));
    }
    if bytes.len() >= 10 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        let width = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
        let height = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
        return Ok((width, height));
    }
    if bytes.len() >= 26 && bytes.starts_with(b"BM") {
        let width = u32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
        let height = u32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]);
        return Ok((width, height));
    }
    if bytes.len() >= 4 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        let mut idx = 2usize;
        while idx + 8 < bytes.len() {
            if bytes[idx] != 0xFF {
                idx += 1;
                continue;
            }
            let marker = bytes[idx + 1];
            idx += 2;
            if marker == 0xD8 || marker == 0xD9 {
                continue;
            }
            if idx + 2 > bytes.len() {
                break;
            }
            let segment_len = u16::from_be_bytes([bytes[idx], bytes[idx + 1]]) as usize;
            if segment_len < 2 || idx + segment_len > bytes.len() {
                break;
            }
            if matches!(
                marker,
                0xC0 | 0xC1
                    | 0xC2
                    | 0xC3
                    | 0xC5
                    | 0xC6
                    | 0xC7
                    | 0xC9
                    | 0xCA
                    | 0xCB
                    | 0xCD
                    | 0xCE
                    | 0xCF
            ) {
                let height = u16::from_be_bytes([bytes[idx + 3], bytes[idx + 4]]) as u32;
                let width = u16::from_be_bytes([bytes[idx + 5], bytes[idx + 6]]) as u32;
                return Ok((width, height));
            }
            idx += segment_len;
        }
    }
    Err("Unsupported image format for imageinfo".into())
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | b2 as u32;
        output.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        output.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[(n & 0x3F) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn delete_file_path(path: &str) -> Result<(), String> {
    #[cfg(test)]
    if let Some(result) = test_delete_file_path(path) {
        return result;
    }

    let normalized = normalize_runtime_path(path);
    if !Path::new(&normalized).exists() {
        return Ok(());
    }
    fs::remove_file(&normalized)
        .map_err(|err| format!("Failed to delete file '{}': {err}", normalized))
}

fn derive_download_file_name(url: &str) -> String {
    let trimmed = url.trim();
    let without_query = trimmed
        .split(['?', '#'])
        .next()
        .unwrap_or(trimmed)
        .trim_end_matches('/');
    let candidate = without_query
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or("download.bin");
    if candidate.contains('.') {
        candidate.to_string()
    } else {
        format!("{candidate}.bin")
    }
}

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

fn cancellation_error() -> String {
    "Action cancelled".into()
}

fn ensure_not_cancelled(control: Option<&ActionExecutionControl>) -> Result<(), String> {
    if control.is_some_and(ActionExecutionControl::is_cancelled) {
        Err(cancellation_error())
    } else {
        Ok(())
    }
}

fn wait_for_child_cancelable(
    child: &mut Child,
    control: Option<&ActionExecutionControl>,
    context: &str,
) -> Result<ExitStatus, String> {
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|err| format!("Failed while waiting for {context}: {err}"))?
        {
            return Ok(status);
        }

        if control.is_some_and(ActionExecutionControl::is_cancelled) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(cancellation_error());
        }

        thread::sleep(Duration::from_millis(50));
    }
}

fn run_command_for_status(
    mut command: Command,
    control: Option<&ActionExecutionControl>,
    context: &str,
) -> Result<ExitStatus, String> {
    let mut child = command
        .spawn()
        .map_err(|err| format!("Failed to start {context}: {err}"))?;
    wait_for_child_cancelable(&mut child, control, context)
}

fn run_command_for_output(
    mut command: Command,
    control: Option<&ActionExecutionControl>,
    context: &str,
) -> Result<Output, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|err| format!("Failed to start {context}: {err}"))?;
    wait_for_child_cancelable(&mut child, control, context)?;
    child
        .wait_with_output()
        .map_err(|err| format!("Failed to collect {context} output: {err}"))
}

fn run_shell_command(
    script: &str,
    shell: &str,
    control: Option<&ActionExecutionControl>,
) -> ExecResult {
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

    let mut command = Command::new(sh);
    command.arg(flag).arg(script);

    match run_command_for_output(command, control, &format!("shell '{shell}'")) {
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

fn send_key_combo(modifiers: &[String], key: &str) -> Result<(), String> {
    #[cfg(test)]
    if let Some(result) = test_send_key_combo(modifiers, key) {
        return result;
    }

    let xdotool = which::which("xdotool")
        .map_err(|_| "Quicker key automation requires xdotool on this system".to_string())?;
    let chord = if modifiers.is_empty() {
        key.to_string()
    } else {
        format!("{}+{key}", modifiers.join("+"))
    };

    Command::new(xdotool)
        .arg("key")
        .arg("--clearmodifiers")
        .arg(chord)
        .status()
        .map_err(|err| format!("Failed to invoke xdotool: {err}"))
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(format!("xdotool exited with {status}"))
            }
        })
}

fn type_input_text(text: &str) -> Result<(), String> {
    #[cfg(test)]
    if let Some(result) = test_type_input_text(text) {
        return result;
    }

    let xdotool = which::which("xdotool")
        .map_err(|_| "Quicker text automation requires xdotool on this system".to_string())?;

    Command::new(xdotool)
        .arg("type")
        .arg("--delay")
        .arg("0")
        .arg("--clearmodifiers")
        .arg(text)
        .status()
        .map_err(|err| format!("Failed to invoke xdotool: {err}"))
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(format!("xdotool exited with {status}"))
            }
        })
}

fn sleep_millis(delay_ms: u64, control: Option<&ActionExecutionControl>) -> Result<(), String> {
    #[cfg(test)]
    {
        if test_sleep_millis(delay_ms) {
            return ensure_not_cancelled(control);
        }
    }

    let mut remaining = delay_ms;
    while remaining > 0 {
        ensure_not_cancelled(control)?;
        let slice = remaining.min(50);
        thread::sleep(Duration::from_millis(slice));
        remaining -= slice;
    }
    Ok(())
}

fn parse_quicker_action_document(input: &str) -> Result<QuickerActionDocument, String> {
    parse_json_lenient(input, "Failed to parse Quicker plugin JSON")
}

fn parse_json_lenient<T: DeserializeOwned>(input: &str, context: &str) -> Result<T, String> {
    let trimmed = input.trim_start_matches('\u{feff}');
    match serde_json::from_str(trimmed) {
        Ok(value) => Ok(value),
        Err(_) => {
            let sanitized = sanitize_json_control_chars(trimmed);
            serde_json::from_str(&sanitized).map_err(|err| format!("{context}: {err}"))
        }
    }
}

fn sanitize_json_control_chars(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if in_string {
            if escaped {
                output.push(ch);
                escaped = false;
                continue;
            }

            match ch {
                '\\' => {
                    output.push(ch);
                    escaped = true;
                }
                '"' => {
                    output.push(ch);
                    in_string = false;
                }
                '\n' => output.push_str("\\n"),
                '\r' => output.push_str("\\r"),
                '\t' => output.push_str("\\t"),
                ch if ch.is_control() => {
                    use std::fmt::Write as _;
                    let _ = write!(output, "\\u{:04x}", ch as u32);
                }
                _ => output.push(ch),
            }
        } else {
            if ch == '"' {
                in_string = true;
            }
            output.push(ch);
        }
    }

    output
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
    clipboard_html_writes: Vec<(String, Option<String>)>,
    clipboard_html_write_results: VecDeque<Result<(), String>>,
    standard_clipboard_reads: VecDeque<Option<String>>,
    primary_clipboard_reads: VecDeque<Option<String>>,
    html_clipboard_reads: VecDeque<Result<String, String>>,
    shell_calls: Vec<(String, String)>,
    shell_results: VecDeque<ExecResult>,
    key_calls: Vec<(Vec<String>, String)>,
    key_results: VecDeque<Result<(), String>>,
    typed_inputs: Vec<String>,
    typed_input_results: VecDeque<Result<(), String>>,
    delays: Vec<u64>,
    action_state_store: ActionStateStore,
    message_boxes: Vec<(String, String)>,
    message_box_results: VecDeque<Result<(), String>>,
    folder_dialog_results: VecDeque<Result<String, String>>,
    input_dialog_results: VecDeque<Result<String, String>>,
    download_calls: Vec<(String, String, String, DownloadRequestOptions)>,
    download_results: VecDeque<Result<String, String>>,
    deleted_paths: Vec<String>,
    delete_results: VecDeque<Result<(), String>>,
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
fn test_write_clipboard_html(html: &str, alt_text: Option<&str>) -> Option<Result<(), String>> {
    with_action_test_runtime(|runtime| {
        runtime
            .clipboard_html_writes
            .push((html.into(), alt_text.map(str::to_string)));
        runtime.clipboard_html_write_results.pop_front()
    })
}

#[cfg(test)]
fn test_read_clipboard_html() -> Option<Result<String, String>> {
    with_action_test_runtime(|runtime| runtime.html_clipboard_reads.pop_front())
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
fn test_send_key_combo(modifiers: &[String], key: &str) -> Option<Result<(), String>> {
    with_action_test_runtime(|runtime| {
        runtime.key_calls.push((modifiers.to_vec(), key.into()));
        runtime.key_results.pop_front()
    })
}

#[cfg(test)]
fn test_type_input_text(text: &str) -> Option<Result<(), String>> {
    with_action_test_runtime(|runtime| {
        runtime.typed_inputs.push(text.into());
        runtime.typed_input_results.pop_front()
    })
}

#[cfg(test)]
fn test_sleep_millis(delay_ms: u64) -> bool {
    with_action_test_runtime(|runtime| {
        runtime.delays.push(delay_ms);
    });
    true
}

#[cfg(test)]
fn test_load_action_state_scope(scope: &str) -> Option<HashMap<String, String>> {
    Some(with_action_test_runtime(|runtime| {
        runtime
            .action_state_store
            .get(scope)
            .cloned()
            .unwrap_or_default()
    }))
}

#[cfg(test)]
fn test_save_action_state_scope(scope: &str, state: &HashMap<String, String>) -> bool {
    with_action_test_runtime(|runtime| {
        runtime
            .action_state_store
            .insert(scope.to_string(), state.clone());
    });
    true
}

#[cfg(test)]
fn test_show_message_box(title: &str, message: &str) -> Option<Result<(), String>> {
    with_action_test_runtime(|runtime| {
        runtime
            .message_boxes
            .push((title.to_string(), message.to_string()));
        runtime.message_box_results.pop_front()
    })
}

#[cfg(test)]
fn test_select_folder_dialog(
    _prompt: &str,
    _init_dir: Option<&str>,
) -> Option<Result<String, String>> {
    with_action_test_runtime(|runtime| runtime.folder_dialog_results.pop_front())
}

#[cfg(test)]
fn test_prompt_user_input_dialog(
    _prompt: &str,
    _default_value: &str,
    _multiline: bool,
) -> Option<Result<String, String>> {
    with_action_test_runtime(|runtime| runtime.input_dialog_results.pop_front())
}

#[cfg(test)]
fn test_download_to_file(
    url: &str,
    save_dir: &str,
    save_name: &str,
    options: &DownloadRequestOptions,
) -> Option<Result<String, String>> {
    with_action_test_runtime(|runtime| {
        runtime.download_calls.push((
            url.to_string(),
            save_dir.to_string(),
            save_name.to_string(),
            options.clone(),
        ));
    });

    let result = with_action_test_runtime(|runtime| runtime.download_results.pop_front());
    match result {
        Some(Ok(path)) => {
            let normalized = normalize_runtime_path(&path);
            if let Some(parent) = Path::new(&normalized).parent() {
                let _ = fs::create_dir_all(parent);
            }
            let png_1x1 = [
                0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H',
                b'D', b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
                0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, b'I', b'D', b'A', b'T', 0x08,
                0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D,
                0x18, 0x00, 0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
            ];
            let _ = fs::write(&normalized, png_1x1);
            Some(Ok(path))
        }
        other => other,
    }
}

#[cfg(test)]
fn test_delete_file_path(path: &str) -> Option<Result<(), String>> {
    with_action_test_runtime(|runtime| {
        runtime.deleted_paths.push(path.to_string());
        runtime.delete_results.pop_front()
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

    fn sample(path: &str) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn quicker_plugin_document_parses_sample_json() {
        let sample = sample("sample/统一格式_20260319_095632.json");
        let document: QuickerActionDocument =
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
                    quicker_json: sample("sample/统一格式_20260319_095632.json"),
                },
            },
        };

        let json = action
            .to_quicker_plugin_json()
            .expect("plugin export should serialize");
        let document: QuickerActionDocument =
            serde_json::from_str(&json).expect("export should be valid Quicker JSON");
        let data = document
            .data_payload()
            .expect("exported data payload should parse");

        assert_eq!(document.action_type, QUICKER_PLUGIN_ACTION_TYPE);
        assert_eq!(document.title, "统一格式");
        assert_eq!(document.description, "将粘贴/导入内容的自带样式去除");
        assert_eq!(document.enable_evaluate_variable, Some(true));
        assert_eq!(data.variables.len(), 6);
        assert_eq!(data.steps.len(), 17);
    }

    #[test]
    fn quicker_plugin_round_trips_as_native_json() {
        let sample = sample("sample/统一格式_20260319_095632.json");

        let parsed = Action::from_quicker_plugin_json(&sample).expect("sample should parse");

        assert_eq!(parsed.name, "统一格式");
        assert_eq!(parsed.description, "将粘贴/导入内容的自带样式去除");
        assert_eq!(
            parsed.icon.as_deref(),
            Some(
                "https://files.getquicker.net/_icons/2D62F4E62FD40AC3F99CB7ABE05B9E2FAE141A3B.png"
            )
        );
        assert_eq!(
            parsed.to_quicker_plugin_json().unwrap(),
            Action::from_quicker_plugin_json(&sample)
                .unwrap()
                .to_quicker_plugin_json()
                .unwrap()
        );
    }

    #[test]
    fn quicker_plugin_executes_open_url_sample() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| runtime.open_results.push_back(Ok(())));

        let action =
            Action::from_quicker_plugin_json(&sample("sample/快捷键_20260319_105627.json"))
                .expect("sample should parse");

        let result = action.execute();

        assert_eq!(result, ExecResult::Ok);
        with_action_test_runtime(|runtime| {
            assert_eq!(
                runtime.opened_targets,
                vec!["https://www.yuque.com/supermemo/wiki/keyboard-shortcuts"]
            );
        });
    }

    #[test]
    fn quicker_plugin_accepts_legacy_key_macro_sample() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| runtime.key_results.push_back(Ok(())));

        let action = Action::from_quicker_plugin_json(&sample("sample/定位_20260319_105649.json"))
            .expect("sample should parse");

        let result = action.execute();

        assert_eq!(result, ExecResult::Ok);
        with_action_test_runtime(|runtime| {
            assert_eq!(runtime.key_calls, vec![(vec!["alt".into()], "c".into())]);
        });
    }

    #[test]
    fn quicker_plugin_accepts_legacy_launch_sample() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| runtime.open_results.push_back(Ok(())));

        let action =
            Action::from_quicker_plugin_json(&sample("sample/ScreenToGif_20260319_095543.json"))
                .expect("sample should parse");

        let result = action.execute();

        assert_eq!(result, ExecResult::Ok);
        with_action_test_runtime(|runtime| {
            assert_eq!(
                runtime.opened_targets,
                vec!["C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\ScreenToGif.lnk"]
            );
        });
    }

    #[test]
    fn quicker_plugin_executes_clipboard_pipeline_steps() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime.key_results.push_back(Ok(()));
            runtime
                .html_clipboard_reads
                .push_back(Ok(r#"<img data-latex-code="x^2">"#.into()));
            runtime.clipboard_write_results.push_back(Ok(()));
            runtime.key_results.push_back(Ok(()));
        });

        let action =
            Action::from_quicker_plugin_json(&sample("sample/图片转公式_20260319_105527.json"))
                .expect("sample should parse");

        let result = action.execute();

        assert_eq!(result, ExecResult::Ok);
        with_action_test_runtime(|runtime| {
            assert_eq!(runtime.clipboard_writes, vec!["x^2"]);
            assert_eq!(
                runtime.key_calls,
                vec![
                    (vec!["ctrl".into()], "x".into()),
                    (vec!["ctrl".into()], "v".into())
                ]
            );
        });
    }

    #[test]
    fn quicker_plugin_executes_state_storage_steps() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime.action_state_store.insert(
                "state-demo".into(),
                HashMap::from([("path".into(), "/tmp/demo".into())]),
            );
        });

        let sample = r#"{
          "ActionType": 24,
          "Title": "State Demo",
          "Description": "",
          "Icon": null,
          "Id": "state-demo",
          "Data": "{\"LimitSingleInstance\":false,\"SummaryExpression\":\"\",\"SubPrograms\":[],\"Variables\":[{\"Key\":\"path\",\"Type\":0,\"DefaultValue\":\"\",\"SaveState\":false},{\"Key\":\"is_empty\",\"Type\":2,\"DefaultValue\":\"\",\"SaveState\":false}],\"Steps\":[{\"StepRunnerKey\":\"sys:stateStorage\",\"InputParams\":{\"type\":{\"VarKey\":null,\"Value\":\"readActionState\"},\"key\":{\"VarKey\":null,\"Value\":\"path\"},\"defaultValue\":{\"VarKey\":null,\"Value\":\"fallback\"}},\"OutputParams\":{\"value\":\"path\",\"isEmpty\":\"is_empty\"},\"IfSteps\":null,\"ElseSteps\":null,\"Disabled\":false,\"Collapsed\":false,\"DelayMs\":0},{\"StepRunnerKey\":\"sys:stateStorage\",\"InputParams\":{\"type\":{\"VarKey\":null,\"Value\":\"saveActionState\"},\"key\":{\"VarKey\":null,\"Value\":\"path\"},\"value\":{\"VarKey\":null,\"Value\":\"/tmp/updated\"}},\"OutputParams\":{},\"IfSteps\":null,\"ElseSteps\":null,\"Disabled\":false,\"Collapsed\":false,\"DelayMs\":0}]}"
        }"#;

        let action = Action::from_quicker_plugin_json(sample).expect("sample should parse");
        let result = action.execute();

        assert_eq!(result, ExecResult::Ok);
        with_action_test_runtime(|runtime| {
            assert_eq!(
                runtime
                    .action_state_store
                    .get("state-demo")
                    .and_then(|scope| scope.get("path"))
                    .map(String::as_str),
                Some("/tmp/updated")
            );
        });
    }

    #[test]
    fn quicker_plugin_executes_formula_to_image_sample() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime.message_box_results.push_back(Ok(()));
            runtime
                .folder_dialog_results
                .push_back(Ok("/tmp/quicker-formula".into()));
            runtime.input_dialog_results.push_back(Ok("x^2".into()));
            runtime
                .download_results
                .push_back(Ok("/tmp/quicker-formula/latex_tmp.jpg".into()));
            runtime.clipboard_html_write_results.push_back(Ok(()));
            runtime.key_results.push_back(Ok(()));
        });

        let action =
            Action::from_quicker_plugin_json(&sample("sample/公式转图片_20260319_105519.json"))
                .expect("sample should parse");

        let result = action.execute();

        assert_eq!(result, ExecResult::Ok);
        with_action_test_runtime(|runtime| {
            assert_eq!(runtime.message_boxes.len(), 1);
            assert_eq!(
                runtime
                    .action_state_store
                    .get("f61295ca-e37e-410b-a401-da0510fc1e88")
                    .and_then(|scope| scope.get("path"))
                    .map(String::as_str),
                Some("/tmp/quicker-formula")
            );
            assert_eq!(runtime.clipboard_html_writes.len(), 1);
            assert_eq!(runtime.key_calls, vec![(vec!["ctrl".into()], "v".into())]);
        });
    }

    #[test]
    fn quicker_plugin_download_step_allows_missing_save_name() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime
                .download_results
                .push_back(Ok("/tmp/quicker-download/test.image.latex.php.bin".into()));
        });

        let sample = r#"{
          "ActionType": 24,
          "Title": "Download Demo",
          "Description": "",
          "Icon": null,
          "Data": "{\"LimitSingleInstance\":false,\"SummaryExpression\":\"\",\"SubPrograms\":[],\"Variables\":[],\"Steps\":[{\"StepRunnerKey\":\"sys:download\",\"InputParams\":{\"url\":{\"VarKey\":null,\"Value\":\"https://latex.vimsky.com/test.image.latex.php?fmt=png&val=x&dl=1\"},\"savePath\":{\"VarKey\":null,\"Value\":\"/tmp/quicker-download\"},\"stopIfFail\":{\"VarKey\":null,\"Value\":\"1\"}},\"OutputParams\":{\"isSuccess\":\"ok\",\"savedPath\":\"path\"},\"IfSteps\":null,\"ElseSteps\":null,\"Disabled\":false,\"Collapsed\":false,\"DelayMs\":0}]}"
        }"#;

        let action = Action::from_quicker_plugin_json(sample).expect("sample should parse");
        let result = action.execute();

        assert_eq!(result, ExecResult::Ok);
        with_action_test_runtime(|runtime| {
            assert_eq!(
                runtime.download_calls,
                vec![(
                    "https://latex.vimsky.com/test.image.latex.php?fmt=png&val=x&dl=1".into(),
                    "/tmp/quicker-download".into(),
                    "test.image.latex.php".into(),
                    DownloadRequestOptions {
                        user_agent: None,
                        headers: Vec::new(),
                        cookie: None,
                    }
                )]
            );
        });
    }

    #[test]
    fn quicker_plugin_download_step_passes_request_options() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime
                .download_results
                .push_back(Ok("/tmp/quicker-download/latex.png".into()));
        });

        let sample = r#"{
          "ActionType": 24,
          "Title": "Download Demo",
          "Description": "",
          "Icon": null,
          "Data": "{\"LimitSingleInstance\":false,\"SummaryExpression\":\"\",\"SubPrograms\":[],\"Variables\":[],\"Steps\":[{\"StepRunnerKey\":\"sys:download\",\"InputParams\":{\"url\":{\"VarKey\":null,\"Value\":\"https://latex.vimsky.com/test.image.latex.php?fmt=png&val=x&dl=1\"},\"savePath\":{\"VarKey\":null,\"Value\":\"/tmp/quicker-download\"},\"saveName\":{\"VarKey\":null,\"Value\":\"latex.png\"},\"ua\":{\"VarKey\":null,\"Value\":\"Custom UA\"},\"header\":{\"VarKey\":null,\"Value\":\"Accept: image/png\\r\\nX-Test: 1\"},\"cookie\":{\"VarKey\":null,\"Value\":\"sid=abc\"},\"stopIfFail\":{\"VarKey\":null,\"Value\":\"1\"}},\"OutputParams\":{\"isSuccess\":\"ok\",\"savedPath\":\"path\"},\"IfSteps\":null,\"ElseSteps\":null,\"Disabled\":false,\"Collapsed\":false,\"DelayMs\":0}]}"
        }"#;

        let action = Action::from_quicker_plugin_json(sample).expect("sample should parse");
        let result = action.execute();

        assert_eq!(result, ExecResult::Ok);
        with_action_test_runtime(|runtime| {
            assert_eq!(
                runtime.download_calls,
                vec![(
                    "https://latex.vimsky.com/test.image.latex.php?fmt=png&val=x&dl=1".into(),
                    "/tmp/quicker-download".into(),
                    "latex.png".into(),
                    DownloadRequestOptions {
                        user_agent: Some("Custom UA".into()),
                        headers: vec![
                            ("Accept".into(), "image/png".into()),
                            ("X-Test".into(), "1".into()),
                        ],
                        cookie: Some("sid=abc".into()),
                    }
                )]
            );
        });
    }

    #[test]
    fn low_code_draft_imports_supported_plugin_json() {
        let draft = LowCodePluginDraft::from_quicker_plugin_json(&sample(
            "sample/快捷键_20260319_105627.json",
        ))
        .expect("sample should import");

        assert_eq!(draft.title, "快捷键");
        assert_eq!(draft.kind, LowCodePluginKind::PluginFlow);
        assert_eq!(draft.steps.len(), 1);
        assert_eq!(
            draft.steps,
            vec![LowCodePluginStep::OpenUrl {
                url: "https://www.yuque.com/supermemo/wiki/keyboard-shortcuts".into()
            }]
        );
    }

    #[test]
    fn low_code_draft_exports_into_executable_plugin_action() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| runtime.open_results.push_back(Ok(())));

        let draft = LowCodePluginDraft {
            kind: LowCodePluginKind::PluginFlow,
            title: "Docs".into(),
            description: "open docs".into(),
            icon: Some("fa:Light_Keyboard".into()),
            key_macro_steps: Vec::new(),
            launch_path: String::new(),
            launch_arguments: String::new(),
            launch_set_working_dir: false,
            steps: vec![LowCodePluginStep::OpenUrl {
                url: "https://example.com/docs".into(),
            }],
        };

        let action = draft.to_action().expect("draft should export");
        let result = action.execute();

        assert_eq!(result, ExecResult::Ok);
        with_action_test_runtime(|runtime| {
            assert_eq!(runtime.opened_targets, vec!["https://example.com/docs"]);
        });
    }

    #[test]
    fn low_code_draft_imports_key_macro_json() {
        let draft = LowCodePluginDraft::from_quicker_plugin_json(&sample(
            "sample/定位_20260319_105649.json",
        ))
        .expect("key macro should import");

        assert_eq!(draft.kind, LowCodePluginKind::KeyMacro);
        assert_eq!(
            draft.key_macro_steps,
            vec![LowCodeKeyMacroStep::SendKeys {
                modifiers: "alt".into(),
                key: "C".into(),
            }]
        );
    }

    #[test]
    fn low_code_draft_imports_open_app_json() {
        let draft = LowCodePluginDraft::from_quicker_plugin_json(&sample(
            "sample/ScreenToGif_20260319_095543.json",
        ))
        .expect("launcher should import");

        assert_eq!(draft.kind, LowCodePluginKind::OpenApp);
        assert_eq!(
            draft.launch_path,
            "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\ScreenToGif.lnk"
        );
    }

    #[test]
    fn low_code_draft_imports_formula_to_image_json() {
        let draft = LowCodePluginDraft::from_quicker_plugin_json(&sample(
            "sample/公式转图片_20260319_105519.json",
        ))
        .expect("formula sample should import");

        assert_eq!(draft.kind, LowCodePluginKind::PluginFlow);
        assert_eq!(draft.title, "公式转图片");
        assert!(matches!(
            draft.steps.first(),
            Some(LowCodePluginStep::StateStorageRead { .. })
        ));
        assert!(draft
            .steps
            .iter()
            .any(|step| matches!(step, LowCodePluginStep::SimpleIf { .. })));
        assert!(draft
            .steps
            .iter()
            .any(|step| matches!(step, LowCodePluginStep::ImageToBase64 { .. })));
    }

    #[test]
    fn quicker_json_parser_accepts_raw_control_chars_inside_strings() {
        let broken = "{\n  \"ActionType\": 7,\n  \"Title\": \"Demo\",\n  \"Description\": \"\",\n  \"Data\": \"line1\nline2\ttext\",\n  \"Icon\": null\n}";

        let action = Action::from_quicker_plugin_json(broken).expect("parser should sanitize");

        assert_eq!(action.name, "Demo");
    }

    #[test]
    fn quicker_plugin_data_parser_accepts_raw_control_chars_inside_inner_json() {
        let broken = "{\n  \"ActionType\": 24,\n  \"Title\": \"Demo\",\n  \"Description\": \"\",\n  \"Icon\": null,\n  \"Data\": \"{\\\"LimitSingleInstance\\\":false,\\\"SummaryExpression\\\":\\\"\\\",\\\"SubPrograms\\\":[],\\\"Variables\\\":[{\\\"Key\\\":\\\"html\\\",\\\"Type\\\":0,\\\"DefaultValue\\\":\\\"line1\nline2\\\",\\\"SaveState\\\":false}],\\\"Steps\\\":[]}\" \n}";

        let draft = LowCodePluginDraft::from_quicker_plugin_json(broken)
            .expect("inner parser should sanitize");

        assert_eq!(draft.kind, LowCodePluginKind::PluginFlow);
        assert_eq!(draft.title, "Demo");
    }

    #[test]
    fn low_code_draft_exports_key_macro_steps_into_executable_plugin_action() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime.key_results.push_back(Ok(()));
            runtime.typed_input_results.push_back(Ok(()));
        });

        let draft = LowCodePluginDraft {
            kind: LowCodePluginKind::KeyMacro,
            title: "Macro".into(),
            description: String::new(),
            icon: None,
            key_macro_steps: vec![
                LowCodeKeyMacroStep::SendKeys {
                    modifiers: "alt".into(),
                    key: "C".into(),
                },
                LowCodeKeyMacroStep::Delay { delay_ms: 300 },
                LowCodeKeyMacroStep::TypeText {
                    text: "demo".into(),
                },
            ],
            launch_path: String::new(),
            launch_arguments: String::new(),
            launch_set_working_dir: false,
            steps: Vec::new(),
        };

        let action = draft.to_action().expect("draft should export");
        let result = action.execute();

        assert_eq!(result, ExecResult::Ok);
        with_action_test_runtime(|runtime| {
            assert_eq!(runtime.key_calls, vec![(vec!["alt".into()], "c".into())]);
            assert_eq!(runtime.delays, vec![300]);
            assert_eq!(runtime.typed_inputs, vec!["demo"]);
        });
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
    fn quicker_plugin_can_be_cancelled_before_delay_step_runs() {
        reset_action_test_runtime();
        let control = ActionExecutionControl::new();
        control.cancel();

        let sample = r#"{
          "ActionType": 24,
          "Title": "Delay Demo",
          "Description": "",
          "Icon": null,
          "Data": "{\"LimitSingleInstance\":false,\"SummaryExpression\":\"\",\"SubPrograms\":[],\"Variables\":[],\"Steps\":[{\"StepRunnerKey\":\"sys:delay\",\"InputParams\":{\"delayMs\":{\"VarKey\":null,\"Value\":\"1000\"}},\"OutputParams\":{},\"IfSteps\":null,\"ElseSteps\":null,\"Disabled\":false,\"Collapsed\":false,\"DelayMs\":0},{\"StepRunnerKey\":\"sys:notify\",\"InputParams\":{\"msg\":{\"VarKey\":null,\"Value\":\"done\"}},\"OutputParams\":{},\"IfSteps\":null,\"ElseSteps\":null,\"Disabled\":false,\"Collapsed\":false,\"DelayMs\":0}]}"
        }"#;

        let action = Action::from_quicker_plugin_json(sample).expect("sample should parse");
        let result = action.execute_with_control(Some(&control));

        assert_eq!(result, ExecResult::Err("Action cancelled".into()));
        with_action_test_runtime(|runtime| {
            assert!(runtime.delays.is_empty());
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
            url_template: "https://example.com/?q={query}".into(),
        })
        .execute();

        assert_eq!(
            result,
            ExecResult::OkWithMessage("Searched for: hello world".into())
        );
        with_action_test_runtime(|runtime| {
            assert_eq!(
                runtime.opened_targets,
                vec!["https://example.com/?q=hello%20world"]
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
            fallback_search_url: None,
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
        with_action_test_runtime(|runtime| {
            runtime
                .standard_clipboard_reads
                .push_back(Some("/tmp".into()));
            runtime.open_results.push_back(Ok(()));
        });

        let result = action(ActionKind::OpenClipboardText {
            fallback_search_url: None,
        })
        .execute();

        assert_eq!(result, ExecResult::OkWithMessage("Opened: /tmp".into()));
        with_action_test_runtime(|runtime| {
            assert_eq!(runtime.opened_targets, vec!["/tmp"]);
        });
    }

    #[test]
    fn open_clipboard_text_uses_fallback_search() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime
                .standard_clipboard_reads
                .push_back(Some("not a url".into()));
            runtime.open_results.push_back(Ok(()));
        });

        let result = action(ActionKind::OpenClipboardText {
            fallback_search_url: Some("https://example.com/?q={query}".into()),
        })
        .execute();

        assert_eq!(
            result,
            ExecResult::OkWithMessage("Searched for: not a url".into())
        );
        with_action_test_runtime(|runtime| {
            assert_eq!(
                runtime.opened_targets,
                vec!["https://example.com/?q=not%20a%20url"]
            );
        });
    }

    #[test]
    fn open_clipboard_text_errors_without_fallback() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime
                .standard_clipboard_reads
                .push_back(Some("not a url".into()));
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
            runtime
                .shell_results
                .push_back(ExecResult::OkWithMessage("selected".into()));
        });

        let result = action(ActionKind::RunClipboardText { shell: "sh".into() }).execute();

        assert_eq!(result, ExecResult::OkWithMessage("selected".into()));
        with_action_test_runtime(|runtime| {
            assert_eq!(
                runtime.shell_calls,
                vec![("sh".into(), "echo selected".into())]
            );
        });
    }

    #[test]
    fn group_actions_are_not_executed() {
        let result = action(ActionKind::Group { actions: vec![] }).execute();

        assert_eq!(result, ExecResult::Ok);
    }

    #[test]
    fn search_text_includes_group_children() {
        let action = Action {
            name: "Group".into(),
            description: "Parent".into(),
            icon: None,
            tags: vec!["folder".into()],
            hotkey: None,
            kind: ActionKind::Group {
                actions: vec![Action {
                    name: "Child".into(),
                    description: "Nested".into(),
                    icon: None,
                    tags: vec!["inner".into()],
                    hotkey: None,
                    kind: ActionKind::CopyText {
                        text: "copy".into(),
                    },
                }],
            },
        };

        let text = action.search_text();

        assert!(text.contains("Group"));
        assert!(text.contains("Child"));
        assert!(text.contains("Nested"));
    }
}
