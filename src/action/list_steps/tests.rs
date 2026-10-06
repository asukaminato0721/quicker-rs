use super::*;
use serde_json::json;

fn runtime(items: Value) -> QuickerRuntime {
    let data = serde_json::from_value(json!({"Variables":[
        {"Key":"list","Type":4}, {"Key":"result","Type":4},
        {"Key":"count","Type":12}, {"Key":"empty","Type":2}
    ]}))
    .unwrap();
    let mut rt = QuickerRuntime::new(&data, "lists".into(), None).unwrap();
    rt.vars.insert("list".into(), items);
    rt
}

fn step(operation: &str, extra: Value) -> QuickerPluginStepDocument {
    let mut params = json!({"type":{"Value":operation}, "list":{"VarKey":"list"}});
    params
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(
        json!({"StepRunnerKey":"sys:listOperations", "InputParams":params,
        "OutputParams":{"value":"value", "length":"count", "isEmpty":"empty",
            "index":"index", "filterOutItems":"excluded", "errMessage":"error"}}),
    )
    .unwrap()
}

#[test]
fn mutations_preserve_order_and_do_not_assign_value() {
    let mut rt = runtime(json!(["a", "b", "a"]));
    rt.vars.insert("value".into(), json!("unchanged"));
    for (operation, extra, expected) in [
        (
            "append",
            json!({"item":{"Value":"中"}}),
            json!(["a", "b", "a", "中"]),
        ),
        (
            "insertAt",
            json!({"item":{"Value":"end"},"pos":{"Value":4}}),
            json!(["a", "b", "a", "中", "end"]),
        ),
        (
            "insertAt",
            json!({"item":{"Value":"before"},"pos":{"Value":-1}}),
            json!(["a", "b", "a", "中", "before", "end"]),
        ),
        (
            "setAt",
            json!({"item":{"Value":"new"},"pos":{"Value":-2}}),
            json!(["a", "b", "a", "中", "new", "end"]),
        ),
        (
            "remove",
            json!({"item":{"Value":"a"}}),
            json!(["b", "a", "中", "new", "end"]),
        ),
        (
            "remove",
            json!({"item":{"Value":"missing"}}),
            json!(["b", "a", "中", "new", "end"]),
        ),
        (
            "removeAt",
            json!({"pos":{"Value":-1}}),
            json!(["b", "a", "中", "new"]),
        ),
        ("reverse", json!({}), json!(["new", "中", "a", "b"])),
        ("clear", json!({}), json!([])),
    ] {
        rt.run_step(&step(operation, extra)).unwrap();
        assert_eq!(rt.vars["list"], expected, "{operation}");
        assert_eq!(rt.vars["count"], expected.as_array().unwrap().len());
        assert_eq!(rt.vars["empty"], expected.as_array().unwrap().is_empty());
        assert_eq!(rt.vars["value"], "unchanged");
    }
    rt.vars.insert("list".into(), json!(["a", "A", "a"]));
    rt.run_step(&step("removeAllByValue", json!({"item":{"Value":"a"}})))
        .unwrap();
    assert_eq!(rt.vars["list"], json!(["A"]));
}

#[test]
fn queries_use_source_or_result_count_as_in_the_msi() {
    let source = json!(["a", "b", "a", "C"]);
    let mut rt = runtime(source.clone());
    for (operation, extra, expected, count) in [
        ("getAt", json!({"pos":{"Value":-1}}), json!("C"), 4),
        (
            "sub",
            json!({"pos":{"Value":-2},"length":{"Value":10}}),
            json!(["a", "C"]),
            2,
        ),
        ("sub", json!({"pos":{"Value":100}}), json!([]), 0),
        (
            "sub",
            json!({"pos":{"Value":-100},"length":{"Value":2}}),
            json!(["a", "b"]),
            2,
        ),
        ("sub", json!({"length":{"Value":-1}}), json!([]), 0),
        ("distinct", json!({}), json!(["a", "b", "C"]), 3),
        (
            "concat",
            json!({"list2":{"Value":"d\ne"}}),
            json!(["a", "b", "a", "C", "d", "e"]),
            6,
        ),
    ] {
        rt.run_step(&step(operation, extra)).unwrap();
        assert_eq!(rt.vars["value"], expected, "{operation}");
        assert_eq!(rt.vars["count"], count);
        assert_eq!(rt.vars["empty"], count == 0);
        assert_eq!(rt.vars["list"], source);
    }
    for (item, expected) in [("a", 0), ("A", -1), ("C", 3)] {
        rt.run_step(&step("indexOf", json!({"item":{"Value":item}})))
            .unwrap();
        assert_eq!(rt.vars["index"], expected);
        assert_eq!(rt.vars["count"], 4);
    }
    let mut s = step("sub", json!({"length":{"Value":2}}));
    s.output_params.insert("value".into(), json!("result"));
    rt.run_step(&s).unwrap();
    assert_eq!(rt.vars["result"], json!(["a", "b"]));
    // Output replacement can explicitly replace the source list.
    s.output_params.insert("value".into(), json!("list"));
    rt.run_step(&s).unwrap();
    assert_eq!(rt.vars["list"], json!(["a", "b"]));
}

