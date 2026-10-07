use super::*;

const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_MATCHES: usize = 100_000;

pub(super) fn mode(value: &str) -> Result<u8, String> {
    match value {
        "0" => Ok(0),
        "1" => Ok(1),
        "2" => Ok(2),
        s if s.eq_ignore_ascii_case("false") => Ok(0),
        s if s.eq_ignore_ascii_case("true") => Ok(1),
        _ => Err(format!("Unsupported regex extraction mode: {value}")),
    }
}

pub(super) fn compile(
    pattern: &str,
    ignore: bool,
    single: bool,
    multi: bool,
) -> Result<Regex, String> {
    if pattern.len() > 64 * 1024 {
        return Err("Regex pattern exceeds 64 KiB".into());
    }
    compile_step_regex(pattern, ignore, single, multi)
}

#[derive(Debug)]
struct Extracted {
    matches: Vec<Value>,
    outputs: [Value; 5],
}

impl Extracted {
    fn empty(mode: u8) -> Self {
        Self {
            matches: Vec::new(),
            outputs: std::array::from_fn(|_| {
                if mode == 2 {
                    Value::Array(Vec::new())
                } else {
                    Value::String(String::new())
                }
            }),
        }
    }
}

fn extract(
    regex: &Regex,
    input: &str,
    mode: u8,
    control: Option<&ActionExecutionControl>,
) -> Result<Extracted, String> {
    if input.len() > MAX_TEXT_BYTES {
        return Err("Regex input exceeds 16 MiB".into());
    }
    // .NET numbers unnamed groups first, then named groups in source order.
    let names: Vec<_> = regex.capture_names().enumerate().skip(1).collect();
    let groups: Vec<usize> = names
        .iter()
        .filter(|(_, n)| n.is_none())
        .chain(names.iter().filter(|(_, n)| n.is_some()))
        .map(|(i, _)| *i)
        .collect();
    let mut result = Extracted::empty(mode);
    let mut bytes = 0usize;
    let mut count = 0usize;
    let mut text = |value: &str| -> Result<Value, String> {
        bytes = bytes
            .checked_add(value.len())
            .ok_or("Regex output size overflow")?;
        if bytes > MAX_TEXT_BYTES {
            return Err("Regex output exceeds 16 MiB".into());
        }
        Ok(Value::String(value.into()))
    };
    for captures in regex.captures_iter(input) {
        ensure_not_cancelled(control)?;
        let captures = captures.map_err(|e| format!("Regex failed: {e}"))?;
        count += 1;
        if count > MAX_MATCHES {
            return Err("Regex exceeds 100000 matches".into());
        }
        if mode == 1 {
            for &group in &groups {
                result
                    .matches
                    .push(text(captures.get(group).map_or("", |c| c.as_str()))?);
            }
            for (i, value) in result.matches.iter().take(5).enumerate() {
                result.outputs[i] = value.clone();
            }
            break;
        }
        let whole = text(captures.get(0).unwrap().as_str())?;
        if mode == 0 && count <= 5 {
            result.outputs[count - 1] = whole.clone();
        }
        result.matches.push(whole);
        if mode == 2 {
            for (i, output) in result.outputs.iter_mut().enumerate() {
                let value = groups
                    .get(i)
                    .and_then(|group| captures.get(*group))
                    .map_or("", |c| c.as_str());
                output.as_array_mut().unwrap().push(text(value)?);
            }
        }
    }
    ensure_not_cancelled(control)?;
    if count == 0 {
        return Err("Regex did not match the input".into());
    }
    Ok(result)
}

