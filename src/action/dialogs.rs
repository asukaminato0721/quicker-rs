//! Quicker dialog outputs and cancellation. Native Linux dialogs use managed children.
use super::*;

pub(super) fn validate_option(runner: &str, key: &str, value: &Value) -> bool {
    let text = value_to_string(value);
    match (runner, key) {
        ("sys:MsgBox", "operation") => matches!(text.as_str(), "" | "default"),
        ("sys:MsgBox", "buttons") => {
            matches!(text.as_str(), "OK" | "OKCancel" | "YesNo" | "YesNoCancel")
        }
        ("sys:MsgBox", "icon") => matches!(
            text.as_str(),
            "None"
                | "Information"
                | "Asterisk"
                | "Question"
                | "Warning"
                | "Exclamation"
                | "Error"
                | "Hand"
                | "Stop"
        ),
        ("sys:userInput", "type") => matches!(text.as_str(), "text" | "multiline"),
        ("sys:userInput", "texttools") => input_tools::parse(&text).is_ok(),
        ("sys:userInput", "extraSettings" | "help" | "helpLink" | "fontfamily") => text.is_empty(),
        ("sys:userInput", "closeOnDeactivated" | "submitWithReturn" | "topMost") => {
            !truthy(Some(value))
        }
        ("sys:userInput", "winLocation") => matches!(text.as_str(), "" | "CenterScreen"),
        ("sys:userInput", "imeState") => matches!(text.as_str(), "" | "NO_CONTROL"),
        ("sys:userInput", "fontsize") => text.parse::<f64>() == Ok(14.0),
        _ => true,
    }
}

pub(super) const INPUT_OPTIONS: &[&str] = &[
    "type",
    "texttools",
    "extraSettings",
    "help",
    "helpLink",
    "fontfamily",
    "closeOnDeactivated",
    "submitWithReturn",
    "topMost",
    "winLocation",
    "imeState",
    "fontsize",
];