#[test]
fn filters_ignore_case_and_except_output_removes_duplicates() {
    let source = json!(["Ab", "aB", "other", "other", "尾AB", "Ab尾"]);
    for (operation, expected, removed) in [
        (
            "filterByContains",
            json!(["Ab", "aB", "尾AB", "Ab尾"]),
            json!(["other"]),
        ),
        (
            "filterByStarts",
            json!(["Ab", "aB", "Ab尾"]),
            json!(["other", "尾AB"]),
        ),
        (
            "filterByEnds",
            json!(["Ab", "aB", "尾AB"]),
            json!(["other", "Ab尾"]),
        ),
        (
            "filterByRegex",
            json!(["Ab", "Ab尾"]),
            json!(["aB", "other", "尾AB"]),
        ),
    ] {
        let mut rt = runtime(source.clone());
        rt.run_step(&step(
            operation,
            json!({"item":{"Value":"ab"}, "pattern":{"Value":"^Ab"}}),
        ))
        .unwrap();
        assert_eq!(rt.vars["value"], expected);
        assert_eq!(rt.vars["excluded"], removed);
        assert_eq!(rt.vars["count"], expected.as_array().unwrap().len());
        assert_eq!(rt.vars["list"], source);
    }
    for (operation, expected) in [
        ("removeByMatch", json!(["aB", "other", "other", "尾AB"])),
        ("removeByNotMatch", json!(["Ab", "Ab尾"])),
    ] {
        let mut rt = runtime(source.clone());
        rt.run_step(&step(operation, json!({"pattern":{"Value":"^Ab"}})))
            .unwrap();
        assert_eq!(rt.vars["list"], expected);
        assert!(!rt.vars.contains_key("excluded"));
    }
}

#[test]
fn sorting_returns_a_new_list_and_uses_native_file_metadata() {
    for (sort, expected) in [
        ("sortAsc", json!(["file02", "file10", "file2"])),
        ("sortDesc", json!(["file2", "file10", "file02"])),
        ("sortAscNature", json!(["file2", "file02", "file10"])),
    ] {
        let source = json!(["file10", "file2", "file02"]);
        let mut rt = runtime(source.clone());
        rt.run_step(&step(sort, json!({}))).unwrap();
        assert_eq!(rt.vars["value"], expected);
        assert_eq!(rt.vars["list"], source);
    }
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("small");
    let b = dir.path().join("large");
    fs::write(&a, "a").unwrap();
    fs::write(&b, "abc").unwrap();
    let mut rt = runtime(json!([b, a]));
    for (sort, expected) in [
        ("FileSizeAsc", json!([a, b])),
        ("FileSizeDesc", json!([b, a])),
    ] {
        rt.run_step(&step(sort, json!({}))).unwrap();
        assert_eq!(rt.vars["value"], expected);
        assert_eq!(rt.vars["list"], json!([b, a]));
    }
    // Timestamps are equal for this fixture. Stable ordering preserves the input.
    let timestamp = std::time::UNIX_EPOCH + Duration::from_secs(1_000_000);
    for path in [&a, &b] {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(
                fs::FileTimes::new()
                    .set_accessed(timestamp)
                    .set_modified(timestamp),
            )
            .unwrap();
    }
    for sort in [
        "LastAccessTimeAsc",
        "LastAccessTimeDesc",
        "LastWriteTimeAsc",
        "LastWriteTimeDesc",
    ] {
        rt.run_step(&step(sort, json!({}))).unwrap();
        assert_eq!(rt.vars["value"], json!([b, a]));
    }
    // Birth times are not available on every filesystem.
    for sort in ["CreationTimeAsc", "CreationTimeDesc"] {
        let result = rt.run_step(&step(sort, json!({})));
        if fs::metadata(&a).unwrap().created().is_ok() {
            result.unwrap();
        } else {
            assert!(result.unwrap_err().contains("unavailable"));
        }
    }
}

