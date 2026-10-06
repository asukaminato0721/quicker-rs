use super::*;
use serde_json::json;

fn runtime(data: Value) -> (QuickerRuntime, QuickerPluginData) {
    let data: QuickerPluginData = serde_json::from_value(data).unwrap();
    let runtime = QuickerRuntime::new(&data, "runtime-tests".into(), None).unwrap();
    (runtime, data)
}

#[test]
fn typed_defaults_assignments_and_false_conditions_execute_correctly() {
    let (mut runtime, data) = runtime(json!({
        "Variables": [
            {"Key": "count", "Type": 1, "DefaultValue": "2"},
            {"Key": "ok", "Type": 2, "DefaultValue": "false"},
            {"Key": "items", "Type": 4, "DefaultValue": "one\r\ntwo"},
            {"Key": "dict", "Type": 10, "DefaultValue": "json:{\"answer\":42}"}
        ],
        "Steps": [
            {"StepRunnerKey": "sys:assign", "InputParams": {"input": {"Value": "$= {count} + 1"}}, "OutputParams": {"output": "count"}},
            {"StepRunnerKey": "sys:simpleIf", "InputParams": {"condition": {"Value": "$= {count} == 3 && !{ok}"}},
             "IfSteps": [{"StepRunnerKey": "sys:assign", "InputParams": {"input": {"Value": "$= {items}[1]"}}, "OutputParams": {"output": "result"}}],
             "ElseSteps": [{"StepRunnerKey": "vendor:must-not-run"}]},
            {"StepRunnerKey": "sys:simpleIf", "InputParams": {"condition": {"Value": "$= {count} < 0"}},
             "IfSteps": [{"StepRunnerKey": "vendor:must-not-run"}]}
        ]
    }));
    assert_eq!(runtime.run_steps(&data.steps).unwrap(), StepFlow::Continue);
    assert_eq!(runtime.vars["count"], json!(3.0));
    assert_eq!(runtime.vars["result"], json!("two"));
    assert_eq!(runtime.vars["dict"]["answer"], json!(42));
}

#[test]
fn failed_expression_stops_condition_but_assignment_honors_failure_output() {
    let (mut runtime, data) = runtime(json!({"Steps": [
        {"StepRunnerKey": "sys:assign", "InputParams": {"input": {"Value": "$= 1 / 0"}, "stopIfFail": {"Value": "0"}}, "OutputParams": {"isSuccess": "ok"}},
        {"StepRunnerKey": "sys:simpleIf", "InputParams": {"condition": {"Value": "$= {missing}.Unsupported()"}},
         "IfSteps": [{"StepRunnerKey": "sys:assign", "InputParams": {"input": {"Value": "wrong branch"}}, "OutputParams": {"output": "sentinel"}}]}
    ]}));
    assert!(runtime
        .run_steps(&data.steps)
        .unwrap_err()
        .contains("Unsupported expression member"));
    assert_eq!(runtime.vars["ok"], json!(false));
    assert!(!runtime.vars.contains_key("sentinel"));
}

#[test]
fn defaults_use_declaration_order_and_conversion_errors_are_explicit() {
    let (runtime, _) = runtime(json!({"Variables": [
        {"Key": "first", "Type": 12, "DefaultValue": "4"},
        {"Key": "next", "Type": 12, "DefaultValue": "$= {first} + 1"}
    ]}));
    assert_eq!(runtime.vars["next"], json!(5));
    let data: QuickerPluginData = serde_json::from_value(json!({"Variables": [
        {"Key": "bad", "Type": 2, "DefaultValue": "maybe"}
    ]}))
    .unwrap();
    assert!(QuickerRuntime::new(&data, "test".into(), None)
        .err()
        .unwrap()
        .contains("Invalid Boolean"));
}

#[test]
fn compatibility_report_checks_expression_syntax_without_execution() {
    let text = json!({"ActionType": 24, "Title": "Syntax", "Data": json!({"Steps": [
        {"StepRunnerKey": "sys:simpleIf", "InputParams": {"condition": {"Value": "$= System.IO.File.Exists(\"/tmp/test\")"}}}
    ]}).to_string()}).to_string();
    let report = compatibility::inspect(&text);
    assert!(report["runtime"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["code"] == "unsupported_expression"));
    assert_eq!(compatibility::exit_code(&report), 1);
}
