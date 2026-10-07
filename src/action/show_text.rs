use super::*;

pub(super) const MODES: &[&str] = &[
    "WAIT",
    "NO_WAIT",
    "CLOSE_WINDOW",
    "GET_WIN_INFO",
    "APPEND_TEXT",
    "ACTIVATE_WINDOW",
    "WAIT_CLOSE",
];

pub(super) const CHECKED_OPTIONS: &[&str] = &[
    "type",
    "operations",
    "winLocation",
    "winSize",
    "fontsize",
    "fontfamily",
    "highlight",
    "bgColor",
    "textColor",
    "autoSaveToState",
    "copyWholeLine",
    "rememberWindowPlacement",
    "advancedSettings",
    "caretPosition",
];

pub(super) fn validate_option(key: &str, value: &Value) -> bool {
    let text = value_to_string(value);
    match key {
        "type" => MODES.contains(&text.as_str()),
        "winLocation" => text == "CenterScreen",
        "winSize" => size(&text).is_ok(),
        "fontsize" => text
            .parse::<f32>()
            .is_ok_and(|n| n.is_finite() && (6.0..=96.0).contains(&n)),
        "caretPosition" => text.parse::<i64>().is_ok_and(|n| n >= -1),
        "bgColor" | "textColor" => color(&text).is_ok(),
        "copyWholeLine" | "rememberWindowPlacement" => !truthy(Some(value)),
        "operations" => operations(&text).is_ok(),
        _ => text.is_empty(),
    }
}

fn size(text: &str) -> Result<[f32; 2], String> {
    if text.is_empty() {
        return Ok([720.0, 480.0]);
    }
    let parts = text
        .split(',')
        .map(|v| v.trim().parse::<f32>())
        .collect::<Result<Vec<_>, _>>();
    match parts {
        Ok(v)
            if v.len() == 2
                && v.iter()
                    .all(|n| n.is_finite() && (160.0..=8192.0).contains(n)) =>
        {
            Ok([v[0], v[1]])
        }
        _ => Err("Text window size requires two pixel values from 160 to 8192".into()),
    }
}

fn color(text: &str) -> Result<Option<egui::Color32>, String> {
    if text.is_empty() {
        return Ok(None);
    }
    if text.len() == 7 && text.starts_with('#') {
        if let Ok(rgb) = u32::from_str_radix(&text[1..], 16) {
            return Ok(Some(egui::Color32::from_rgb(
                (rgb >> 16) as u8,
                (rgb >> 8) as u8,
                rgb as u8,
            )));
        }
    }
    Err("Text window colors require #RRGGBB".into())
}

pub(super) fn has_return_buttons(text: &str) -> bool {
    operations(text).is_ok_and(|buttons| !buttons.is_empty())
}

pub(super) fn operations(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut result = Vec::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with("////"))
    {
        let (label, value) = line.split_once('|').unwrap_or((line, line));
        // Menus, icons, access keys, tooltips, and call: handlers need their own parser.
        if label.is_empty()
            || label.contains(['[', '(', '_'])
            || line.starts_with("|=")
            || value.starts_with("call:")
            || value.contains('|')
            || result.len() >= 32
        {
            return Err(
                "Only plain text-window buttons with literal return values are supported".into(),
            );
        }
        result.push((label.into(), value.into()));
    }
    Ok(result)
}

impl QuickerRuntime {
    pub(super) fn run_show_text(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        ensure_not_cancelled(self.control.as_ref())?;
        let stop = self
            .input_value(&step.input_params, "stopIfFail")?
            .is_none_or(|v| truthy(Some(&v)));
        let result = self.show_text_result(step);
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
        if stop {
            result?;
        }
        Ok(StepFlow::Continue)
    }