#[test]
fn errors_preserve_lists_and_cancellation_always_stops() {
    let original = json!(["a", "b"]);
    let mut rt = runtime(original.clone());
    rt.vars.insert("value".into(), json!("previous"));
    for (operation, extra) in [
        ("getAt", json!({"pos":{"Value":2}})),
        ("setAt", json!({"pos":{"Value":-3}})),
        ("insertAt", json!({"pos":{"Value":3}})),
        ("removeAt", json!({"pos":{"Value":-5}})),
        ("append", json!({"list":{"Value":"a\nb"}})),
        ("filterByRegex", json!({"pattern":{"Value":"("}})),
        ("removeByMatch", json!({"pattern":{"Value":""}})),
        ("filterByDefault", json!({})),
        ("unknown", json!({})),
        ("none", json!({"stopIfFail":{"Value":"0"}})),
        ("none", json!({"pos":{"Value":2147483648i64}})),
        ("none", json!({"list":{"Value":[1]}})),
    ] {
        assert!(rt.run_step(&step(operation, extra)).is_err(), "{operation}");
        assert_eq!(rt.vars["list"], original);
        assert_eq!(rt.vars["value"], "previous");
        assert!(!rt.vars["error"].as_str().unwrap().is_empty());
    }
    // Preserve the MSI's second negative-index adjustment for removeAt only.
    rt.run_step(&step("removeAt", json!({"pos":{"Value":-3}})))
        .unwrap();
    assert_eq!(rt.vars["list"], json!(["a"]));
    let control = ActionExecutionControl::new();
    control.cancel();
    rt.control = Some(control);
    assert_eq!(
        rt.run_step(&step("append", json!({"item":{"Value":"b"}})))
            .unwrap_err(),
        "Action cancelled"
    );
    assert_eq!(rt.vars["list"], json!(["a"]));
    assert!(check_size(&vec![String::new(); MAX_ITEMS + 1]).is_err());
    assert!(check_size(&["a".repeat(MAX_BYTES + 1)]).is_err());
}

#[test]
fn comments_do_not_evaluate_inputs_or_execute_children() {
    let mut rt = runtime(json!([]));
    let s = json!({"StepRunnerKey":"sys:comment", "InputParams":{"text":{"Value":"$= arbitrary C#"}},
        "IfSteps":[{"StepRunnerKey":"unknown"}]});
    rt.run_step(&serde_json::from_value(s.clone()).unwrap())
        .unwrap();
    let document =
        json!({"Title":"List tests", "ActionType":24,"Data":json!({"Steps":[s]}).to_string()})
            .to_string();
    assert_eq!(
        compatibility::exit_code(&compatibility::inspect(&document)),
        0
    );
}

#[test]
fn checker_reports_list_gaps_and_preserves_json() {
    for (operation, extra, expected) in [
        ("append", json!({"item":{"Value":"text"}}), 0),
        ("filterByDefault", json!({}), 1),
        ("filterByRegex", json!({"pattern":{"Value":"["}}), 1),
        ("none", json!({"stopIfFail":{"Value":false}}), 1),
    ] {
        let s = serde_json::to_value(step(operation, extra)).unwrap();
        let doc =
            json!({"Title":"List tests", "ActionType":24, "Data":json!({"Steps":[s]}).to_string()})
                .to_string();
        let report = compatibility::inspect(&doc);
        assert_eq!(compatibility::exit_code(&report), expected, "{report}");
        assert_eq!(report["raw_round_trip"]["status"], "pass");
        assert_eq!(report["editor_round_trip"]["status"], "pass");
    }
}

#[test]
#[ignore = "requires the downloaded OpenCC export in QUICKER_COMPAT_CORPUS"]
fn downloaded_opencc_list_and_comment_steps_execute_unchanged() {
    fn collect(value: &Value, result: &mut Vec<Value>) {
        if value["Disabled"] == true {
            return;
        }
        if matches!(
            value["StepRunnerKey"].as_str(),
            Some("sys:listOperations" | "sys:comment")
        ) {
            result.push(value.clone());
        }
        match value {
            Value::Array(a) => {
                for v in a {
                    collect(v, result);
                }
            }
            Value::Object(o) => {
                for v in o.values() {
                    collect(v, result);
                }
            }
            _ => {}
        }
    }
    let corpus = std::env::var("QUICKER_COMPAT_CORPUS").expect("Set QUICKER_COMPAT_CORPUS");
    let doc: Value =
        serde_json::from_slice(&fs::read(Path::new(&corpus).join("opencc.json")).unwrap()).unwrap();
    let data: Value = serde_json::from_str(doc["Data"].as_str().unwrap()).unwrap();
    let typed = serde_json::from_value(data.clone()).unwrap();
    let mut rt = QuickerRuntime::new(&typed, "opencc-lists".into(), None).unwrap();
    rt.vars.insert("path".into(), json!("/tmp/列表 中文.txt"));
    let mut steps = Vec::new();
    collect(&data["Steps"], &mut steps);
    assert_eq!(steps.len(), 4);
    for raw in steps {
        rt.run_step(&serde_json::from_value(raw).unwrap()).unwrap();
    }
    assert_eq!(
        rt.vars["文件处理列表"],
        json!(["/tmp/列表 中文.txt", "/tmp/列表 中文.txt"])
    );
    eprintln!("OpenCC: two original append steps and two original comment steps passed with native path input");
}
