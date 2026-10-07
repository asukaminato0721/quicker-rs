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

fn assign(name: &str, expression: &str) -> Value {
    json!({"StepRunnerKey": "sys:assign", "InputParams": {"input": {"Value": expression}}, "OutputParams": {"output": name}})
}

#[test]
fn repeat_checks_stop_after_index_output_and_rechecks_changed_variables() {
    let (mut runtime, data) = runtime(json!({
        "Variables": [{"Key": "visits", "Type": 12, "DefaultValue": "0"},
                      {"Key": "limit", "Type": 1, "DefaultValue": "100"}],
        "Steps": [{"StepRunnerKey": "sys:repeat",
            "InputParams": {"count": {"VarKey": "limit"}, "startIndex": {"Value": "4"},
                "repeatDelayMs": {"Value": "0"}, "stopCondition": {"Value": "$= {index} >= 7 || {visits} > 10"}},
            "OutputParams": {"count": "index"},
            "IfSteps": [assign("visits", "$= {visits} + 1")]}]
    }));
    assert_eq!(runtime.run_steps(&data.steps).unwrap(), StepFlow::Continue);
    assert_eq!(runtime.vars["visits"], json!(3));
    assert_eq!(runtime.vars["index"], json!(7));
}

#[test]
fn nested_loop_controls_apply_to_the_nearest_loop_and_skip_remaining_steps() {
    let (mut runtime, data) = runtime(json!({
        "Variables": [{"Key": "visits", "Type": 12, "DefaultValue": "0"}],
        "Steps": [{"StepRunnerKey": "sys:repeat", "InputParams": {"count": {"Value": "3"}, "repeatDelayMs": {"Value": "0"}},
            "IfSteps": [
                {"StepRunnerKey": "sys:each", "InputParams": {"input": {"Value": [1, 2, 3, 4]}}, "OutputParams": {"item": "item", "count": "index", "isSuccess": "ok"},
                 "IfSteps": [
                    {"StepRunnerKey": "sys:if", "InputParams": {"condition": {"Value": "$= {item} == 2"}}, "IfSteps": [{"StepRunnerKey": "sys:continue"}]},
                    {"StepRunnerKey": "sys:if", "InputParams": {"condition": {"Value": "$= {item} == 3"}}, "IfSteps": [{"StepRunnerKey": "sys:break"}]},
                    assign("visits", "$= {visits} + {item}")
                 ]},
                assign("visits", "$= {visits} + 10")
            ]}]
    }));
    assert_eq!(runtime.run_steps(&data.steps).unwrap(), StepFlow::Continue);
    assert_eq!(runtime.vars["visits"], json!(33));
    assert_eq!(runtime.vars["index"], json!(2));
    assert_eq!(runtime.vars["ok"], json!(true));
}

#[test]
fn stop_exits_nested_loops_and_invalid_loop_control_fails_at_action_boundary() {
    let (mut runtime, data) = runtime(json!({"Steps": [
        {"StepRunnerKey": "sys:repeat", "IfSteps": [
            {"StepRunnerKey": "sys:each", "InputParams": {"input": {"Value": "one\ntwo"}},
                "IfSteps": [{"StepRunnerKey": "sys:stop", "InputParams": {"showMessage": {"Value": "done"}}}, {"StepRunnerKey": "vendor:must-not-run"}]},
            {"StepRunnerKey": "vendor:must-not-run"}]},
        {"StepRunnerKey": "vendor:must-not-run"}
    ]}));
    assert_eq!(
        runtime.run_steps(&data.steps).unwrap(),
        StepFlow::Stop(Some("done".into()))
    );
    for runner in ["sys:break", "sys:continue"] {
        let document = json!({"ActionType": 24, "Title": "Invalid control", "Data": json!({"Steps": [{"StepRunnerKey": runner}]}).to_string()});
        assert!(
            matches!(execute_quicker_action_document(&document.to_string(), None), ExecResult::Err(error) if error.contains("enclosing loop"))
        );
    }
}

