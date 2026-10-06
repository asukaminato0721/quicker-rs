use super::*;
use std::time::Duration;
use web_time::Instant;

fn focused_selection_text() -> Result<Option<String>, String> {
    #[cfg(test)]
    return Ok(None); // Unit tests must not read the user's desktop selection.
    #[cfg(all(not(test), target_os = "linux"))]
    return crate::x11::focused_selection_text();
    #[cfg(all(not(test), not(target_os = "linux")))]
    Ok(None)
}

pub(super) fn clipboard_snapshot() -> Result<(u64, Option<u32>), String> {
    #[cfg(test)]
    if let Some(result) = with_action_test_runtime(|r| r.clipboard_snapshots.pop_front()) {
        return result;
    }
    #[cfg(target_os = "linux")]
    return crate::clipboard_monitor::snapshot().map(|s| (s.sequence, s.age_ms));
    #[cfg(not(target_os = "linux"))]
    Err("Clipboard event monitoring requires the Linux X11 backend".into())
}

fn read_copied_text(format: &str) -> Result<String, String> {
    #[cfg(test)]
    if let Some(result) = with_action_test_runtime(|r| r.raw_clipboard_reads.pop_front()) {
        return result;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        match format {
            "UnicodeText" => clipboard.get_text(),
            "Html" => clipboard.get().html(),
            _ => return Err(format!("Unsupported selected text format: {format}")),
        }
        .map_err(|e| e.to_string())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = format;
        Err("Selected text is unavailable in the web preview".into())
    }
}

impl QuickerRuntime {
    fn wait_for_clipboard(&self, baseline: u64, timeout: Duration) -> Result<(), String> {
        let start = Instant::now();
        loop {
            ensure_not_cancelled(self.control.as_ref())?;
            if clipboard_snapshot()?.0 != baseline {
                return Ok(());
            }
            if start.elapsed() >= timeout {
                return Err("Clipboard did not change before the timeout".into());
            }
            sleep_millis(5, self.control.as_ref())?;
        }
    }

    fn clipboard_result(
        &mut self,
        step: &QuickerPluginStepDocument,
        result: Result<(), String>,
    ) -> Result<StepFlow, String> {
        ensure_not_cancelled(self.control.as_ref())?;
        self.assign_output(
            &step.output_params,
            "isSuccess",
            Value::Bool(result.is_ok()),
        )?;
        self.assign_output(
            &step.output_params,
            "errMessage",
            Value::String(result.as_ref().err().cloned().unwrap_or_default()),
        )?;
        let stop = self
            .input_value(&step.input_params, "stopIfFail")?
            .is_none_or(|v| truthy(Some(&v)));
        match result {
            Err(error) if stop => Err(error),
            _ => Ok(StepFlow::Continue),
        }
    }

    pub(super) fn run_wait_clipboard(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        let result = (|| {
            if self.input_bool(&step.input_params, "monitorWaitWin")? {
                return Err("Wait-window monitoring is not supported".into());
            }
            let seconds = self
                .input_string_opt(&step.input_params, "maxWaitSeconds")?
                .unwrap_or_else(|| "10".into())
                .parse::<f64>()
                .map_err(|_| "Invalid maxWaitSeconds")?;
            let timeout =
                Duration::try_from_secs_f64(seconds).map_err(|_| "Invalid maxWaitSeconds")?;
            let recent = self
                .input_string_opt(&step.input_params, "recentChangeMs")?
                .unwrap_or_else(|| "10".into())
                .parse::<u32>()
                .map_err(|_| "Invalid recentChangeMs")?;
            let current = clipboard_snapshot()?;
            let baseline = self.clipboard_before_copy.take().unwrap_or(current.0);
            if current.0 != baseline || current.1.is_some_and(|age| recent > 0 && age <= recent) {
                return Ok(());
            }
            self.wait_for_clipboard(baseline, timeout)
        })();
        self.clipboard_result(step, result)
    }