    fn show_text_result(&mut self, step: &QuickerPluginStepDocument) -> Result<(), String> {
        for key in CHECKED_OPTIONS {
            if let Some(value) = self.input_value(&step.input_params, key)? {
                if !validate_option(key, &value) {
                    return Err(format!("Unsupported showText option: {key}"));
                }
            }
        }
        if step
            .output_params
            .get("windowHandle")
            .is_some_and(|v| !v.is_null() && v != "")
        {
            return Err("Native window handles are not available from showText".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            Err("Text windows require the native application".into())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::text_windows::{self, snapshot, Options};
            let input = |key, default: &str| {
                self.input_string_opt(&step.input_params, key)
                    .map(|v| v.unwrap_or_else(|| default.into()))
            };
            let boolean = |key, default| {
                self.input_value(&step.input_params, key)
                    .map(|v| v.map_or(default, |v| truthy(Some(&v))))
            };
            let mode = input("type", "NO_WAIT")?;
            let key = input("autoCloseKey", "=")?;
            let key = if key == "=" {
                self.state_scope.clone()
            } else {
                key
            };
            let host = text_windows::host()?;
            let _session = dialogs::DialogSession::new(self.control.as_ref());
            let handle = if matches!(mode.as_str(), "WAIT" | "NO_WAIT") {
                let operations = operations(&input("operations", "")?)?;
                if mode != "WAIT" && !operations.is_empty() {
                    return Err("Text-window return buttons require WAIT mode".into());
                }
                Some(
                    host.open(
                        Options {
                            title: input("title", "文本窗口")?,
                            text: input("text", "")?,
                            key,
                            size: size(&input("winSize", "")?)?,
                            font_size: input("fontsize", "14")?
                                .parse()
                                .map_err(|_| "Invalid text-window font size")?,
                            top_most: boolean("topMost", false)?,
                            wrap: boolean("autoWrap", true)?,
                            line_numbers: boolean("showLineNum", true)?,
                            toolbar: boolean("showBuildInToolbar", true)?,
                            escape_close: boolean("enableEscClose", true)?,
                            close_on_blur: boolean("closeWhenLostFocus", false)?,
                            caret: input("caretPosition", "0")?
                                .parse()
                                .map_err(|_| "Invalid text-window caret position")?,
                            background: color(&input("bgColor", "")?)?,
                            foreground: color(&input("textColor", "")?)?,
                            operations,
                        },
                        mode == "NO_WAIT" && boolean("updateIfExists", false)?,
                    )?,
                )
            } else {
                host.find(&key)?
            };
            match mode.as_str() {
                "WAIT" | "NO_WAIT" | "WAIT_CLOSE" => {
                    if let Some(handle) = &handle {
                        let deadline =
                            std::time::Instant::now() + std::time::Duration::from_secs(10);
                        loop {
                            let (closed, shown) = text_windows::status(handle);
                            if closed || (mode == "NO_WAIT" && shown) {
                                break;
                            }
                            if !shown && std::time::Instant::now() > deadline {
                                host.close(handle);
                                return Err("Text window did not open within 10 seconds".into());
                            }
                            if let Err(error) = sleep_millis(20, self.control.as_ref()) {
                                if mode != "WAIT_CLOSE" {
                                    host.close(handle);
                                }
                                return Err(error);
                            }
                        }
                    }
                }
                "CLOSE_WINDOW" => {
                    if let Some(handle) = &handle {
                        host.close(handle);
                    }
                }
                "APPEND_TEXT" => host.append(
                    handle.as_ref().ok_or("Text window was not found")?,
                    &input("text", "")?,
                )?,
                "ACTIVATE_WINDOW" => {
                    host.activate(handle.as_ref().ok_or("Text window was not found")?)
                }
                "GET_WIN_INFO" => {}
                _ => unreachable!(),
            }
            if mode == "GET_WIN_INFO" {
                self.assign_output(
                    &step.output_params,
                    "isWindowExists",
                    Value::Bool(handle.is_some()),
                )?;
            }
            if matches!(mode.as_str(), "WAIT" | "CLOSE_WINDOW" | "GET_WIN_INFO") {
                // CLOSE_WINDOW leaves outputs unchanged when no matching window exists.
                if handle.is_some() || mode == "GET_WIN_INFO" {
                    let state = handle.as_ref().map(snapshot).unwrap_or_default();
                    self.assign_output(
                        &step.output_params,
                        "resultText",
                        Value::String(state.text),
                    )?;
                    self.assign_output(
                        &step.output_params,
                        "selectedText",
                        Value::String(state.selected),
                    )?;
                    if handle.is_some() {
                        self.assign_output(
                            &step.output_params,
                            "caretPosition",
                            Value::from(state.caret),
                        )?;
                        self.assign_output(
                            &step.output_params,
                            "windowPosition",
                            Value::String(state.position),
                        )?;
                    }
                    if mode == "WAIT" {
                        self.assign_output(
                            &step.output_params,
                            "selectedOperation",
                            Value::String(state.operation),
                        )?;
                    }
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn show_text_validation_rejects_unimplemented_behavior_and_native_handles() {
        for (key, value) in [
            ("type", "GET_ALL_WINDOWS"),
            ("highlight", "C#"),
            ("fontfamily", "Arial"),
            ("advancedSettings", "loaded_sp:run"),
            ("winLocation", "WithMouse1"),
            ("winSize", "NaN,500"),
            ("fontsize", "inf"),
            ("caretPosition", "-2"),
            ("copyWholeLine", "1"),
            ("bgColor", "#xyzxyz"),
            ("operations", "Run|call:all$replaceall$sp$foo"),
        ] {
            let step: QuickerPluginStepDocument = serde_json::from_value(json!({
                "StepRunnerKey":"sys:showText", "InputParams": {key:{"Value":value}}
            }))
            .unwrap();
            let mut runtime = QuickerRuntime::new(
                &serde_json::from_value(json!({})).unwrap(),
                "text".into(),
                None,
            )
            .unwrap();
            assert!(
                runtime
                    .run_step(&step)
                    .unwrap_err()
                    .contains("Unsupported showText option"),
                "{key}"
            );
            let report = compatibility::inspect(
                &json!({"ActionType":24,"Title":"Text","Data":json!({"Steps":[step]}).to_string()})
                    .to_string(),
            );
            assert_eq!(report["runtime"]["status"], "blocked", "{report}");
        }
        for step in [
            json!({"StepRunnerKey":"sys:showText","OutputParams":{"windowHandle":"handle"}}),
            json!({"StepRunnerKey":"sys:showText","InputParams":{"operations":{"Value":"Accept|yes"}}}),
        ] {
            let report = compatibility::inspect(
                &json!({"ActionType":24,"Title":"Text","Data":json!({"Steps":[step]}).to_string()})
                    .to_string(),
            );
            assert_eq!(report["runtime"]["status"], "blocked", "{report}");
        }
    }

    #[test]
    fn simple_buttons_and_native_dimensions_are_validated() {
        assert_eq!(
            operations("First|one\r\n////comment\nSecond\n").unwrap(),
            vec![
                ("First".into(), "one".into()),
                ("Second".into(), "Second".into())
            ]
        );
        assert_eq!(size("800, 600").unwrap(), [800.0, 600.0]);
        assert!(size("50%,50%").is_err());
        assert!(color("#12abFF").unwrap().is_some());
        assert_eq!(color("").unwrap(), None);
        for inputs in [
            json!({"operations":{"Value":"//// disabled button"}}),
            json!({"type":{"VarKey":"mode"},"operations":{"Value":"Accept|yes"}}),
            json!({"operations":{"VarKey":"buttons"}}),
        ] {
            let report = compatibility::inspect(
                &json!({"ActionType":24,"Title":"Text","Data":json!({"Steps":[{
                "StepRunnerKey":"sys:showText","InputParams":inputs
            }]}).to_string()})
                .to_string(),
            );
            assert_ne!(report["runtime"]["status"], "blocked", "{report}");
        }
    }

    #[test]
    #[ignore = "requires QUICKER_COMPAT_CORPUS with pinned author downloads"]
    fn downloaded_opencc_text_window_steps_validate_without_modification() {
        let dir = std::env::var("QUICKER_COMPAT_CORPUS").expect("corpus");
        let source =
            std::fs::read_to_string(std::path::Path::new(&dir).join("opencc.json")).unwrap();
        let document: Value = serde_json::from_str(&source).unwrap();
        let data: Value = serde_json::from_str(document["Data"].as_str().unwrap()).unwrap();
        fn collect<'a>(value: &'a Value, found: &mut Vec<&'a Value>) {
            match value {
                Value::Object(map) if !map.get("Disabled").is_some_and(|v| v == true) => {
                    if map
                        .get("StepRunnerKey")
                        .is_some_and(|v| v == "sys:showText")
                    {
                        found.push(value);
                    }
                    for value in map.values() {
                        collect(value, found);
                    }
                }
                Value::Array(values) => {
                    for value in values {
                        collect(value, found);
                    }
                }
                _ => {}
            }
        }
        let mut steps = vec![];
        collect(&data, &mut steps);
        assert_eq!(steps.len(), 3);
        for step in steps {
            let report = compatibility::inspect(&json!({"ActionType":24,"Title":"OpenCC text","Data":json!({"Steps":[step]}).to_string()}).to_string());
            assert_ne!(report["runtime"]["status"], "blocked", "{report}");
        }
    }
}
