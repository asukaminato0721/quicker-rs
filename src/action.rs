use serde::{Deserialize, Serialize};
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Run an ordered text-processing pipeline against the current selection
    PluginPipeline { steps: Vec<TextPluginStep> },
    /// A group/folder that contains sub-actions
    Group { actions: Vec<Action> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TextPluginStep {
    ReadSelectedText,
    ReadClipboardText,
    Uppercase,
    WriteSelectedText,
    WriteClipboardText,
    PromptInput {
        #[serde(default = "default_prompt_title")]
        title: String,
        #[serde(default)]
        default_value: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPromptRequest {
    pub title: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPluginRun {
    pub steps: Vec<TextPluginStep>,
    pub next_step_idx: usize,
    pub current_text: String,
    pub wrote_to_selection: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginRunOutcome {
    Complete(ExecResult),
    NeedsInput {
        prompt: PluginPromptRequest,
        pending: PendingPluginRun,
    },
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

fn default_prompt_title() -> String {
    "Input".into()
}

/// A single action in the launcher panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

            ActionKind::PluginPipeline { steps } => match run_plugin_pipeline(steps) {
                PluginRunOutcome::Complete(result) => result,
                PluginRunOutcome::NeedsInput { .. } => ExecResult::Err(
                    "Plugin pipelines that prompt for input must be run from the UI".into(),
                ),
            },

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
            ActionKind::PluginPipeline { steps } => {
                parts.extend(steps.iter().map(TextPluginStep::search_text));
            }
            _ => {}
        }
        parts.join(" ")
    }
}

impl TextPluginStep {
    pub fn label(&self) -> &'static str {
        match self {
            Self::ReadSelectedText => "Read Selected Text",
            Self::ReadClipboardText => "Read Clipboard Text",
            Self::Uppercase => "Uppercase",
            Self::WriteSelectedText => "Write Into Selected Part",
            Self::WriteClipboardText => "Write Clipboard Text",
            Self::PromptInput { .. } => "Prompt For Input",
        }
    }

    pub fn search_text(&self) -> String {
        match self {
            Self::PromptInput {
                title,
                default_value,
            } => {
                format!("{} {} {}", self.label(), title, default_value)
            }
            _ => self.label().into(),
        }
    }
}

pub fn run_plugin_pipeline(steps: &[TextPluginStep]) -> PluginRunOutcome {
    continue_plugin_pipeline(PendingPluginRun {
        steps: steps.to_vec(),
        next_step_idx: 0,
        current_text: String::new(),
        wrote_to_selection: false,
    })
}

pub fn continue_plugin_pipeline(mut pending: PendingPluginRun) -> PluginRunOutcome {
    while let Some(step) = pending.steps.get(pending.next_step_idx).cloned() {
        pending.next_step_idx += 1;

        match step {
            TextPluginStep::ReadSelectedText => match read_selected_text() {
                Ok(text) => pending.current_text = text,
                Err(err) => return PluginRunOutcome::Complete(ExecResult::Err(err)),
            },
            TextPluginStep::ReadClipboardText => match read_explicit_clipboard_text() {
                Ok(text) => pending.current_text = text,
                Err(err) => return PluginRunOutcome::Complete(ExecResult::Err(err)),
            },
            TextPluginStep::Uppercase => {
                pending.current_text = pending.current_text.to_uppercase();
            }
            TextPluginStep::WriteSelectedText => {
                if pending.current_text.is_empty() {
                    return PluginRunOutcome::Complete(ExecResult::Err(
                        "No text is available to write. Add a read or prompt step first.".into(),
                    ));
                }

                match write_text_into_selection(&pending.current_text) {
                    Ok(()) => pending.wrote_to_selection = true,
                    Err(err) => return PluginRunOutcome::Complete(ExecResult::Err(err)),
                }
            }
            TextPluginStep::WriteClipboardText => {
                match write_clipboard_text(&pending.current_text) {
                    Ok(()) => {}
                    Err(err) => return PluginRunOutcome::Complete(ExecResult::Err(err)),
                }
            }
            TextPluginStep::PromptInput {
                title,
                default_value,
            } => {
                let value = if pending.current_text.is_empty() {
                    default_value
                } else {
                    pending.current_text.clone()
                };
                return PluginRunOutcome::NeedsInput {
                    prompt: PluginPromptRequest { title, value },
                    pending,
                };
            }
        }
    }

    let result = if pending.wrote_to_selection {
        ExecResult::OkWithMessage(
            "Plugin pipeline finished and wrote into the selected part".into(),
        )
    } else if pending
        .steps
        .iter()
        .any(|step| matches!(step, TextPluginStep::WriteClipboardText))
    {
        ExecResult::OkWithMessage("Plugin pipeline finished and wrote to the clipboard".into())
    } else if pending.current_text.trim().is_empty() {
        ExecResult::OkWithMessage("Plugin pipeline finished".into())
    } else {
        ExecResult::OkWithMessage(pending.current_text)
    };

    PluginRunOutcome::Complete(result)
}

pub fn read_selected_text() -> Result<String, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("Clipboard error: {}", e))?;

    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
    ))]
    {
        if let Some(text) = read_primary_clipboard_text(&mut clipboard) {
            return Ok(text);
        }
    }

    if let Some(text) = read_standard_clipboard_text(&mut clipboard) {
        return Ok(text);
    }

    Err(
        "No selected text was found. On Linux, select text first before triggering the plugin."
            .into(),
    )
}