    pub(super) fn run_selected_text(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        let result = (|| {
            let format = self
                .input_string_opt(&step.input_params, "format")?
                .unwrap_or_else(|| "UnicodeText".into());
            if !matches!(format.as_str(), "UnicodeText" | "Html") {
                return Err(format!("Unsupported selected text format: {format}"));
            }
            for key in ["tryNoClipboard", "useActionParam"] {
                if self.input_bool(&step.input_params, key)? {
                    return Err(format!("Selected text option {key} is not supported"));
                }
            }
            let wait = self
                .input_string_opt(&step.input_params, "waitMs")?
                .unwrap_or_else(|| "250".into())
                .parse::<u64>()
                .map_err(|_| "Invalid waitMs")?;
            let retries = self
                .input_string_opt(&step.input_params, "repeat")?
                .unwrap_or_else(|| "0".into())
                .parse::<u32>()
                .map_err(|_| "Invalid repeat")?;
            let mut text = None;
            let mut last_error = "Selected text is unavailable".to_string();
            for _ in 0..=retries {
                ensure_not_cancelled(self.control.as_ref())?;
                let before = clipboard_snapshot()?.0;
                send_key_combo(&["ctrl".into()], "c")?;
                let mut result = self
                    .wait_for_clipboard(before, Duration::from_millis(wait))
                    .and_then(|()| read_copied_text(&format));
                ensure_not_cancelled(self.control.as_ref())?;
                // Some X11 clients do not announce a second copy of the same selection.
                // Read their active PRIMARY selection, never old CLIPBOARD contents.
                if result.is_err() && format == "UnicodeText" {
                    if let Ok(Some(selection)) = focused_selection_text() {
                        write_clipboard_text(&selection)?;
                        result = Ok(selection);
                    }
                }
                match result {
                    Ok(value) if !value.is_empty() => {
                        text = Some(value);
                        break;
                    }
                    Ok(_) => last_error = "The copied selection is empty".into(),
                    Err(error) => last_error = error,
                }
            }
            let mut text = text.ok_or(last_error)?;
            if self.input_bool(&step.input_params, "trim")? {
                text = text.trim().to_string();
            }
            self.assign_output(
                &step.output_params,
                "outputEncoded",
                Value::String(urlencoding::encode(&text).into_owned()),
            )?;
            self.assign_output(&step.output_params, "url", Value::String(String::new()))?;
            self.assign_output(&step.output_params, "output", Value::String(text))?;
            Ok(())
        })();
        self.clipboard_result(step, result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(step: Value) -> (Result<StepFlow, String>, QuickerRuntime) {
        let data: QuickerPluginData = serde_json::from_value(json!({"Steps": [step]})).unwrap();
        let mut runtime = QuickerRuntime::new(&data, "clipboard-test".into(), None).unwrap();
        (runtime.run_steps(&data.steps), runtime)
    }

    #[test]
    fn wait_accepts_recent_changes_and_reports_timeout_without_stale_success() {
        reset_action_test_runtime();
        with_action_test_runtime(|r| r.clipboard_snapshots.push_back(Ok((5, Some(8)))));
        let (result, runtime) = run(
            json!({"StepRunnerKey": "sys:waitClipboardChange", "OutputParams": {"isSuccess": "ok", "errMessage": "error"}}),
        );
        assert_eq!(result.unwrap(), StepFlow::Continue);
        assert_eq!(runtime.vars["ok"], true);
        assert_eq!(runtime.vars["error"], "");
        with_action_test_runtime(|r| {
            r.clipboard_snapshots
                .extend([Ok((5, Some(30))), Ok((5, Some(30)))])
        });
        let (result, runtime) = run(json!({"StepRunnerKey": "sys:waitClipboardChange",
            "InputParams": {"maxWaitSeconds": {"Value": "0"}, "stopIfFail": {"Value": "false"}},
            "OutputParams": {"isSuccess": "ok", "errMessage": "error"}}));
        assert_eq!(result.unwrap(), StepFlow::Continue);
        assert_eq!(runtime.vars["ok"], false);
        assert!(runtime.vars["error"].as_str().unwrap().contains("timeout"));
    }

    #[test]
    fn selected_text_waits_for_copy_and_preserves_whitespace_unless_trimmed() {
        for (trim, expected) in [(false, "  hello 世界\n"), (true, "hello 世界")] {
            reset_action_test_runtime();
            with_action_test_runtime(|r| {
                r.clipboard_snapshots
                    .extend([Ok((1, None)), Ok((2, Some(0)))]);
                r.key_results.push_back(Ok(()));
                r.raw_clipboard_reads.push_back(Ok("  hello 世界\n".into()));
            });
            let (result, runtime) = run(json!({"StepRunnerKey": "sys:getSelectedText",
                "InputParams": {"trim": {"Value": trim}},
                "OutputParams": {"output": "text", "outputEncoded": "encoded", "isSuccess": "ok"}}));
            assert_eq!(result.unwrap(), StepFlow::Continue);
            assert_eq!(runtime.vars["text"], expected);
            assert_eq!(
                runtime.vars["encoded"],
                urlencoding::encode(expected).as_ref()
            );
            assert_eq!(runtime.vars["ok"], true);
            with_action_test_runtime(|r| {
                assert_eq!(r.key_calls, vec![(vec!["ctrl".into()], "c".into())])
            });
        }
    }

    #[test]
    fn selected_text_retries_without_using_an_unchanged_clipboard() {
        reset_action_test_runtime();
        with_action_test_runtime(|r| {
            r.clipboard_snapshots.extend([
                Ok((1, None)),
                Ok((1, None)),
                Ok((1, None)),
                Ok((2, Some(0))),
            ]);
            r.key_results.extend([Ok(()), Ok(())]);
            r.raw_clipboard_reads.push_back(Ok("fresh".into()));
        });
        let (result, runtime) = run(json!({"StepRunnerKey": "sys:getSelectedText",
            "InputParams": {"waitMs": {"Value": "0"}, "repeat": {"Value": "1"}},
            "OutputParams": {"output": "text"}}));
        assert!(result.is_ok());
        assert_eq!(runtime.vars["text"], "fresh");
        with_action_test_runtime(|r| assert_eq!(r.key_calls.len(), 2));
    }

    #[test]
    fn wait_retains_the_sequence_from_before_ctrl_c() {
        reset_action_test_runtime();
        with_action_test_runtime(|r| {
            r.clipboard_snapshots
                .extend([Ok((1, None)), Ok((2, Some(100)))]);
            r.key_results.push_back(Ok(()));
        });
        let data: QuickerPluginData = serde_json::from_value(json!({"Steps": [
            {"StepRunnerKey": "sys:keyInput", "InputParams": {"keys": {"Value": "{\"CtrlKeys\":[17],\"Keys\":[67]}"}}},
            {"StepRunnerKey": "sys:waitClipboardChange", "InputParams": {"maxWaitSeconds": {"Value": "0"}, "recentChangeMs": {"Value": "0"}}, "OutputParams": {"isSuccess": "ok"}}
        ]})).unwrap();
        let mut runtime = QuickerRuntime::new(&data, "clipboard-test".into(), None).unwrap();
        assert!(runtime.run_steps(&data.steps).is_ok());
        assert_eq!(runtime.vars["ok"], true);
        assert!(runtime.clipboard_before_copy.is_none());
    }

    #[test]
    fn cancellation_is_not_suppressed_by_stop_if_fail() {
        reset_action_test_runtime();
        let data: QuickerPluginData = serde_json::from_value(json!({})).unwrap();
        let control = ActionExecutionControl::new();
        let mut runtime =
            QuickerRuntime::new(&data, "clipboard-test".into(), Some(control.clone())).unwrap();
        control.cancel();
        let step = serde_json::from_value(json!({"StepRunnerKey": "sys:getSelectedText", "InputParams": {"stopIfFail": {"Value": "false"}}})).unwrap();
        assert_eq!(
            runtime.run_selected_text(&step).unwrap_err(),
            cancellation_error()
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod x11_tests {
    use super::*;
    use serde_json::json;

    #[test]
    #[ignore = "requires an isolated X11 display with XFixes"]
    fn clipboard_wait_cancels_while_waiting() {
        let data: QuickerPluginData = serde_json::from_value(json!({})).unwrap();
        let control = ActionExecutionControl::new();
        let mut runtime =
            QuickerRuntime::new(&data, "clipboard-cancel-test".into(), Some(control.clone()))
                .unwrap();
        let step = serde_json::from_value(json!({"StepRunnerKey": "sys:waitClipboardChange",
            "InputParams": {"maxWaitSeconds": {"Value": "60"}, "recentChangeMs": {"Value": "0"}, "stopIfFail": {"Value": "false"}}})).unwrap();
        clipboard_snapshot().unwrap();
        let cancel = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            control.cancel();
        });
        let start = Instant::now();
        assert_eq!(
            runtime.run_wait_clipboard(&step).unwrap_err(),
            cancellation_error()
        );
        assert!(start.elapsed() < Duration::from_secs(2));
        cancel.join().unwrap();
    }
}
