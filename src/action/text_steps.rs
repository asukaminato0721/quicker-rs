use super::*;

pub(super) fn delay(value: &str) -> Result<u64, String> {
    value
        .parse::<u32>()
        .map(u64::from)
        .map_err(|_| "Text delay must be a nonnegative integer in milliseconds".into())
}

/// Finish each short input batch before observing cancellation. This lets the
/// input backend release generated keys and restore cleared modifiers.
fn type_in_batches(
    text: &str,
    delay: u64,
    control: Option<&ActionExecutionControl>,
    mut send: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    let text = text.replace("\r\n", "\n");
    if text.contains('\0') {
        return Err("Text input contains a NUL character".into());
    }
    let mut chars = text.chars().peekable();
    while chars.peek().is_some() {
        ensure_not_cancelled(control)?;
        let batch: String = chars
            .by_ref()
            .take(if delay == 0 { 32 } else { 1 })
            .collect();
        send(&batch)?;
        ensure_not_cancelled(control)?;
        if delay != 0 && chars.peek().is_some() {
            sleep_millis(delay, control)?;
        }
    }
    Ok(())
}

impl QuickerRuntime {
    pub(super) fn run_output_text(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        let stop = self
            .input_value(&step.input_params, "stopIfFail")?
            .is_none_or(|v| truthy(Some(&v)));
        let result = self.output_text(step);
        ensure_not_cancelled(self.control.as_ref())?;
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

    fn output_text(&self, step: &QuickerPluginStepDocument) -> Result<(), String> {
        ensure_not_cancelled(self.control.as_ref())?;
        let content = self
            .input_string_opt(&step.input_params, "content")?
            .unwrap_or_default();
        let method = self
            .input_string_opt(&step.input_params, "method")?
            .unwrap_or_else(|| "paste".into())
            .to_ascii_lowercase();
        let append_return = self.input_bool(&step.input_params, "appendReturn")?;
        // Empty content does not send Return or change the clipboard in Quicker.
        if content.is_empty() {
            return Ok(());
        }
        match method.as_str() {
            "paste" => {
                if self.input_bool(&step.input_params, "hideInHistory")? {
                    return Err("Clipboard history exclusion is not supported on Linux".into());
                }
                let before = delay(
                    &self
                        .input_string_opt(&step.input_params, "delayBeforePaste")?
                        .unwrap_or_else(|| "50".into()),
                )?;
                let after = delay(
                    &self
                        .input_string_opt(&step.input_params, "delayAfterPaste")?
                        .unwrap_or_else(|| "10".into()),
                )?;
                write_clipboard_text(&content)?;
                sleep_millis(before, self.control.as_ref())?;
                ensure_not_cancelled(self.control.as_ref())?;
                send_key_combo(&["ctrl".into()], "v")?;
                sleep_millis(after, self.control.as_ref())?;
            }
            "input" => {
                let between = delay(
                    &self
                        .input_string_opt(&step.input_params, "delayBetweenChar")?
                        .unwrap_or_else(|| "0".into()),
                )?;
                type_in_batches(&content, between, self.control.as_ref(), type_input_text)?;
            }
            other => return Err(format!("Unsupported outputText method: {other}")),
        }
        ensure_not_cancelled(self.control.as_ref())?;
        if append_return {
            send_key_combo(&[], "Return")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn execute(
        params: Value,
        vars: Option<HashMap<String, Value>>,
    ) -> (Result<StepFlow, String>, QuickerRuntime) {
        let data: QuickerPluginData = serde_json::from_value(json!({})).unwrap();
        let mut runtime = QuickerRuntime::new(&data, "output-test".into(), None).unwrap();
        if let Some(vars) = vars {
            runtime.vars = vars;
        }
        let step = serde_json::from_value(json!({"StepRunnerKey":"sys:outputText", "InputParams": params, "OutputParams":{"isSuccess":"ok"}})).unwrap();
        let result = runtime.run_output_text(&step);
        (result, runtime)
    }

    #[test]
    fn output_input_preserves_unicode_delays_and_literal_text() {
        reset_action_test_runtime();
        with_action_test_runtime(|r| {
            r.typed_input_results
                .extend([Ok(()), Ok(()), Ok(()), Ok(())]);
            r.key_results.push_back(Ok(()));
        });
        let (result, runtime) = execute(
            json!({"method":{"Value":"input"}, "content":{"VarKey":"text"}, "delayBetweenChar":{"Value":"5"}, "appendReturn":{"Value":"1"}}),
            Some(HashMap::from([("text".into(), json!("-中🙂\r\n"))])),
        );
        result.unwrap();
        assert_eq!(runtime.vars["ok"], true);
        with_action_test_runtime(|r| {
            assert_eq!(r.typed_inputs, ["-", "中", "🙂", "\n"]);
            assert_eq!(r.delays, [5, 5, 5]);
            assert_eq!(r.key_calls, [(vec![], "Return".into())]);
            assert!(r.clipboard_writes.is_empty());
        });
    }

    #[test]
    fn output_paste_uses_defaults_and_reports_failure_and_empty_content() {
        reset_action_test_runtime();
        with_action_test_runtime(|r| {
            r.clipboard_write_results.push_back(Ok(()));
            r.key_results.push_back(Ok(()));
        });
        let (result, runtime) = execute(json!({"content":{"Value":"hello"}}), None);
        result.unwrap();
        assert_eq!(runtime.vars["ok"], true);
        with_action_test_runtime(|r| {
            assert_eq!(r.clipboard_writes, ["hello"]);
            assert_eq!(r.delays, [50, 10]);
        });
        with_action_test_runtime(|r| {
            r.typed_input_results.push_back(Err("test failure".into()));
        });
        let (result, runtime) = execute(
            json!({"method":{"Value":"input"}, "content":{"Value":"x"}, "stopIfFail":{"Value":"false"}}),
            None,
        );
        result.unwrap();
        assert_eq!(runtime.vars["ok"], false);
        reset_action_test_runtime();
        let (result, runtime) = execute(
            json!({"content":{"Value":""}, "appendReturn":{"Value":"1"}}),
            None,
        );
        result.unwrap();
        assert_eq!(runtime.vars["ok"], true);
        with_action_test_runtime(|r| {
            assert!(r.key_calls.is_empty());
            assert!(r.clipboard_writes.is_empty());
        });
        for invalid in [
            json!({"hideInHistory":{"Value":"1"}}),
            json!({"delayAfterPaste":{"Value":"-1"}}),
        ] {
            let mut params = invalid;
            params["content"] = json!({"Value":"do not send"});
            let (result, runtime) = execute(params, None);
            assert!(result.is_err());
            assert_eq!(runtime.vars["ok"], false);
        }
        with_action_test_runtime(|r| {
            assert!(r.clipboard_writes.is_empty());
        });
    }

    #[test]
    fn text_batches_stop_at_cancellation_and_do_not_split_unicode() {
        reset_action_test_runtime();
        let control = ActionExecutionControl::default();
        let mut sent = Vec::new();
        let text = "🙂".repeat(100);
        let result = type_in_batches(&text, 0, Some(&control), |batch| {
            sent.push(batch.to_owned());
            control.cancel();
            Ok(())
        });
        assert_eq!(result.unwrap_err(), "Action cancelled");
        assert_eq!(sent, ["🙂".repeat(32)]);
        assert!(type_in_batches("before\0after", 0, None, |_| panic!("must not send")).is_err());
    }
}
