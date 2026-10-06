//! Built-in path tools for Quicker text input. Custom tools remain unsupported.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tool {
    File,
    Files,
    Folder,
    Save,
}

pub(super) fn parse(text: &str) -> Result<Vec<Tool>, String> {
    let mut tools = Vec::new();
    for name in text.split(',').filter(|name| !name.is_empty()) {
        let tool = match name.trim() {
            "SelectSingleFile" => Tool::File,
            "SelectMultiFile" => Tool::Files,
            "SelectSingleFolder" => Tool::Folder,
            "SelectSavePath" => Tool::Save,
            other => return Err(format!("Unsupported input text tool: {other}")),
        };
        if !tools.contains(&tool) {
            tools.push(tool);
        }
    }
    Ok(tools)
}

#[cfg(target_os = "linux")]
impl Tool {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::File => "Select file",
            Self::Files => "Select files",
            Self::Folder => "Select folder",
            Self::Save => "Save path",
        }
    }

    pub(crate) fn choose(
        self,
        current: &str,
        control: &ActionExecutionControl,
    ) -> Result<String, String> {
        ensure_not_cancelled(Some(control))?;
        match self {
            Self::Folder => select_folder_dialog(
                self.label(),
                Path::new(current).is_dir().then_some(current),
                Some(control),
            ),
            Self::File | Self::Files | Self::Save => {
                file_dialogs::choose_text_tool_file(self, current, control)
            }
        }
    }
}

pub(super) fn prompt(
    prompt: &str,
    initial: &str,
    multiline: bool,
    tools: &[Tool],
    restore: bool,
    control: Option<&ActionExecutionControl>,
) -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        let focus = dialogs::DialogFocus::capture(restore)?;
        let result = crate::input_windows::prompt(
            prompt,
            initial,
            multiline,
            tools,
            control.cloned().unwrap_or_default(),
        );
        ensure_not_cancelled(control)?;
        focus.restore(control)?;
        result
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (prompt, initial, multiline, tools, restore, control);
        Err("Input text tools require the native Linux application".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_tools_and_rejects_unknown_tools() {
        assert!(parse("").unwrap().is_empty());
        assert_eq!(parse("SelectMultiFile").unwrap(), vec![Tool::Files]);
        assert_eq!(
            parse("SelectSingleFile, SelectSingleFolder,,SelectSavePath,SelectSingleFile").unwrap(),
            vec![Tool::File, Tool::Folder, Tool::Save]
        );
        for text in [
            "SelectProcessPath",
            "SelectSingleFolder;SelectSavePath",
            "Custom",
            " ",
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn checker_accepts_path_tools_but_retains_advanced_option_blockers() {
        for (tools, extra, code) in [
            ("SelectSingleFolder", "", 0),
            ("SelectSingleFile,SelectMultiFile,SelectSavePath", "", 0),
            ("SelectProcessPath", "", 1),
            ("SelectSingleFolder", "{\"TextToolsReplaceMode\":0}", 1),
        ] {
            let report = compatibility::inspect(
                &serde_json::json!({
                    "ActionType":24, "Title":"Input tools", "Data": serde_json::json!({
                        "Steps":[{"StepRunnerKey":"sys:userInput", "InputParams":{
                            "texttools":{"Value":tools}, "extraSettings":{"Value":extra}
                        }}]
                    }).to_string()
                })
                .to_string(),
            );
            assert_eq!(compatibility::exit_code(&report), code, "{report}");
            assert_eq!(report["runtime"]["executed"], false);
        }
    }
}
