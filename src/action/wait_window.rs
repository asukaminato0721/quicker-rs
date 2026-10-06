use super::*;

pub(super) const MODES: &[&str] = &[
    "show",
    "update",
    "check",
    "close",
    "waitClose",
    "showAndWaitClose",
];
pub(super) const LOCATIONS: &[&str] = &[
    "WithMouse1",
    "WithMouse2",
    "CenterScreen",
    "TopLeft",
    "TopCenter",
    "TopRight",
    "LeftCenter",
    "RightCenter",
    "BottomLeft",
    "BottomCenter",
    "BottomRight",
    "LastPosition",
];
pub(super) const CONTENT_OPTIONS: &[&str] =
    &["title", "prompt", "btnText", "progress", "operations"];
pub(super) const WINDOW_OPTIONS: &[&str] = &[
    "winLocation",
    "fontsize",
    "autoCloseSeconds",
    "activateMode",
    "help",
];

pub(super) fn validate_option(key: &str, value: &Value) -> bool {
    let text = value_to_string(value);
    if text.len() > 64 * 1024 {
        return false;
    }
    match key {
        "mode" => MODES.contains(&text.as_str()),
        "progress" => progress(&text).is_ok(),
        "operations" => show_text::operations(&text).is_ok(),
        "winLocation" => LOCATIONS.contains(&text.as_str()),
        "fontsize" => text
            .parse::<f32>()
            .is_ok_and(|v| v.is_finite() && (6.0..=96.0).contains(&v)),
        "autoCloseSeconds" => text
            .parse::<f64>()
            .is_ok_and(|v| v.is_finite() && (0.0..=86400.0).contains(&v)),
        "activateMode" => matches!(
            text.as_str(),
            "NotActivatable" | "NotActivated" | "AutoActivate"
        ),
        "help" => text.is_empty(),
        _ => true,
    }
}

// MSI WaitUserWindow.C60Vyf7DYlO: a negative numerator counts down from the maximum.
pub(super) fn progress(text: &str) -> Result<Option<f32>, String> {
    if text.is_empty() {
        return Ok(None);
    }
    let invalid = || {
        "Wait-window progress requires current/total with finite values and a positive total"
            .to_string()
    };
    let (current, total) = text.split_once('/').ok_or_else(invalid)?;
    let mut current = current.trim().parse::<f64>().map_err(|_| invalid())?;
    let total = total.trim().parse::<f64>().map_err(|_| invalid())?;
    if !current.is_finite() || !total.is_finite() || total <= 0.0 {
        return Err(invalid());
    }
    if current < 0.0 {
        current += total;
    }
    Ok(Some((current / total).clamp(0.0, 1.0) as f32))
}

pub(super) fn delay_millis(value: &Value) -> Result<i32, String> {
    if let Some(number) = value.as_f64() {
        if number.is_finite()
            && number.fract() == 0.0
            && (i32::MIN as f64..=i32::MAX as f64).contains(&number)
        {
            return Ok(number as i32);
        }
    }
    let text = value_to_string(value);
    if text.trim().is_empty() {
        return Ok(0);
    }
    text.trim()
        .parse::<i32>()
        .map_err(|_| "delayMs requires a 32-bit integer".into())
}

impl QuickerRuntime {
    pub(super) fn wait_window_closed(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return self.wait_window.is_closed();
        #[cfg(target_arch = "wasm32")]
        true
    }

    pub(super) fn run_delay(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        let milliseconds = delay_millis(
            &self
                .input_value(&step.input_params, "delayMs")?
                .unwrap_or_else(|| Value::from(100)),
        )?;
        let monitor = self.input_bool(&step.input_params, "monitorWaitWin")?;
        ensure_not_cancelled(self.control.as_ref())?;
        if milliseconds <= 0 {
            return Ok(StepFlow::Continue);
        }
        if !monitor {
            sleep_millis(milliseconds as u64, self.control.as_ref())?;
        } else {
            #[cfg(target_arch = "wasm32")]
            return Err("Wait-window monitoring requires the native application".into());
            #[cfg(not(target_arch = "wasm32"))]
            {
                let duration = Duration::from_millis(milliseconds as u64);
                let start = std::time::Instant::now();
                // The MSI tests closure even for delays shorter than one second.
                while start.elapsed() < duration && !self.wait_window_closed() {
                    ensure_not_cancelled(self.control.as_ref())?;
                    thread::sleep(
                        Duration::from_millis(20).min(duration.saturating_sub(start.elapsed())),
                    );
                }
            }
        }
        ensure_not_cancelled(self.control.as_ref())?;
        Ok(StepFlow::Continue)
    }