impl QuickerRuntime {
    fn check_dialog_options(
        &self,
        step: &QuickerPluginStepDocument,
        keys: &[&str],
    ) -> Result<(), String> {
        for key in keys {
            if let Some(value) = self.input_value(&step.input_params, key)? {
                if !validate_option(&step.step_runner_key, key, &value) {
                    return Err(format!(
                        "Unsupported {} option: {key}",
                        step.step_runner_key
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn run_message_box(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        ensure_not_cancelled(self.control.as_ref())?;
        self.check_dialog_options(step, &["operation", "buttons", "icon"])?;
        let title = self
            .input_string_opt(&step.input_params, "title")?
            .unwrap_or_else(|| "Quicker".into());
        let title = if title.is_empty() {
            &self.action_title
        } else {
            &title
        };
        let message = self.input_string(&step.input_params, "message")?;
        let buttons = self
            .input_string_opt(&step.input_params, "buttons")?
            .unwrap_or_else(|| "OK".into());
        let icon = self
            .input_string_opt(&step.input_params, "icon")?
            .unwrap_or_else(|| "Information".into());
        let restore = self
            .input_value(&step.input_params, "restoreFocus")?
            .is_none_or(|v| truthy(Some(&v)));
        let result = message_box(
            title,
            &message,
            &buttons,
            &icon,
            restore,
            self.control.as_ref(),
        )?;
        ensure_not_cancelled(self.control.as_ref())?;
        self.assign_output(
            &step.output_params,
            "okOrYes",
            Value::Bool(matches!(result.as_str(), "OK" | "Yes")),
        )?;
        self.assign_output(&step.output_params, "result", Value::String(result))?;
        Ok(StepFlow::Continue)
    }

    pub(super) fn run_folder_dialog(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        let stop = self
            .input_value(&step.input_params, "stopIfFail")?
            .is_none_or(|v| truthy(Some(&v)));
        ensure_not_cancelled(self.control.as_ref())?;
        let prompt = self
            .input_string_opt(&step.input_params, "prompt")?
            .unwrap_or_else(|| "请选择文件夹".into());
        let init = self.input_string_opt(&step.input_params, "initDir")?;
        let _dialog = DialogSession::new(self.control.as_ref());
        let result = select_folder_dialog(&prompt, init.as_deref(), self.control.as_ref());
        ensure_not_cancelled(self.control.as_ref())?;
        self.assign_output(
            &step.output_params,
            "path",
            Value::String(result.as_ref().cloned().unwrap_or_default()),
        )?;
        self.assign_output(
            &step.output_params,
            "isSuccess",
            Value::Bool(result.is_ok()),
        )?;
        if stop {
            result?;
        }
        Ok(StepFlow::Continue)
    }

    pub(super) fn run_input_dialog(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        let stop = self
            .input_value(&step.input_params, "stopIfFail")?
            .is_none_or(|v| truthy(Some(&v)));
        let result: Result<String, String> = (|| {
            ensure_not_cancelled(self.control.as_ref())?;
            self.check_dialog_options(step, INPUT_OPTIONS)?;
            let prompt = self
                .input_string_opt(&step.input_params, "prompt")?
                .unwrap_or_else(|| "请输入内容".into());
            let mut initial = self
                .input_string_opt(&step.input_params, "defaultValue")?
                .unwrap_or_default();
            let multiline = self
                .input_string_opt(&step.input_params, "type")?
                .as_deref()
                == Some("multiline");
            let required = self.input_bool(&step.input_params, "isRequired")?;
            let pattern = self
                .input_string_opt(&step.input_params, "pattern")?
                .unwrap_or_default();
            let pattern = if pattern.is_empty() {
                None
            } else {
                Some(regex_steps::compile(&pattern, false, false, false)?)
            };
            let restore = self
                .input_value(&step.input_params, "restoreFocus")?
                .is_none_or(|v| truthy(Some(&v)));
            let _dialog = DialogSession::new(self.control.as_ref());
            let tools = input_tools::parse(
                &self
                    .input_string_opt(&step.input_params, "texttools")?
                    .unwrap_or_default(),
            )?;
            loop {
                let text = if tools.is_empty() {
                    prompt_user_input_dialog(
                        &prompt,
                        &initial,
                        multiline,
                        restore,
                        self.control.as_ref(),
                    )?
                } else {
                    input_tools::prompt(
                        &prompt,
                        &initial,
                        multiline,
                        &tools,
                        restore,
                        self.control.as_ref(),
                    )?
                };
                ensure_not_cancelled(self.control.as_ref())?;
                let valid = if text.is_empty() {
                    !required
                } else {
                    pattern.as_ref().map_or(Ok(true), |p| {
                        p.is_match(&text)
                            .map_err(|e| format!("Input validation failed: {e}"))
                    })?
                };
                if valid {
                    return Ok(text);
                }
                message_box(
                    "Input",
                    "Enter a value that satisfies the input rule.",
                    "OK",
                    "Warning",
                    restore,
                    self.control.as_ref(),
                )?;
                initial = text;
            }
        })();
        ensure_not_cancelled(self.control.as_ref())?;
        let text = result.as_ref().cloned().unwrap_or_default();
        self.assign_output(
            &step.output_params,
            "isSuccess",
            Value::Bool(result.is_ok()),
        )?;
        self.assign_output(&step.output_params, "isEmpty", Value::Bool(text.is_empty()))?;
        self.assign_output(&step.output_params, "textValue", Value::String(text))?;
        if stop {
            result?;
        }
        Ok(StepFlow::Continue)
    }
}

pub(super) struct DialogSession {
    control: Option<ActionExecutionControl>,
    previous: bool,
}

impl DialogSession {
    pub(super) fn new(control: Option<&ActionExecutionControl>) -> Self {
        let previous = control.is_some_and(|c| c.dialog_active.swap(true, Ordering::SeqCst));
        Self {
            control: control.cloned(),
            previous,
        }
    }
}

impl Drop for DialogSession {
    fn drop(&mut self) {
        if let Some(control) = &self.control {
            control.dialog_active.store(self.previous, Ordering::SeqCst);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn decode_exact_output(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > 1024 * 1024 {
        return Err("Dialog output exceeds 1 MiB".into());
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| "Dialog returned invalid UTF-8".into())
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn decode_text_output(bytes: &[u8]) -> Result<String, String> {
    // kdialog and zenity entry/file dialogs append one newline, even to empty data.
    let text = decode_exact_output(bytes)?;
    #[cfg(target_os = "windows")]
    let suffix = "\r\n";
    #[cfg(not(target_os = "windows"))]
    let suffix = "\n";
    Ok(text.strip_suffix(suffix).unwrap_or(&text).to_owned())
}

pub(super) fn message_box(
    title: &str,
    message: &str,
    buttons: &str,
    icon: &str,
    restore: bool,
    control: Option<&ActionExecutionControl>,
) -> Result<String, String> {
    ensure_not_cancelled(control)?;
    let _dialog = DialogSession::new(control);
    #[cfg(test)]
    if let Some(result) = test_show_message_box(title, message) {
        return result;
    }
    #[cfg(target_os = "linux")]
    {
        let focus = DialogFocus::capture(restore)?;
        let backend = if which::which("kdialog").is_ok() {
            "kdialog"
        } else if which::which("zenity").is_ok() {
            "zenity"
        } else {
            return Err("Install kdialog or zenity to show workflow dialogs".into());
        };
        let result = run_message_backend(backend, title, message, buttons, icon, control);
        ensure_not_cancelled(control)?;
        focus.restore(control)?;
        result
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (icon, restore);
        if buttons != "OK" {
            return Err("This message box backend only supports OK".into());
        }
        show_message_box(title, message)?;
        ensure_not_cancelled(control)?;
        Ok("OK".into())
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn configure_zenity(command: &mut Command) {
    // Zenity allows users to replace its exit codes through environment variables.
    // Use fixed child values so Cancel cannot become a positive response.
    for (name, value) in [
        ("OK", "0"),
        ("CANCEL", "1"),
        ("ESC", "1"),
        ("EXTRA", "1"),
        ("ERROR", "255"),
        ("TIMEOUT", "5"),
    ] {
        command.env(format!("ZENITY_{name}"), value);
    }
}

#[cfg(target_os = "linux")]
fn run_message_backend(
    backend: &str,
    title: &str,
    message: &str,
    buttons: &str,
    icon: &str,
    control: Option<&ActionExecutionControl>,
) -> Result<String, String> {
    let mut command = Command::new(backend);
    command.args(["--title", title]);
    if backend == "zenity" {
        configure_zenity(&mut command);
    }
    let warning = matches!(icon, "Warning" | "Exclamation");
    if backend == "kdialog" {
        let mode = match buttons {
            "OK" if matches!(icon, "Error" | "Hand" | "Stop") => "--error",
            "OK" if warning => "--sorry",
            "OK" => "--msgbox",
            "OKCancel" | "YesNo" if warning => "--warningyesno",
            "OKCancel" | "YesNo" => "--yesno",
            "YesNoCancel" if warning => "--warningyesnocancel",
            "YesNoCancel" => "--yesnocancel",
            _ => return Err(format!("Unsupported message buttons: {buttons}")),
        };
        // Standard Quicker messages are plain text. Prevent Qt rich-text detection.
        let escaped = message
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('\n', "<br>");
        command.arg(mode).arg(format!("<qt>{escaped}</qt>"));
        if buttons == "OKCancel" {
            command.args(["--yes-label", "OK", "--no-label", "Cancel"]);
        }
    } else {
        command.args(["--no-markup", "--text", message]);
        if buttons == "OK" {
            command.arg(if matches!(icon, "Error" | "Hand" | "Stop") {
                "--error"
            } else if warning {
                "--warning"
            } else {
                "--info"
            });
        } else {
            command.arg("--question");
            match buttons {
                "OKCancel" => {
                    command.args(["--ok-label", "OK", "--cancel-label", "Cancel"]);
                }
                "YesNo" => {
                    command.args(["--ok-label", "Yes", "--cancel-label", "No"]);
                }
                "YesNoCancel" => {
                    command.args([
                        "--ok-label",
                        "Yes",
                        "--cancel-label",
                        "Cancel",
                        "--extra-button",
                        "_No",
                    ]);
                }
                _ => return Err(format!("Unsupported message buttons: {buttons}")),
            }
        }
    }
    let output = run_command_for_output(command, control, "message box")?;
    let result = match (backend, buttons, output.status.code()) {
        (_, "OK", Some(0 | 1)) => "OK",
        (_, "OKCancel", Some(0)) => "OK",
        (_, "OKCancel", Some(1)) | ("kdialog", "OKCancel", Some(2)) => "Cancel",
        (_, "YesNo" | "YesNoCancel", Some(0)) => "Yes",
        (_, "YesNo", Some(1))
        | ("kdialog", "YesNo", Some(2))
        | ("kdialog", "YesNoCancel", Some(1)) => "No",
        ("kdialog", "YesNoCancel", Some(2)) => "Cancel",
        ("zenity", "YesNoCancel", Some(1)) if output.stdout == b"_No\n" => "No",
        ("zenity", "YesNoCancel", Some(1)) => "Cancel",
        _ => return Err(format!("Message box failed with {}", output.status)),
    };
    Ok(result.into())
}

#[cfg(target_os = "linux")]
pub(super) struct DialogFocus(Option<crate::focus::FocusedProcess>);

#[cfg(target_os = "linux")]
impl DialogFocus {
    pub(super) fn capture(restore: bool) -> Result<Self, String> {
        if restore && crate::x11::is_wayland() {
            return Err(
                "Dialog focus restoration requires X11. Disable restoreFocus on Wayland".into(),
            );
        }
        Ok(Self(if restore {
            crate::x11::focused_process()
        } else {
            None
        }))
    }

    pub(super) fn restore(self, control: Option<&ActionExecutionControl>) -> Result<(), String> {
        if let Some(target) = self.0 {
            crate::x11::restore_focus(&target, &control.cloned().unwrap_or_default())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn runtime() -> QuickerRuntime {
        reset_action_test_runtime();
        QuickerRuntime::new(
            &serde_json::from_value(json!({})).unwrap(),
            "dialogs".into(),
            None,
        )
        .unwrap()
    }

    fn step(key: &str, inputs: Value, outputs: Value) -> QuickerPluginStepDocument {
        serde_json::from_value(
            json!({"StepRunnerKey": key, "InputParams": inputs, "OutputParams": outputs}),
        )
        .unwrap()
    }

    #[test]
    fn message_buttons_control_plugin_branches_and_default_title() {
        let mut rt = runtime();
        rt.action_title = "Actual action title".into();
        for (buttons, result, confirmed) in [
            ("OK", "OK", true),
            ("OKCancel", "Cancel", false),
            ("YesNo", "No", false),
            ("YesNo", "Yes", true),
            ("YesNoCancel", "Cancel", false),
        ] {
            with_action_test_runtime(|t| t.message_box_results.push_back(Ok(result.into())));
            let s = step(
                "sys:MsgBox",
                json!({"title":{"Value":""},"message":{"Value":"选择"},"buttons":{"Value":buttons}}),
                json!({"result":"button","okOrYes":"confirmed"}),
            );
            rt.run_message_box(&s).unwrap();
            assert_eq!(rt.vars["button"], result);
            assert_eq!(rt.vars["confirmed"], confirmed);
        }
        with_action_test_runtime(|t| {
            assert!(t.message_boxes.iter().all(|v| v.0 == "Actual action title"))
        });
    }

    #[test]
    fn user_input_preserves_whitespace_retries_validation_and_clears_cancelled_values() {
        let mut rt = runtime();
        let mut s = step(
            "sys:userInput",
            json!({}),
            json!({"textValue":"text","isEmpty":"empty","isSuccess":"ok"}),
        );
        with_action_test_runtime(|t| t.input_dialog_results.push_back(Ok(" \t\n".into())));
        rt.run_input_dialog(&s).unwrap();
        assert_eq!(rt.vars["text"], " \t\n");
        assert_eq!(rt.vars["empty"], false);
        s.input_params =
            serde_json::from_value(json!({"isRequired":{"Value":"1"},"pattern":{"Value":"^a+$"}}))
                .unwrap();
        with_action_test_runtime(|t| {
            for value in ["", "bad", "aaa"] {
                t.input_dialog_results.push_back(Ok(value.into()));
            }
            for _ in 0..2 {
                t.message_box_results.push_back(Ok("OK".into()));
            }
        });
        rt.run_input_dialog(&s).unwrap();
        assert_eq!(rt.vars["text"], "aaa");
        with_action_test_runtime(|t| {
            t.input_dialog_results
                .push_back(Err("Input dialog exited with 1".into()))
        });
        assert!(rt.run_input_dialog(&s).is_err()); // Missing stopIfFail defaults to true.
        assert_eq!(rt.vars["text"], "");
        assert_eq!(rt.vars["empty"], true);
        assert_eq!(rt.vars["ok"], false);
        s.input_params
            .insert("stopIfFail".into(), json!({"Value":"0"}));
        with_action_test_runtime(|t| t.input_dialog_results.push_back(Err("cancelled".into())));
        assert!(rt.run_input_dialog(&s).is_ok());
    }

    #[test]
    fn folder_cancel_clears_old_path_and_action_cancel_never_continues() {
        let mut rt = runtime();
        let mut s = step(
            "sys:selectFolder",
            json!({}),
            json!({"path":"path","isSuccess":"ok"}),
        );
        rt.vars.insert("path".into(), json!("old"));
        with_action_test_runtime(|t| t.folder_dialog_results.push_back(Err("cancelled".into())));
        assert!(rt.run_folder_dialog(&s).is_err());
        assert_eq!(rt.vars["path"], "");
        assert_eq!(rt.vars["ok"], false);
        s.input_params
            .insert("stopIfFail".into(), json!({"Value":"0"}));
        with_action_test_runtime(|t| t.folder_dialog_results.push_back(Err("cancelled".into())));
        assert!(rt.run_folder_dialog(&s).is_ok());
        rt.control = Some(ActionExecutionControl::new());
        rt.control.as_ref().unwrap().cancel();
        assert_eq!(rt.run_folder_dialog(&s).unwrap_err(), "Action cancelled");
        let input = step(
            "sys:userInput",
            json!({"stopIfFail":{"Value":"0"}}),
            json!({}),
        );
        assert_eq!(rt.run_input_dialog(&input).unwrap_err(), "Action cancelled");
    }

    #[test]
    fn unsupported_dialog_options_are_reported_without_showing_a_window() {
        let mut rt = runtime();
        for (runner, key, value) in [
            ("sys:MsgBox", "operation", "custom"),
            ("sys:MsgBox", "buttons", "Typo"),
            ("sys:userInput", "type", "number"),
            ("sys:userInput", "topMost", "1"),
        ] {
            let s = step(
                runner,
                json!({key:{"Value":value},"message":{"Value":"test"}}),
                json!({}),
            );
            assert!(rt.run_step(&s).is_err());
            let report = compatibility::inspect(
                &json!({"ActionType":24,"Title":"dialogs","Data":json!({"Steps":[s]}).to_string()})
                    .to_string(),
            );
            assert_eq!(report["runtime"]["status"], "blocked", "{report}");
        }
        with_action_test_runtime(|t| assert!(t.message_boxes.is_empty()));
    }

    #[test]
    fn dialog_protocol_removes_only_its_own_newline() {
        assert_eq!(decode_text_output(b"  data \n\n").unwrap(), "  data \n");
        assert_eq!(decode_text_output(b"\n").unwrap(), "");
        assert_eq!(decode_exact_output(b"  data \n").unwrap(), "  data \n");
        assert!(decode_exact_output(&[0xff]).is_err());
        assert!(decode_exact_output(&vec![b'x'; 1024 * 1024 + 1]).is_err());
    }
}
