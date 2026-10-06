//! Static inspection only. Importing a file never executes its workflow.
use super::*;
use serde_json::json;

pub(crate) fn inspect(input: &str) -> Value {
    let input = input.trim_start_matches('\u{feff}');
    let mut report = json!({
        "schema_version": 1,
        "import": {"status": "not_checked"},
        "raw_round_trip": {"status": "not_checked"},
        "editor_round_trip": {"status": "not_checked"},
        "editor_metadata_edit": {"status": "not_checked"},
        "runtime": {"executed": false, "status": "not_checked", "steps": [], "issues": [],
            "scope": "Static runner, expression syntax, and selected option checks. Value types, applications, dependencies, permissions, and behavior need runtime validation."}
    });
    let original: Value = match parse_json_lenient(input, "Invalid Quicker JSON") {
        Ok(value) => value,
        Err(error) => {
            report["import"] = json!({"status": "error", "error": error});
            return report;
        }
    };
    report["title"] = original["Title"].clone();
    report["action_type"] = original["ActionType"].clone();
    match Action::from_quicker_plugin_json(input) {
        Ok(action) => {
            report["import"] = json!({"status": "pass"});
            report["raw_round_trip"] = compare(action.to_quicker_plugin_json(), &original);
        }
        Err(error) => report["import"] = json!({"status": "error", "error": error}),
    }
    match LowCodePluginDraft::from_quicker_plugin_json(input) {
        Ok(mut draft) => {
            report["editor_round_trip"] = compare(draft.to_quicker_json(), &original);
            draft.title.push_str(" [compatibility check]");
            let mut expected = original.clone();
            expected["Title"] = json!(draft.title);
            report["editor_metadata_edit"] = compare(draft.to_quicker_json(), &expected);
        }
        Err(error) => report["editor_round_trip"] = json!({"status": "error", "error": error}),
    }
    let mut steps = Vec::new();
    let mut issues = Vec::new();
    let action_type = original["ActionType"].as_u64().unwrap_or(0);
    let payload = original["Data"].as_str().unwrap_or("");
    match action_type {
        24 => match parse_json_lenient::<Value>(payload, "Invalid workflow data") {
            Ok(data) => {
                // Include subprograms and unknown containers, even when the typed
                // runtime parser does not know their fields. Paths are JSON pointers.
                visit(&data, "/Data", false, 0, &mut steps, &mut issues);
                if data["LimitSingleInstance"] == true {
                    issue(
                        &mut issues,
                        "/Data/LimitSingleInstance",
                        "single_instance_not_enforced",
                        "warning",
                    );
                }
                if data["SubPrograms"]
                    .as_array()
                    .is_some_and(|v| !v.is_empty())
                {
                    issue(
                        &mut issues,
                        "/Data/SubPrograms",
                        "subprogram_definitions_not_executable",
                        "warning",
                    );
                }
            }
            Err(_) => issue(
                &mut issues,
                "/Data",
                "missing_or_invalid_workflow_body",
                "blocker",
            ),
        },
        7 => {
            if let Err(error) = parse_quicker_key_macro_script(payload) {
                issues.push(json!({"path": "/Data", "code": "unsupported_macro", "severity": "blocker", "detail": error}));
            }
            issue(
                &mut issues,
                "/Data",
                "requires_input_backend_and_target_window",
                "warning",
            );
        }
        11 => {
            if let Ok(data) = parse_json_lenient::<Value>(
                payload.strip_prefix("json:").unwrap_or(payload),
                "Invalid launch data",
            ) {
                let path = data["FileName"].as_str().unwrap_or("").to_lowercase();
                if path.ends_with(".exe") || path.as_bytes().get(1) == Some(&b':') {
                    issue(
                        &mut issues,
                        "/Data/FileName",
                        "windows_program_requires_linux_replacement",
                        "blocker",
                    );
                }
                issue(
                    &mut issues,
                    "/Data",
                    "launch_target_and_arguments_require_validation",
                    "warning",
                );
            }
        }
        _ => issue(
            &mut issues,
            "/ActionType",
            "unsupported_action_type",
            "blocker",
        ),
    }
    let blocked = issues.iter().any(|i| i["severity"] == "blocker");
    report["runtime"]["status"] = json!(if blocked {
        "blocked"
    } else {
        "needs_runtime_validation"
    });
    report["runtime"]["steps"] = json!(steps);
    report["runtime"]["issues"] = json!(issues);
    report
}