impl QuickerRuntime {
    pub(super) fn run_regex_extract(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        let stop = self
            .input_value(&step.input_params, "stopIfFail")?
            .is_none_or(|v| truthy(Some(&v)));
        let mut selected_mode = 0;
        let result = (|| {
            ensure_not_cancelled(self.control.as_ref())?;
            selected_mode = mode(
                &self
                    .input_string_opt(&step.input_params, "getGroup")?
                    .unwrap_or_else(|| "0".into()),
            )?;
            if self.input_bool(&step.input_params, "rightToLeft")? {
                return Err("Right-to-left regex matching is not supported".into());
            }
            if ["matchObj", "matchesCollection"]
                .iter()
                .any(|key| output_var_name(&step.output_params, key).is_some())
            {
                return Err("Native .NET regex object outputs are not supported".into());
            }
            let input = self
                .input_string_opt(&step.input_params, "data")?
                .unwrap_or_default();
            let pattern = self.input_string(&step.input_params, "pattern")?;
            let regex = compile(
                &pattern,
                self.input_bool(&step.input_params, "ignoreCase")?,
                self.input_bool(&step.input_params, "singleLine")?,
                self.input_bool(&step.input_params, "multiLine")?,
            )?;
            extract(&regex, &input, selected_mode, self.control.as_ref())
        })();
        ensure_not_cancelled(self.control.as_ref())?;
        self.assign_output(
            &step.output_params,
            "isSuccess",
            Value::Bool(result.is_ok()),
        )?;
        let error = result.as_ref().err().cloned();
        self.assign_output(
            &step.output_params,
            "errMessage",
            Value::String(error.clone().unwrap_or_default()),
        )?;
        let result = result.unwrap_or_else(|_| Extracted::empty(selected_mode));
        // Preserve the prototype's output alias for existing local workflows.
        self.assign_output(&step.output_params, "output", result.outputs[0].clone())?;
        for (i, value) in result.outputs.into_iter().enumerate() {
            self.assign_output(&step.output_params, &format!("match{}", i + 1), value)?;
        }
        self.assign_output(&step.output_params, "matches", Value::Array(result.matches))?;
        if stop {
            if let Some(error) = error {
                return Err(error);
            }
        }
        Ok(StepFlow::Continue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn runtime() -> QuickerRuntime {
        let data = serde_json::from_value(json!({"Variables":[{"Key":"all", "Type":4}]})).unwrap();
        QuickerRuntime::new(&data, "regex-test".into(), None).unwrap()
    }
    fn step(mode: &str, input: &str, pattern: &str) -> QuickerPluginStepDocument {
        serde_json::from_value(json!({"StepRunnerKey":"sys:regexExtract", "InputParams":{
            "getGroup":{"Value":mode}, "data":{"Value":input}, "pattern":{"Value":pattern}},
            "OutputParams":{"matches":"all", "match1 ":"first", "match2 ":"second", "match5 ":"fifth", "isSuccess":"ok", "errMessage":"error"}})).unwrap()
    }

    #[test]
    fn regex_modes_return_matches_groups_and_columns_with_missing_groups() {
        for (mode, all, first, second, fifth) in [
            (
                "0",
                json!(["a1", "b2", "c"]),
                json!("a1"),
                json!("b2"),
                json!(""),
            ),
            ("1", json!(["a", "1"]), json!("a"), json!("1"), json!("")),
            (
                "2",
                json!(["a1", "b2", "c"]),
                json!(["a", "b", "c"]),
                json!(["1", "2", ""]),
                json!(["", "", ""]),
            ),
        ] {
            let mut runtime = runtime();
            runtime
                .run_regex_extract(&step(mode, "a1 b2 c", "([a-z])([0-9])?"))
                .unwrap();
            assert_eq!(runtime.vars["all"], all);
            assert_eq!(runtime.vars["first"], first);
            assert_eq!(runtime.vars["second"], second);
            assert_eq!(runtime.vars["fifth"], fifth);
            assert_eq!(runtime.vars["ok"], true);
        }
    }

    #[test]
    fn regex_named_groups_use_dotnet_output_order_and_empty_matches_advance() {
        let mut runtime = runtime();
        runtime
            .run_regex_extract(&step("true", "ab", "(?<named>a)(b)"))
            .unwrap();
        assert_eq!(runtime.vars["all"], json!(["b", "a"]));
        runtime
            .run_regex_extract(&step("false", "中🙂", ""))
            .unwrap();
        assert_eq!(runtime.vars["all"], json!(["", "", ""]));
        runtime.run_regex_extract(&step("1", "abc", "abc")).unwrap();
        assert_eq!(runtime.vars["all"], json!([]));
        assert_eq!(runtime.vars["ok"], true);
    }

    #[test]
    fn regex_failures_clear_outputs_and_cancellation_overrides_failure_policy() {
        let mut runtime = runtime();
        runtime.run_regex_extract(&step("0", "a1", ".+")).unwrap();
        let mut missing = step("2", "none", r"(\d+)");
        missing
            .input_params
            .insert("stopIfFail".into(), json!({"Value":"0"}));
        runtime.run_regex_extract(&missing).unwrap();
        assert_eq!(runtime.vars["all"], json!([]));
        assert_eq!(runtime.vars["first"], json!([]));
        assert_eq!(runtime.vars["ok"], false);
        assert!(!runtime.vars["error"].as_str().unwrap().is_empty());
        for (key, value) in [("getGroup", "3"), ("pattern", "("), ("rightToLeft", "true")] {
            let mut invalid = step("0", "a1", ".+");
            invalid
                .input_params
                .insert(key.into(), json!({"Value":value}));
            assert!(runtime.run_regex_extract(&invalid).is_err());
        }
        let mut objects = step("1", "a", "(.)");
        objects
            .output_params
            .insert("matchObj".into(), json!("object"));
        assert!(runtime
            .run_regex_extract(&objects)
            .unwrap_err()
            .contains(".NET"));
        let control = ActionExecutionControl::new();
        control.cancel();
        runtime.control = Some(control);
        assert!(runtime
            .run_regex_extract(&missing)
            .unwrap_err()
            .contains("cancelled"));
    }

    #[test]
    fn regex_flags_and_limits_are_enforced() {
        let regex = compile("^a.(b)$", true, true, true).unwrap();
        let result = extract(&regex, "x\nA\nb\ny", 1, None).unwrap();
        assert_eq!(result.matches, vec![json!("b")]);
        assert!(compile(&"a".repeat(64 * 1024 + 1), false, false, false).is_err());
        let regex = compile("", false, false, false).unwrap();
        assert!(extract(&regex, &"a".repeat(MAX_MATCHES), 0, None)
            .unwrap_err()
            .contains("100000"));
        assert!(extract(&regex, &"a".repeat(MAX_TEXT_BYTES + 1), 0, None)
            .unwrap_err()
            .contains("16 MiB"));
        let regex = compile("((a+))", false, false, false).unwrap();
        assert!(extract(&regex, &"a".repeat(6 * 1024 * 1024), 2, None)
            .unwrap_err()
            .contains("output"));
    }

    #[test]
    #[ignore = "requires downloaded real actions in QUICKER_COMPAT_CORPUS"]
    fn downloaded_citavi_text_steps_extract_name_and_id() {
        fn collect(value: &Value, steps: &mut Vec<Value>) {
            if let Some(object) = value.as_object() {
                if object.get("Disabled") == Some(&Value::Bool(true)) {
                    return;
                }
                if matches!(
                    object.get("StepRunnerKey").and_then(Value::as_str),
                    Some("sys:stringProcess" | "sys:regexExtract")
                ) {
                    steps.push(value.clone());
                }
                for child in object.values() {
                    collect(child, steps);
                }
            } else if let Some(array) = value.as_array() {
                for child in array {
                    collect(child, steps);
                }
            }
        }
        let directory = std::env::var("QUICKER_COMPAT_CORPUS").expect("Set QUICKER_COMPAT_CORPUS");
        let mut checked = 0;
        for path in fs::read_dir(directory)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
        {
            let document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            let data: Value = serde_json::from_str(document["Data"].as_str().unwrap()).unwrap();
            let mut steps = Vec::new();
            collect(&data["Steps"], &mut steps);
            if !steps
                .iter()
                .any(|s| s["StepRunnerKey"] == "sys:regexExtract")
            {
                continue;
            }
            let variables: Vec<_> = data["Variables"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|v| {
                    matches!(
                        v["Key"].as_str(),
                        Some("IDtext" | "matchName" | "matchIDnumber")
                    )
                })
                .cloned()
                .collect();
            let data: QuickerPluginData =
                serde_json::from_value(json!({"Variables":variables, "Steps":steps})).unwrap();
            let mut runtime = QuickerRuntime::new(&data, "real-citavi-text".into(), None).unwrap();
            runtime.vars.insert(
                "IDtext".into(),
                json!(" \tTextID： 01234567-89ab-cdef-0123-456789abcdef\r\n"),
            );
            runtime.run_steps(&data.steps).unwrap();
            assert_eq!(runtime.vars["matchName"], "Text", "{}", path.display());
            assert_eq!(
                runtime.vars["matchIDnumber"],
                "01234567-89ab-cdef-0123-456789abcdef"
            );
            checked += 1;
        }
        assert!(checked > 0, "No real Citavi regex steps found");
    }
}