pub fn read_explicit_clipboard_text() -> Result<String, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("Clipboard error: {}", e))?;

    if let Some(text) = read_standard_clipboard_text(&mut clipboard) {
        return Ok(text);
    }

    Err("No clipboard text was found. Copy text first before triggering the plugin.".into())
}

pub fn write_text_into_selection(text: &str) -> Result<(), String> {
    #[cfg(test)]
    if let Some(result) = test_write_text_into_selection(text) {
        return result;
    }

    let mut backend_errors = Vec::new();

    if try_type_with_command("wtype", &[], text, &mut backend_errors).is_ok()
        || try_xdotool_type(text, &mut backend_errors).is_ok()
    {
        return Ok(());
    }

    write_clipboard_text(text)?;
    Err(format!(
        "Copied text to the clipboard, but automatic typing failed. {}",
        backend_errors.join(" ")
    ))
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

fn try_type_with_command(
    command: &str,
    prefix_args: &[&str],
    text: &str,
    backend_errors: &mut Vec<String>,
) -> Result<(), ()> {
    match Command::new(command).args(prefix_args).arg(text).output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let detail = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                format!("exited with status {}", output.status)
            };
            backend_errors.push(format!("{command}: {detail}."));
            Err(())
        }
        Err(err) => {
            backend_errors.push(format!("{command}: {err}."));
            Err(())
        }
    }
}

fn try_xdotool_type(text: &str, backend_errors: &mut Vec<String>) -> Result<(), ()> {
    match Command::new("xdotool")
        .args(["type", "--clearmodifiers", "--delay", "0", "--", text])
        .output()
    {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let detail = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                format!("exited with status {}", output.status)
            };
            backend_errors.push(format!("xdotool: {detail}."));
            Err(())
        }
        Err(err) => {
            backend_errors.push(format!("xdotool: {err}."));
            Err(())
        }
    }
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
    selection_writes: Vec<String>,
    selection_write_results: VecDeque<Result<(), String>>,
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
fn test_write_text_into_selection(text: &str) -> Option<Result<(), String>> {
    with_action_test_runtime(|runtime| {
        runtime.selection_writes.push(text.into());
        runtime.selection_write_results.pop_front()
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
    fn plugin_pipeline_reads_uppercases_and_writes_selected_text() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime
                .primary_clipboard_reads
                .push_back(Some("hello world".into()));
            runtime.selection_write_results.push_back(Ok(()));
        });

        let result = run_plugin_pipeline(&[
            TextPluginStep::ReadSelectedText,
            TextPluginStep::Uppercase,
            TextPluginStep::WriteSelectedText,
        ]);

        assert_eq!(
            result,
            PluginRunOutcome::Complete(ExecResult::OkWithMessage(
                "Plugin pipeline finished and wrote into the selected part".into()
            ))
        );
        with_action_test_runtime(|runtime| {
            assert_eq!(runtime.selection_writes, vec!["HELLO WORLD"]);
        });
    }

    #[test]
    fn plugin_pipeline_pauses_for_input_and_resumes() {
        reset_action_test_runtime();

        let paused = run_plugin_pipeline(&[
            TextPluginStep::PromptInput {
                title: "Enter text".into(),
                default_value: "seed".into(),
            },
            TextPluginStep::Uppercase,
        ]);

        let (prompt, pending) = match paused {
            PluginRunOutcome::NeedsInput { prompt, pending } => (prompt, pending),
            other => panic!("unexpected outcome: {other:?}"),
        };

        assert_eq!(
            prompt,
            PluginPromptRequest {
                title: "Enter text".into(),
                value: "seed".into(),
            }
        );

        let resumed = continue_plugin_pipeline(PendingPluginRun {
            current_text: "hello".into(),
            ..pending
        });

        assert_eq!(
            resumed,
            PluginRunOutcome::Complete(ExecResult::OkWithMessage("HELLO".into()))
        );
    }

    #[test]
    fn plugin_pipeline_reads_uppercases_and_writes_clipboard_text() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime
                .standard_clipboard_reads
                .push_back(Some("hello clip".into()));
            runtime.clipboard_write_results.push_back(Ok(()));
        });

        let result = run_plugin_pipeline(&[
            TextPluginStep::ReadClipboardText,
            TextPluginStep::Uppercase,
            TextPluginStep::WriteClipboardText,
        ]);

        assert_eq!(
            result,
            PluginRunOutcome::Complete(ExecResult::OkWithMessage(
                "Plugin pipeline finished and wrote to the clipboard".into()
            ))
        );
        with_action_test_runtime(|runtime| {
            assert_eq!(runtime.clipboard_writes, vec!["HELLO CLIP"]);
        });
    }

    #[test]
    fn read_selected_text_prefers_primary_selection_over_clipboard() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime
                .standard_clipboard_reads
                .push_back(Some("clipboard text".into()));
            runtime
                .primary_clipboard_reads
                .push_back(Some("selected text".into()));
        });

        let result = read_selected_text();

        assert_eq!(result, Ok("selected text".into()));
    }

    #[test]
    fn read_explicit_clipboard_text_uses_standard_clipboard_only() {
        reset_action_test_runtime();
        with_action_test_runtime(|runtime| {
            runtime
                .standard_clipboard_reads
                .push_back(Some("clipboard only".into()));
            runtime
                .primary_clipboard_reads
                .push_back(Some("selection".into()));
        });

        let result = read_explicit_clipboard_text();

        assert_eq!(result, Ok("clipboard only".into()));
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