    pub(super) fn run_wait_window(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        ensure_not_cancelled(self.control.as_ref())?;
        let mode = self
            .input_string_opt(&step.input_params, "mode")?
            .unwrap_or_else(|| "show".into());
        if !MODES.contains(&mode.as_str()) {
            return Err(format!("Unsupported showWaitWin mode: {mode}"));
        }
        // Appearance fields are inactive for check, close, and waitClose.
        let mut keys = Vec::new();
        if matches!(mode.as_str(), "show" | "showAndWaitClose" | "update") {
            keys.extend_from_slice(CONTENT_OPTIONS);
        }
        if matches!(mode.as_str(), "show" | "showAndWaitClose") {
            keys.extend_from_slice(WINDOW_OPTIONS);
        }
        for key in keys {
            if let Some(value) = self.input_value(&step.input_params, key)? {
                if !validate_option(key, &value) {
                    return Err(format!("Unsupported showWaitWin option: {key}"));
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        return Err("Wait windows require the native application".into());
        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::wait_windows::{Content, Options};
            let input = |key, default: &str| {
                self.input_string_opt(&step.input_params, key)
                    .map(|v| v.unwrap_or_else(|| default.into()))
            };
            let boolean = |key, default| {
                self.input_value(&step.input_params, key)
                    .map(|v| v.map_or(default, |v| truthy(Some(&v))))
            };
            if matches!(mode.as_str(), "show" | "showAndWaitClose" | "update") {
                let progress_text = input("progress", "")?;
                let content = Content {
                    title: input("title", "完成后继续")?,
                    prompt: input("prompt", "请在完成操作后点下面的按钮")?,
                    button: input("btnText", "完成")?,
                    progress: progress(&progress_text)?,
                    progress_text: progress_text.trim_start_matches('-').into(),
                    operations: show_text::operations(&input("operations", "")?)?,
                };
                if mode == "update" {
                    self.wait_window.update(content);
                } else {
                    self.wait_window.show(
                        content,
                        Options {
                            location: input("winLocation", "BottomRight")?,
                            activation: input("activateMode", "NotActivatable")?,
                            font_size: input("fontsize", "12")?
                                .parse()
                                .map_err(|_| "Invalid wait-window font size")?,
                            auto_close: input("autoCloseSeconds", "0")?
                                .parse()
                                .map_err(|_| "Invalid wait-window timeout")?,
                            stop_on_close: boolean("stopActionIfClose", true)?,
                        },
                        self.control.clone().unwrap_or_default(),
                    )?;
                    let _dialog = dialogs::DialogSession::new(self.control.as_ref());
                    self.wait_window.wait(true, self.control.as_ref())?;
                }
            }
            if mode == "close" {
                self.wait_window.close();
            }
            if matches!(mode.as_str(), "waitClose" | "showAndWaitClose") {
                let _dialog = dialogs::DialogSession::new(self.control.as_ref());
                self.wait_window.wait(false, self.control.as_ref())?;
            }
            ensure_not_cancelled(self.control.as_ref())?;
            if matches!(mode.as_str(), "check" | "waitClose" | "showAndWaitClose") {
                let status = self.wait_window.snapshot();
                // The MSI also writes isClosed after both blocking modes.
                self.assign_output(&step.output_params, "isClosed", Value::Bool(status.closed))?;
                self.assign_output(
                    &step.output_params,
                    "selectedOperation",
                    Value::String(status.operation),
                )?;
            }
            Ok(StepFlow::Continue)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn step(mode: &str) -> QuickerPluginStepDocument {
        serde_json::from_value(
            json!({"StepRunnerKey":"sys:showWaitWin", "InputParams":{"mode":{"Value":mode}},
            "OutputParams":{"isClosed":"closed","selectedOperation":"button"}}),
        )
        .unwrap()
    }

    #[test]
    fn progress_preserves_countdown_and_rejects_nonfinite_totals() {
        assert_eq!(progress(""), Ok(None));
        assert_eq!(progress("40/80"), Ok(Some(0.5)));
        assert_eq!(progress("-10/100"), Ok(Some(0.9)));
        assert_eq!(progress(" 3 / 2 "), Ok(Some(1.0)));
        assert_eq!(progress("-20/10"), Ok(Some(0.0)));
        for invalid in ["NaN/100", "0/0", "1/-1", "1/inf", "1/2/3", "30", "1e400/2"] {
            assert!(progress(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn missing_windows_return_closed_and_ignore_inactive_options() {
        let data: QuickerPluginData = serde_json::from_value(json!({"Steps":[]})).unwrap();
        let mut runtime = QuickerRuntime::new(&data, "wait-test".into(), None).unwrap();
        for mode in ["check", "waitClose", "close"] {
            let mut step = step(mode);
            step.input_params.insert(
                "progress".into(),
                serde_json::from_value(json!({"Value":"invalid"})).unwrap(),
            );
            runtime.run_step(&step).unwrap();
        }
        assert_eq!(runtime.vars["closed"], json!(true));
        assert_eq!(runtime.vars["button"], json!(""));
        runtime.run_step(&step("update")).unwrap();
        assert!(runtime.run_step(&step("unknown")).is_err());
        let mut update = step("update");
        update.input_params.insert(
            "progress".into(),
            serde_json::from_value(json!({"Value":"invalid"})).unwrap(),
        );
        assert!(runtime.run_step(&update).unwrap_err().contains("progress"));
    }

    #[test]
    fn static_options_follow_active_modes_and_defer_dynamic_values() {
        for (mode, help, expected) in [
            ("show", "markdown", true),
            ("update", "markdown", false),
            ("check", "markdown", false),
            ("$= {mode}", "markdown", false),
        ] {
            let document = json!({"ActionType":24,"Title":"Wait options","Data":json!({"Steps":[{
                "StepRunnerKey":"sys:showWaitWin","InputParams":{"mode":{"Value":mode},"help":{"Value":help}}
            }]}).to_string()}).to_string();
            let report = compatibility::inspect(&document);
            let blocked = report["runtime"]["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["code"] == "unsupported_wait_window_option");
            assert_eq!(blocked, expected, "{report}");
        }
        assert!(!validate_option("operations", &json!("[fa:x]Icon|x")));
        assert!(validate_option("operations", &json!("Continue|ok\nCancel")));
        assert!(!validate_option("autoCloseSeconds", &json!("NaN")));
    }

    #[test]
    fn delay_monitor_returns_when_no_window_exists_and_rejects_bad_delays() {
        let data: QuickerPluginData = serde_json::from_value(json!({"Steps":[]})).unwrap();
        let mut runtime = QuickerRuntime::new(&data, "delay-test".into(), None).unwrap();
        let start = std::time::Instant::now();
        for delay in ["", " 0 ", "-1", "0", "500", "30000"] {
            let step = serde_json::from_value(json!({"StepRunnerKey":"sys:delay",
                "InputParams":{"delayMs":{"Value":delay},"monitorWaitWin":{"Value":"1"}}}))
            .unwrap();
            runtime.run_step(&step).unwrap();
        }
        assert!(start.elapsed().as_secs_f64() < 0.5);
        assert_eq!(delay_millis(&json!(500.0)), Ok(500));
        assert!(delay_millis(&json!(500.5)).is_err());
        for delay in ["NaN", "12.5", "2147483648"] {
            let step = serde_json::from_value(json!({"StepRunnerKey":"sys:delay",
                "InputParams":{"delayMs":{"Value":delay}}}))
            .unwrap();
            assert!(runtime.run_step(&step).is_err());
        }
    }

    #[test]
    #[ignore = "requires the downloaded OpenCC corpus"]
    fn downloaded_opencc_wait_window_update_preserves_original_step() {
        let root = std::env::var("QUICKER_COMPAT_CORPUS").expect("Set QUICKER_COMPAT_CORPUS");
        let document = parse_quicker_action_document(
            &fs::read_to_string(Path::new(&root).join("opencc.json")).unwrap(),
        )
        .unwrap();
        let data = document.data_payload().unwrap();
        fn visit(steps: &[QuickerPluginStepDocument], found: &mut Vec<QuickerPluginStepDocument>) {
            for step in steps.iter().filter(|s| !s.disabled) {
                if step.step_runner_key == "sys:showWaitWin" {
                    found.push(step.clone());
                }
                if let Some(children) = &step.if_steps {
                    visit(children, found);
                }
                if let Some(children) = &step.else_steps {
                    visit(children, found);
                }
            }
        }
        let mut steps = Vec::new();
        visit(&data.steps, &mut steps);
        assert_eq!(steps.len(), 1);
        let empty: QuickerPluginData = serde_json::from_value(json!({"Steps":[]})).unwrap();
        let mut runtime = QuickerRuntime::new(&empty, "opencc-wait-test".into(), None).unwrap();
        runtime.vars.insert("count".into(), json!(1));
        runtime.vars.insert(
            "文件处理列表".into(),
            json!(["one", "two", "three", "four"]),
        );
        assert_eq!(
            runtime
                .input_string(&steps[0].input_params, "progress")
                .unwrap(),
            "2/4"
        );
        runtime.run_steps(&steps).unwrap();
        assert!(runtime.wait_window_closed());
    }
}