#[test]
fn repeat_empty_counts_and_invalid_values_do_not_execute_body() {
    for count in [
        json!(0),
        json!(-2),
        json!(1.5),
        json!("bad"),
        json!(u64::MAX),
    ] {
        let valid_empty = count == json!(0) || count == json!(-2);
        let (mut runtime, data) = runtime(json!({"Steps": [{"StepRunnerKey": "sys:repeat",
            "InputParams": {"count": {"Value": count}}, "IfSteps": [assign("sentinel", "ran")]}]}));
        assert_eq!(runtime.run_steps(&data.steps).is_ok(), valid_empty);
        assert!(!runtime.vars.contains_key("sentinel"));
    }
}

#[test]
fn each_preserves_objects_and_reports_failure_and_parallel_mode() {
    let (mut runtime, data) = runtime(json!({"Steps": [
        {"StepRunnerKey": "sys:each", "InputParams": {"input": {"Value": [{"answer": 42}]}},
            "OutputParams": {"item": "item"}, "IfSteps": [assign("answer", "$= {item}[\"answer\"]")]},
        {"StepRunnerKey": "sys:each", "InputParams": {"input": {"Value": 42}, "stopIfFail": {"Value": "false"}}, "OutputParams": {"isSuccess": "ok"}},
        {"StepRunnerKey": "sys:each", "InputParams": {"input": {"Value": []}, "useMultiThread": {"Value": "1"}, "stopIfFail": {"Value": "false"}}}
    ]}));
    assert!(runtime
        .run_steps(&data.steps)
        .unwrap_err()
        .contains("Parallel each"));
    assert_eq!(runtime.vars["answer"], json!(42));
    assert_eq!(runtime.vars["ok"], json!(false));
    let document =
        json!({"ActionType": 24, "Title": "Each", "Data": serde_json::to_string(&data).unwrap()});
    assert_eq!(
        compatibility::exit_code(&compatibility::inspect(&document.to_string())),
        1
    );
}

#[test]
fn infinite_repeat_remains_cancellable_inside_each_with_failure_ignored() {
    let (mut runtime, data) = runtime(json!({"Steps": [{"StepRunnerKey": "sys:each",
        "InputParams": {"input": {"Value": [1]}, "stopIfFail": {"Value": "false"}},
        "IfSteps": [{"StepRunnerKey": "sys:repeat", "InputParams": {"count": {"Value": "-1"}, "repeatDelayMs": {"Value": "0"}}}]}]}));
    let control = ActionExecutionControl::new();
    runtime.control = Some(control.clone());
    let (send, receive) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        send.send(runtime.run_steps(&data.steps)).unwrap();
    });
    std::thread::sleep(std::time::Duration::from_millis(20));
    control.cancel();
    assert_eq!(
        receive
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
            .unwrap_err(),
        cancellation_error()
    );
    worker.join().unwrap();
}

#[test]
fn if_has_two_branches_and_simple_if_has_only_one() {
    let (mut runtime, data) = runtime(json!({"Steps": [
        {"StepRunnerKey": "sys:if", "InputParams": {"condition": {"Value": "false"}},
            "IfSteps": [{"StepRunnerKey": "vendor:must-not-run"}], "ElseSteps": [assign("branch", "else")]},
        {"StepRunnerKey": "sys:simpleIf", "InputParams": {"condition": {"Value": "false"}},
            "ElseSteps": [{"StepRunnerKey": "vendor:must-not-run"}]}
    ]}));
    assert_eq!(runtime.run_steps(&data.steps).unwrap(), StepFlow::Continue);
    assert_eq!(runtime.vars["branch"], json!("else"));
}

#[test]
fn static_loop_control_checks_parent_loop_without_execution() {
    for (steps, expected) in [
        (json!([{"StepRunnerKey": "sys:continue"}]), 1),
        (
            json!([{"StepRunnerKey": "sys:each", "InputParams": {"input": {"Value": []}, "useMultiThread": {"Value": "false"}},
            "IfSteps": [{"StepRunnerKey": "sys:group", "IfSteps": [{"StepRunnerKey": "sys:break"}]}]}]),
            0,
        ),
        (
            json!([{"StepRunnerKey": "sys:repeat", "IfSteps": [{"StepRunnerKey": "sys:continue"}]}]),
            0,
        ),
    ] {
        let input = json!({"ActionType": 24, "Title": "Control", "Data": json!({"Steps": steps}).to_string()}).to_string();
        let report = compatibility::inspect(&input);
        assert_eq!(compatibility::exit_code(&report), expected);
        assert_eq!(report["runtime"]["executed"], false);
    }
}