pub(crate) fn exit_code(report: &Value) -> i32 {
    if report["import"]["status"] != "pass" {
        2
    } else if report["runtime"]["status"] == "blocked"
        || [
            "raw_round_trip",
            "editor_round_trip",
            "editor_metadata_edit",
        ]
        .iter()
        .any(|key| report[key]["status"] != "pass")
    {
        1
    } else {
        0
    }
}

fn compare(export: Result<String, String>, expected: &Value) -> Value {
    match export.and_then(|text| serde_json::from_str::<Value>(&text).map_err(|e| e.to_string())) {
        Ok(actual) => json!({"status": if actual == *expected { "pass" } else { "fail" }}),
        Err(error) => json!({"status": "error", "error": error}),
    }
}

fn issue(issues: &mut Vec<Value>, path: &str, code: &str, severity: &str) {
    issues.push(json!({"path": path, "code": code, "severity": severity}));
}

fn visit(
    value: &Value,
    path: &str,
    disabled: bool,
    loop_depth: usize,
    steps: &mut Vec<Value>,
    issues: &mut Vec<Value>,
) {
    match value {
        Value::Object(object) => {
            let disabled = disabled || object.get("Disabled") == Some(&Value::Bool(true));
            if !disabled {
                for key in ["Value", "DefaultValue"] {
                    if key == "Value" && object.get("VarKey").is_some_and(Value::is_string) {
                        continue;
                    }
                    if let Some(text) = object
                        .get(key)
                        .and_then(Value::as_str)
                        .filter(|s| s.trim_start().starts_with("$="))
                    {
                        if let Err(error) = expression::validate(text) {
                            issues.push(json!({"path": format!("{path}/{key}"), "code": "unsupported_expression", "severity": "blocker", "detail": error}));
                        }
                    }
                }
            }
            if let Some(key) = object.get("StepRunnerKey").and_then(Value::as_str) {
                let implemented = runner::StepRunner::from_key(key).is_some();
                steps.push(json!({"path": path, "runner": key, "disabled": disabled, "runner_implemented": implemented}));
                if !disabled {
                    if !implemented {
                        issues.push(json!({"path": path, "code": "unsupported_runner", "runner": key, "severity": "blocker"}));
                    } else {
                        check_options(value, path, key, issues);
                        if matches!(key, "sys:break" | "sys:continue") && loop_depth == 0 {
                            issue(issues, path, "loop_control_outside_loop", "blocker");
                        }
                    }
                }
            }
            for (key, child) in object {
                let child_depth = if key == "SubPrograms" {
                    0
                } else if key == "IfSteps"
                    && matches!(
                        object.get("StepRunnerKey").and_then(Value::as_str),
                        Some("sys:repeat" | "sys:each")
                    )
                {
                    loop_depth + 1
                } else {
                    loop_depth
                };
                let key = key.replace('~', "~0").replace('/', "~1");
                visit(
                    child,
                    &format!("{path}/{key}"),
                    disabled,
                    child_depth,
                    steps,
                    issues,
                );
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                visit(
                    child,
                    &format!("{path}/{index}"),
                    disabled,
                    loop_depth,
                    steps,
                    issues,
                );
            }
        }
        _ => {}
    }
}

