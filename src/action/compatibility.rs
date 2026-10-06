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
        24 | 25 => match parse_json_lenient::<Value>(payload, "Invalid workflow data") {
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
                inspect_subprogram_calls(
                    &data,
                    "/Data",
                    &[],
                    &mut Vec::new(),
                    &mut 0,
                    &mut steps,
                    &mut issues,
                );
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

fn inspect_subprogram_calls(
    data: &Value,
    path: &str,
    parent_scopes: &[Vec<Value>],
    stack: &mut Vec<String>,
    count: &mut usize,
    steps: &mut Vec<Value>,
    issues: &mut Vec<Value>,
) {
    let mut scopes = parent_scopes.to_vec();
    scopes.push(data["SubPrograms"].as_array().cloned().unwrap_or_default());
    let mut pending = vec![(&data["Steps"], format!("{path}/Steps"))];
    while let Some((value, path)) = pending.pop() {
        if let Some(array) = value.as_array() {
            pending.extend(
                array
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (v, format!("{path}/{i}"))),
            );
            continue;
        }
        if value["Disabled"] == true || value["StepRunnerKey"] == "sys:comment" {
            continue;
        }
        for key in ["IfSteps", "ElseSteps"] {
            if value[key].is_array() {
                pending.push((&value[key], format!("{path}/{key}")));
            }
        }
        if value["StepRunnerKey"] != "sys:subprogram" {
            continue;
        }
        let binding = &value["InputParams"]["subProgram"];
        let name = binding["Value"].as_str().unwrap_or_default();
        if binding["VarKey"].is_string() || name.starts_with("$=") || name.contains('{') {
            issue(
                issues,
                &path,
                "dynamic_subprogram_requires_validation",
                "warning",
            );
            continue;
        }
        if stack.iter().any(|v| v == name) {
            issue(
                issues,
                &path,
                "recursive_subprogram_depth_requires_validation",
                "warning",
            );
            continue;
        }
        *count += 1;
        if *count > 256 || stack.len() >= 32 {
            issue(issues, &path, "subprogram_inspection_limit", "blocker");
            return;
        }
        match subprogram::resolve(name, &scopes, subprogram::dependency_dir().as_deref()) {
            Ok(program) => {
                if program.variables.iter().any(|v| v.is_input && matches!(v.value_type, Some(4 | 10))) {
                    issue(issues, &path, "subprogram_collection_reference_semantics_require_validation", "warning");
                }
                let data = serde_json::to_value(program).expect("workflow serialization");
                let child_path = format!("{path}/ResolvedSubprogram");
                visit(&data, &child_path, false, 0, steps, issues);
                stack.push(name.into());
                inspect_subprogram_calls(&data, &child_path, &scopes, stack, count, steps, issues);
                stack.pop();
            }
            Err(error) => issues.push(json!({"path": path, "code": "unresolved_subprogram", "severity": "blocker", "reference": name, "detail": error})),
        }
    }
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
                        match expression::validate(text) {
                            Err(error) => issues.push(json!({"path": format!("{path}/{key}"), "code": "unsupported_expression", "severity": "blocker", "detail": error})),
                            Ok(features) => {
                                if features.path_calls {
                                    issue(issues, &format!("{path}/{key}"), "path_expression_uses_linux_semantics", "warning");
                                }
                                if features.windows_paths {
                                    issue(issues, &format!("{path}/{key}"), "path_expression_contains_windows_path_literal", "warning");
                                }
                            }
                        }
                    }
                }
            }
            if let Some(key) = object.get("StepRunnerKey").and_then(Value::as_str) {
                let implemented = runner::StepRunner::from_key(key).is_some();
                steps.push(json!({"path": path, "runner": key, "disabled": disabled, "runner_implemented": implemented}));
                // CommentStep.Execute returns without reading inputs or children.
                if key == "sys:comment" {
                    return;
                }
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
    if runner == "sys:listOperations" {
        issue(
            issues,
            path,
            "list_values_do_not_share_dotnet_reference_identity",
            "warning",
        );
        let binding = &step["InputParams"]["type"];
        let operation = binding["Value"].as_str().unwrap_or("none");
        let dynamic =
            binding["VarKey"].is_string() || operation.starts_with("$=") || operation.contains('{');
        if dynamic {
            issue(
                issues,
                path,
                "dynamic_option_requires_validation",
                "warning",
            );
        } else if !super::list_steps::OPERATIONS.contains(&operation) {
            issue(issues, path, "unsupported_list_operation", "blocker");
        }
        let stop = &step["InputParams"]["stopIfFail"];
        if !stop["VarKey"].is_string() && !stop["Value"].is_null() && !truthy(Some(&stop["Value"]))
        {
            issue(
                issues,
                path,
                "unsupported_list_continue_on_failure",
                "blocker",
            );
        }
        if dynamic
            || matches!(
                operation,
                "sortAsc"
                    | "sortDesc"
                    | "sortAscNature"
                    | "filterByContains"
                    | "filterByStarts"
                    | "filterByEnds"
            )
        {
            issue(
                issues,
                path,
                "list_sort_and_unicode_rules_can_differ_from_dotnet",
                "warning",
            );
        }
        if dynamic || operation.starts_with("FileSize") || operation.contains("Time") {
            issue(
                issues,
                path,
                "list_metadata_sort_requires_native_regular_files_and_timestamps",
                "warning",
            );
        }
        if matches!(
            operation,
            "removeByMatch" | "removeByNotMatch" | "filterByRegex"
        ) {
            issue(
                issues,
                path,
                "regex_engine_semantics_require_validation",
                "warning",
            );
            let pattern = &step["InputParams"]["pattern"];
            if pattern["VarKey"].is_string()
                || pattern["Value"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("$=") || s.contains('{'))
            {
                issue(
                    issues,
                    path,
                    "dynamic_option_requires_validation",
                    "warning",
                );
            } else if pattern["Value"].as_str().is_none_or(|s| {
                s.is_empty() || regex_steps::compile(s, false, false, false).is_err()
            }) {
                issue(issues, path, "unsupported_list_pattern", "blocker");
            }
        }
    }
    if runner == "sys:selectFile" {
        issue(
            issues,
            path,
            "requires_native_file_dialog_backend",
            "warning",
        );
        issue(
            issues,
            path,
            "file_dialog_extension_and_filter_behavior_depends_on_backend",
            "warning",
        );
        let top = &step["InputParams"]["topMost"];
        if top.is_null() || top["VarKey"].is_string() || truthy(Some(&top["Value"])) {
            issue(
                issues,
                path,
                "file_dialog_topmost_requires_x11_window_manager",
                "warning",
            );
        }
        for key in ["type", "filter", "defaultExt", "initDir", "initFileName"] {
            let binding = &step["InputParams"][key];
            let option_path = format!("{path}/InputParams/{key}");
            if binding["VarKey"].is_string()
                || binding["Value"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("$=") || s.contains('{'))
            {
                issue(
                    issues,
                    &option_path,
                    "dynamic_option_requires_validation",
                    "warning",
                );
            } else if let Some(value) = binding["Value"].as_str() {
                if !super::file_dialogs::validate_option(key, value) {
                    issue(
                        issues,
                        &option_path,
                        "unsupported_file_dialog_option",
                        "blocker",
                    );
                }
            }
        }
    }
    if matches!(runner, "sys:MsgBox" | "sys:userInput" | "sys:selectFolder") {
        issue(issues, path, "requires_native_dialog_backend", "warning");
        if runner == "sys:selectFolder" {
            issue(
                issues,
                path,
                "folder_dialog_does_not_list_open_file_manager_windows",
                "warning",
            );
        } else {
            issue(
                issues,
                path,
                "dialog_appearance_depends_on_desktop_backend",
                "warning",
            );
            let restore = &step["InputParams"]["restoreFocus"];
            if restore.is_null() || truthy(Some(&restore["Value"])) || restore["VarKey"].is_string()
            {
                issue(
                    issues,
                    path,
                    "dialog_focus_restoration_requires_x11",
                    "warning",
                );
            }
        }
        let keys: &[&str] = match runner {
            "sys:MsgBox" => &["operation", "buttons", "icon"],
            "sys:userInput" => dialogs::INPUT_OPTIONS,
            _ => &[],
        };
        for key in keys
            .iter()
            .copied()
            .chain((runner == "sys:userInput").then_some("pattern"))
        {
            let binding = &step["InputParams"][key];
            let option_path = format!("{path}/InputParams/{key}");
            if binding["VarKey"].is_string()
                || binding["Value"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("$=") || (key != "pattern" && s.contains('{')))
            {
                issue(
                    issues,
                    &option_path,
                    "dynamic_option_requires_validation",
                    "warning",
                );
            } else if !binding["Value"].is_null()
                && !dialogs::validate_option(runner, key, &binding["Value"])
            {
                issue(issues, &option_path, "unsupported_dialog_option", "blocker");
            } else if key == "pattern" && binding["Value"].as_str().is_some_and(|s| !s.is_empty()) {
                let pattern = value_to_string(&binding["Value"]);
                if regex_steps::compile(&pattern, false, false, false).is_err()
                    && !pattern.contains('{')
                {
                    issue(issues, &option_path, "unsupported_input_pattern", "blocker");
                } else {
                    issue(
                        issues,
                        &option_path,
                        "regex_engine_semantics_require_validation",
                        "warning",
                    );
                }
            }
        }
    }
    if runner == "sys:regexExtract" {
        issue(
            issues,
            path,
            "regex_engine_semantics_require_validation",
            "warning",
        );
        for key in ["getGroup", "rightToLeft", "pattern"] {
            let binding = &step["InputParams"][key];
            let option_path = format!("{path}/InputParams/{key}");
            if binding["VarKey"].is_string()
                || binding["Value"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("$=") || (key != "pattern" && s.contains('{')))
            {
                issue(
                    issues,
                    &option_path,
                    "dynamic_option_requires_validation",
                    "warning",
                );
                continue;
            }
            let value = value_to_string(&binding["Value"]);
            if (key == "getGroup"
                && !binding["Value"].is_null()
                && regex_steps::mode(&value).is_err())
                || (key == "rightToLeft" && truthy(Some(&binding["Value"])))
            {
                issue(issues, &option_path, "unsupported_option", "blocker");
            } else if key == "pattern"
                && !binding["Value"].is_null()
                && regex_steps::compile(&value, false, false, false).is_err()
            {
                if value.contains('{') {
                    issue(
                        issues,
                        &option_path,
                        "dynamic_option_requires_validation",
                        "warning",
                    );
                } else {
                    issue(issues, &option_path, "unsupported_regex_pattern", "blocker");
                }
            }
        }
        if let Some(outputs) = step["OutputParams"].as_object() {
            for key in ["matchObj", "matchesCollection"] {
                if output_var_name(outputs, key).is_some() {
                    issue(
                        issues,
                        &format!("{path}/OutputParams/{key}"),
                        "dotnet_regex_object_not_supported",
                        "blocker",
                    );
                }
            }
        }
    }
    if runner == "sys:stringProcess" {
        let method = step["InputParams"]["method"]["Value"]
            .as_str()
            .unwrap_or_default();
        if method.eq_ignore_ascii_case("urlEncode") {
            let binding = &step["InputParams"]["srcEncoding"];
            if binding["VarKey"].is_string()
                || binding["Value"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("$=") || s.contains('{'))
            {
                issue(
                    issues,
                    path,
                    "dynamic_option_requires_validation",
                    "warning",
                );
            } else if !binding["Value"].is_null()
                && !matches!(
                    value_to_string(&binding["Value"])
                        .to_ascii_lowercase()
                        .as_str(),
                    "" | "utf8" | "utf-8"
                )
            {
                issue(issues, path, "unsupported_text_encoding", "blocker");
            }
        }
        if method.eq_ignore_ascii_case("toLower") || method.eq_ignore_ascii_case("toUpper") {
            issue(
                issues,
                path,
                "unicode_case_mapping_can_differ_from_dotnet_culture",
                "warning",
            );
        }
    }
    if runner == "sys:getSelectedFiles" {
        issue(
            issues,
            path,
            "requires_x11_file_clipboard_and_target_window",
            "warning",
        );
        for key in ["sortType", "waitMs"] {
            let binding = &step["InputParams"][key];
            let option_path = format!("{path}/InputParams/{key}");
            if binding["VarKey"].is_string()
                || binding["Value"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("$=") || s.contains('{'))
            {
                issue(
                    issues,
                    &option_path,
                    "dynamic_option_requires_validation",
                    "warning",
                );
            } else if !binding["Value"].is_null() {
                let value = value_to_string(&binding["Value"]);
                if (key == "sortType" && !file_selection::SORT_TYPES.contains(&value.as_str()))
                    || (key == "waitMs" && value.parse::<u32>().is_err())
                {
                    issue(issues, &option_path, "unsupported_option", "blocker");
                }
            }
        }
        let sort = step["InputParams"]["sortType"]["Value"]
            .as_str()
            .unwrap_or("Default");
        if matches!(sort, "Default" | "FileName" | "FileNameNature") {
            issue(
                issues,
                path,
                "filename_sort_can_differ_from_windows_locale",
                "warning",
            );
        } else if sort != "Origin" {
            issue(
                issues,
                path,
                "requires_local_regular_file_metadata",
                "warning",
            );
        }
    }
    if runner == "sys:outputText" {
        let method = step["InputParams"]["method"]["Value"]
            .as_str()
            .unwrap_or("paste");
        let keys: &[&str] = if method.eq_ignore_ascii_case("input") {
            &["delayBetweenChar"]
        } else {
            &["delayBeforePaste", "delayAfterPaste", "hideInHistory"]
        };
        for key in keys {
            let binding = &step["InputParams"][key];
            let option_path = format!("{path}/InputParams/{key}");
            if binding["VarKey"].is_string()
                || binding["Value"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("$=") || s.contains('{'))
            {
                issue(
                    issues,
                    &option_path,
                    "dynamic_option_requires_validation",
                    "warning",
                );
            } else if *key == "hideInHistory" {
                if truthy(Some(&binding["Value"])) {
                    issue(
                        issues,
                        &option_path,
                        "clipboard_history_exclusion_not_supported",
                        "blocker",
                    );
                }
            } else if !binding["Value"].is_null()
                && text_steps::delay(&value_to_string(&binding["Value"])).is_err()
            {
                issue(issues, &option_path, "invalid_text_delay", "blocker");
            }
        }
    }
    if runner == "sys:keyoperation" {
        issue(issues, path, "requires_x11_keyboard_layout", "warning");
        for key in ["key", "getRealMouseState"] {
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
            } else if key == "getRealMouseState" {
                if truthy(Some(&binding["Value"]))
                    && !matches!(
                        step["InputParams"]["type"]["Value"].as_str(),
                        Some("key_down" | "key_up")
                    )
                {
                    issue(
                        issues,
                        &option_path,
                        "physical_key_state_not_supported",
                        "blocker",
                    );
                }
            } else {
                match key_steps::key_code(&value_to_string(&binding["Value"])) {
                    Ok(5 | 6) => issue(
                        issues,
                        &option_path,
                        "side_mouse_button_state_not_supported",
                        "blocker",
                    ),
                    Ok(1 | 2 | 4)
                        if step["InputParams"]["type"]["Value"]
                            .as_str()
                            .is_some_and(|s| s != "get_key_state") =>
                    {
                        issue(
                            issues,
                            &option_path,
                            "mouse_injection_requires_mouse_module",
                            "blocker",
                        )
                    }
                    Ok(_) => {}
                    Err(_) => issue(issues, &option_path, "unsupported_windows_key", "blocker"),
                }
            }
        }
    }
    if runner == "sys:run" {
        issue(
            issues,
            path,
            "linux_launch_target_requires_validation",
            "warning",
        );
        let dynamic = |key: &str| {
            let binding = &step["InputParams"][key];
            binding["VarKey"].is_string()
                || binding["Value"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("$=") || s.contains('{') || s.contains('%'))
        };
        for key in [
            "path",
            "arg",
            "windowStyle",
            "runas",
            "waitInputIdle",
            "username",
            "password",
            "envVariables",
            "outputEncoding",
        ] {
            let option_path = format!("{path}/InputParams/{key}");
            if dynamic(key) {
                issue(
                    issues,
                    &option_path,
                    "dynamic_option_requires_validation",
                    "warning",
                );
                continue;
            }
            let value = step["InputParams"][key]["Value"]
                .as_str()
                .unwrap_or_default();
            let supported = match key {
                "path" => {
                    if value.is_empty() {
                        issue(issues, &option_path, "missing_run_target", "blocker");
                    }
                    let windows = value.to_ascii_lowercase().ends_with(".exe")
                        || value.as_bytes().get(1) == Some(&b':');
                    if windows {
                        let alternatives = &step["InputParams"]["alternativePath"];
                        let has_alternative = alternatives["VarKey"].is_string()
                            || alternatives["Value"]
                                .as_str()
                                .is_some_and(|s| !s.trim().is_empty());
                        issue(
                            issues,
                            &option_path,
                            "windows_program_requires_linux_replacement",
                            if has_alternative {
                                "warning"
                            } else {
                                "blocker"
                            },
                        );
                    }
                    true
                }
                "arg" => run_steps::parse_arguments(value).is_ok(),
                "windowStyle" => matches!(value, "" | "0"),
                "runas" | "waitInputIdle" => !truthy(Some(&step["InputParams"][key]["Value"])),
                "username" | "password" => value.is_empty(),
                "envVariables" => run_steps::environment(value).is_ok(),
                "outputEncoding" => matches!(value, "" | "oem" | "utf8"),
                _ => true,
            };
            if !supported {
                issue(issues, &option_path, "unsupported_run_option", "blocker");
            }
        }
        if truthy(Some(
            &step["InputParams"]["activateWindowIfRunning"]["Value"],
        )) || ["mainWinHandle", "mainWinTitle"]
            .iter()
            .any(|key| step["OutputParams"][key].is_string())
        {
            issue(
                issues,
                path,
                "requires_x11_matching_application_window",
                "warning",
            );
        }
    }
    let option: Option<(&str, &[&str], &str)> = match runner {
        "sys:keyoperation" => Some((
            "type",
            &["get_key_state", "key_down", "key_up"],
            "get_key_state",
        )),
        "sys:stop" => Some(("method", &["default", "forcestop"], "default")),
        "sys:stringProcess" => Some(("method", string_process::METHODS, "")),
        "sys:stateStorage" => Some(("type", &["readActionState", "saveActionState"], "")),
        "sys:readFile" => Some(("type", &["text", "image"], "text")),
        "sys:fileOperation" => Some(("type", &["deleteFile"], "")),
        "sys:outputText" => Some(("method", &["paste", "input"], "paste")),
        "sys:getSelectedText" => Some(("format", &["UnicodeText", "Html"], "UnicodeText")),
        "sys:getSelectedFiles" => Some(("operation", &["getSelection"], "getSelection")),
        _ => None,
    };
    if runner == "sys:WriteTextFile"
        || (runner == "sys:readFile" && step["InputParams"]["type"]["Value"] != "image")
    {
        for key in ["encoding", "newLineChars"] {
            if key == "newLineChars" && runner != "sys:WriteTextFile" {
                continue;
            }
            let binding = &step["InputParams"][key];
            let option_path = format!("{path}/InputParams/{key}");
            if binding["VarKey"].is_string()
                || binding["Value"]
                    .as_str()
                    .is_some_and(|v| v.contains('{') || v.starts_with("$="))
            {
                issue(
                    issues,
                    &option_path,
                    "dynamic_option_requires_validation",
                    "warning",
                );
            } else if let Some(value) = binding["Value"].as_str() {
                if !super::file_steps::validate_option(key, value) {
                    issue(
                        issues,
                        &option_path,
                        "unsupported_text_file_option",
                        "blocker",
                    );
                }
            }
        }
        issue(
            issues,
            path,
            "text_file_requires_native_path_and_valid_encoding",
            "warning",
        );
        let key = if runner == "sys:WriteTextFile" {
            "filePath"
        } else {
            "path"
        };
        let binding = &step["InputParams"][key];
        if !binding["VarKey"].is_string()
            && binding["Value"]
                .as_str()
                .is_some_and(|v| !v.starts_with("$=") && super::file_steps::windows_path(v))
        {
            issue(
                issues,
                &format!("{path}/InputParams/{key}"),
                "windows_file_path_requires_linux_replacement",
                "blocker",
            );
        }
    }
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
            let supported = if matches!(runner, "sys:outputText" | "sys:stringProcess") {
                allowed.iter().any(|v| v.eq_ignore_ascii_case(literal))
            } else {
                allowed.contains(&literal)
            };
            if !supported {
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
    if runner == "sys:activateProcessMainWindow" {
        issue(
            issues,
            path,
            "requires_x11_matching_application_window",
            "warning",
        );
        for key in ["hotkey", "className", "windowTitle"] {
            let binding = &step["InputParams"][key];
            let option_path = format!("{path}/InputParams/{key}");
            let value = binding["Value"].as_str().unwrap_or_default();
            if binding["VarKey"].is_string() || value.trim_start().starts_with("$=") {
                issue(
                    issues,
                    &option_path,
                    "dynamic_option_requires_validation",
                    "warning",
                );
            } else if !value.is_empty() {
                let valid = if key == "hotkey" {
                    window_steps::activation_hotkey(value).map(|_| ())
                } else {
                    fancy_regex::Regex::new(value)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                };
                if let Err(error) = valid {
                    issues.push(json!({"path": option_path, "code": "unsupported_option", "detail": error, "severity": "blocker"}));
                }
            }
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
            "Steps": [{"StepRunnerKey": "sys:stringProcess", "InputParams": {"method": {"Value": "futureMethod"}}}]
        }).to_string()}).to_string();
        let report = inspect(&input);
        let issues = report["runtime"]["issues"].as_array().unwrap();
        assert!(issues.iter().any(|i| i["code"] == "unsupported_option"));
        assert!(issues
            .iter()
            .any(|i| i["path"] == "/Data/SubPrograms/0/Steps/0"));
    }

    #[test]
    fn subprogram_resolution_reports_missing_and_nested_dependencies() {
        let call = json!({"StepRunnerKey":"sys:subprogram","InputParams":{"subProgram":{"Value":"local"}}});
        let data = json!({"Steps":[call],"SubPrograms":[{"Name":"local","Steps":[
            {"StepRunnerKey":"sys:subprogram","InputParams":{"subProgram":{"Value":"missing"}}}
        ]}]});
        let report = inspect(
            &json!({"ActionType":24,"Title":"Subprogram", "Data":data.to_string()}).to_string(),
        );
        assert_eq!(report["editor_round_trip"]["status"], "pass");
        assert_eq!(exit_code(&report), 1);
        assert!(report["runtime"]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["code"] == "unresolved_subprogram"
                && i["reference"] == "missing"
                && i["path"].as_str().unwrap().contains("ResolvedSubprogram")));
        let mut data = data;
        data["SubPrograms"][0]["Steps"] = json!([]);
        let report = inspect(
            &json!({"ActionType":24,"Title":"Subprogram", "Data":data.to_string()}).to_string(),
        );
        assert_eq!(exit_code(&report), 0);
    }

    #[test]
    fn run_reports_windows_targets_and_unsupported_options_without_execution() {
        let mut step = json!({"StepRunnerKey":"sys:run", "InputParams": {
            "path":{"Value":"QuickLook.exe"}, "runas":{"Value":"1"},
            "password":{"Value":"do-not-print-this-password"}
        }});
        let report = inspect(&workflow(json!([step])));
        let issues = report["runtime"]["issues"].as_array().unwrap();
        assert!(issues.iter().any(
            |i| i["code"] == "windows_program_requires_linux_replacement"
                && i["severity"] == "blocker"
        ));
        assert!(issues.iter().any(|i| i["code"] == "unsupported_run_option"));
        assert!(!issues.iter().any(|i| i["code"] == "unsupported_runner"));
        assert!(!report.to_string().contains("do-not-print-this-password"));
        assert_eq!(report["runtime"]["executed"], false);
        step["InputParams"] = json!({"path":{"Value":"QuickLook.exe"}, "alternativePath":{"Value":"/usr/bin/preview"}});
        let report = inspect(&workflow(json!([step])));
        assert_eq!(exit_code(&report), 0);
        assert!(report["runtime"]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |i| i["code"] == "windows_program_requires_linux_replacement"
                    && i["severity"] == "warning"
            ));
    }

    #[test]
    fn key_operation_report_checks_keys_and_backend_limits() {
        let report = |operation: &str, key: &str| {
            inspect(&workflow(
                json!([{"StepRunnerKey":"sys:keyoperation", "InputParams":{"type":{"Value":operation}, "key":{"Value":key}}}]),
            ))
        };
        assert_eq!(exit_code(&report("key_up", "Space")), 0);
        assert_eq!(exit_code(&report("get_key_state", "LBUTTON")), 0);
        for (operation, key) in [
            ("key_down", "LBUTTON"),
            ("key_up", "Ctrl+A"),
            ("key_keydown_v1", "Space"),
            ("get_key_state", "XBUTTON1"),
        ] {
            assert_eq!(exit_code(&report(operation, key)), 1);
        }
        assert_eq!(report("key_up", "Space")["runtime"]["executed"], false);
    }

    #[test]
    fn text_output_report_accepts_input_and_rejects_ignored_options() {
        let report = |params: Value| {
            inspect(&workflow(
                json!([{"StepRunnerKey":"sys:outputText", "InputParams":params}]),
            ))
        };
        assert_eq!(
            exit_code(&report(
                json!({"method":{"Value":"input"}, "delayBetweenChar":{"Value":"3"}})
            )),
            0
        );
        assert_eq!(
            exit_code(&report(
                json!({"method":{"Value":"input"}, "delayBetweenChar":{"Value":"-1"}})
            )),
            1
        );
        assert_eq!(
            exit_code(&report(json!({"hideInHistory":{"Value":"1"}}))),
            1
        );
        assert_eq!(report(json!({}))["runtime"]["executed"], false);
    }

    #[test]
    fn selected_files_report_checks_operations_and_sorts() {
        let report = |params: Value| {
            inspect(&workflow(json!([
                {"StepRunnerKey":"sys:getSelectedFiles", "InputParams":params}
            ])))
        };
        for sort in file_selection::SORT_TYPES {
            assert_eq!(exit_code(&report(json!({"sortType":{"Value":sort}}))), 0);
        }
        for params in [
            json!({"operation":{"Value":"setSelection"}}),
            json!({"waitMs":{"Value":"-1"}}),
            json!({"sortType":{"Value":"Unknown"}}),
        ] {
            assert_eq!(exit_code(&report(params)), 1);
        }
        let dynamic = report(json!({"sortType":{"VarKey":"sort"}}));
        assert!(dynamic["runtime"]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["code"] == "dynamic_option_requires_validation"));
        assert_eq!(dynamic["runtime"]["executed"], false);
    }

    #[test]
    fn regex_reports_modes_patterns_objects_and_text_options() {
        let report = |params: Value, outputs: Value| {
            inspect(&workflow(json!([
                {"StepRunnerKey":"sys:regexExtract", "InputParams":params, "OutputParams":outputs}
            ])))
        };
        for mode in ["0", "1", "2", "true", "false"] {
            assert_eq!(
                exit_code(&report(
                    json!({"getGroup":{"Value":mode}, "pattern":{"Value":"(a)(b)"}}),
                    json!({})
                )),
                0
            );
        }
        for params in [
            json!({"getGroup":{"Value":"3"}}),
            json!({"rightToLeft":{"Value":"1"}}),
            json!({"pattern":{"Value":"("}}),
        ] {
            assert_eq!(exit_code(&report(params, json!({}))), 1);
        }
        assert_eq!(
            exit_code(&report(json!({}), json!({"matchObj ":"object"}))),
            1
        );
        assert_eq!(
            exit_code(&report(
                json!({"pattern":{"VarKey":"pattern"}}),
                json!({"matchObj":null})
            )),
            0
        );
        assert_eq!(report(json!({}), json!({}))["runtime"]["executed"], false);
        for method in string_process::METHODS {
            assert_eq!(
                exit_code(&inspect(&workflow(json!([
                    {"StepRunnerKey":"sys:stringProcess", "InputParams":{"method":{"Value":method}}}
                ])))),
                0
            );
        }
        assert_eq!(
            exit_code(&inspect(&workflow(json!([
                {"StepRunnerKey":"sys:stringProcess", "InputParams":{"method":{"Value":"urlEncode"}, "srcEncoding":{"Value":"gbk"}}}
            ])))),
            1
        );
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