fn check_options(step: &Value, path: &str, runner: &str, issues: &mut Vec<Value>) {
    let option: Option<(&str, &[&str], &str)> = match runner {
        "sys:stringProcess" => Some(("method", &["toLower", "urlEncode"], "")),
        "sys:stateStorage" => Some(("type", &["readActionState", "saveActionState"], "")),
        "sys:readFile" => Some(("type", &["image"], "")),
        "sys:fileOperation" => Some(("type", &["deleteFile"], "")),
        "sys:outputText" => Some(("method", &["paste"], "paste")),
        "sys:getSelectedText" => Some(("format", &["UnicodeText", "Html"], "UnicodeText")),
        _ => None,
    };
    if let Some((key, allowed, default)) = option {
        let binding = &step["InputParams"][key];
        let option_path = format!("{path}/InputParams/{key}");
        if binding["VarKey"].is_string()
            || binding["Value"]
                .as_str()
                .is_some_and(|s| s.contains('{') || s.starts_with("$="))
        {
            issue(
                issues,
                &option_path,
                "dynamic_option_requires_validation",
                "warning",
            );
        } else {
            let literal = binding["Value"].as_str().unwrap_or(default);
            if !allowed.contains(&literal) {
                issues.push(json!({"path": option_path, "code": "unsupported_option", "value": literal, "severity": "blocker"}));
            }
        }
    }
    if matches!(
        runner,
        "sys:keyInput" | "sys:outputText" | "sys:getSelectedText"
    ) {
        issue(
            issues,
            path,
            "requires_input_backend_and_target_window",
            "warning",
        );
    }
    if runner == "sys:reportProgress" {
        issue(issues, path, "progress_reporting_is_noop", "warning");
    }
    if matches!(runner, "sys:waitClipboardChange" | "sys:getSelectedText") {
        issue(issues, path, "requires_x11_clipboard_events", "warning");
        let keys: &[&str] = if runner == "sys:waitClipboardChange" {
            &["monitorWaitWin"]
        } else {
            &["tryNoClipboard", "useActionParam"]
        };
        for key in keys {
            let binding = &step["InputParams"][key];
            let option_path = format!("{path}/InputParams/{key}");
            if binding["VarKey"].is_string()
                || binding["Value"]
                    .as_str()
                    .is_some_and(|s| s.contains('{') || s.starts_with("$="))
            {
                issue(
                    issues,
                    &option_path,
                    "dynamic_option_requires_validation",
                    "warning",
                );
            } else if truthy(Some(&binding["Value"])) {
                issue(issues, &option_path, "unsupported_option", "blocker");
            }
        }
        if runner == "sys:getSelectedText" && step["OutputParams"]["url"].is_string() {
            issue(
                issues,
                path,
                "clipboard_source_url_not_available",
                "warning",
            );
        }
    }
    if runner == "sys:each" {
        let binding = &step["InputParams"]["useMultiThread"];
        let path = format!("{path}/InputParams/useMultiThread");
        if binding["VarKey"].is_string()
            || binding["Value"]
                .as_str()
                .is_some_and(|s| s.contains('{') || s.starts_with("$="))
        {
            issue(
                issues,
                &path,
                "dynamic_option_requires_validation",
                "warning",
            );
        } else if truthy(Some(&binding["Value"])) {
            issue(issues, &path, "parallel_each_not_supported", "blocker");
        }
    }
    if matches!(runner, "sys:each" | "sys:repeat") {
        let binding = &step["InputParams"]["progressBarTitle"];
        if binding["VarKey"].is_string() || binding["Value"].as_str().is_some_and(|s| !s.is_empty())
        {
            issue(issues, path, "loop_progress_not_displayed", "warning");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workflow(steps: Value) -> String {
        json!({"ActionType": 24, "Title": "Test", "Data": json!({"Steps": steps}).to_string()})
            .to_string()
    }

    #[test]
    fn preserves_unknown_steps_but_reports_runtime_blockers() {
        let report = inspect(&workflow(
            json!([{"StepRunnerKey": "sys:future", "Extra": 9}]),
        ));
        assert_eq!(report["editor_round_trip"]["status"], "pass");
        assert_eq!(report["editor_metadata_edit"]["status"], "pass");
        assert_eq!(report["runtime"]["issues"][0]["path"], "/Data/Steps/0");
        assert_eq!(exit_code(&report), 1);
        assert_eq!(report["runtime"]["executed"], false);
    }

    #[test]
    fn disabled_parent_disables_unknown_descendants() {
        let report = inspect(&workflow(
            json!([{"StepRunnerKey": "sys:group", "Disabled": true,
            "IfSteps": [{"StepRunnerKey": "sys:future"}]}]),
        ));
        assert_eq!(exit_code(&report), 0);
        assert_eq!(report["runtime"]["steps"][1]["disabled"], true);
        assert_eq!(report["runtime"]["status"], "needs_runtime_validation");
    }

    #[test]
    fn checks_subprograms_and_unsupported_options() {
        let input = json!({"ActionType": 24, "Title": "Test", "Data": json!({
            "SubPrograms": [{"Steps": [{"StepRunnerKey": "sys:run"}]}],
            "Steps": [{"StepRunnerKey": "sys:stringProcess", "InputParams": {"method": {"Value": "toUpper"}}}]
        }).to_string()}).to_string();
        let report = inspect(&input);
        let issues = report["runtime"]["issues"].as_array().unwrap();
        assert!(issues.iter().any(|i| i["code"] == "unsupported_option"));
        assert!(issues
            .iter()
            .any(|i| i["path"] == "/Data/SubPrograms/0/Steps/0"));
    }

    #[test]
    fn invalid_and_unsupported_documents_are_machine_readable() {
        assert_eq!(exit_code(&inspect("not json")), 2);
        assert_eq!(
            exit_code(&inspect(r#"{"ActionType":99,"Title":"Unknown"}"#)),
            2
        );
        assert_eq!(
            exit_code(&inspect(
                "\u{feff}{\"ActionType\":7,\"Title\":\"Keys\",\"Data\":\"Key(A)\"}"
            )),
            1
        );
    }
}
