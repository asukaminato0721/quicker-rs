use super::*;
use serde_json::json;

#[test]
#[ignore = "requires downloaded real actions in QUICKER_COMPAT_CORPUS"]
fn downloaded_actions_round_trip_without_data_loss() {
    let dir = std::env::var("QUICKER_COMPAT_CORPUS").expect("Set QUICKER_COMPAT_CORPUS");
    let mut count = 0;
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        count += 1;
        let text = std::fs::read_to_string(&path).unwrap();
        let original: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap();
        let result = (|| {
            let action = Action::from_quicker_plugin_json(&original.to_string())?;
            let raw: Value = serde_json::from_str(&action.to_quicker_plugin_json()?).unwrap();
            if raw != original {
                return Err("raw import/export changed data".into());
            }
            let mut draft = LowCodePluginDraft::from_quicker_plugin_json(&original.to_string())?;
            if export(&draft) != original {
                return Err("builder round trip changed data".into());
            }
            draft.title.push_str(" edited");
            let mut expected = original.clone();
            expected["Title"] = json!(draft.title);
            if export(&draft) != expected {
                return Err("metadata edit changed unrelated data".into());
            }
            Ok::<_, String>(())
        })();
        match result {
            Ok(()) => println!("PASS: {} ({})", original["Title"], path.display()),
            Err(err) => failures.push(format!("{}: {err}", path.display())),
        }
    }
    assert!(count > 0, "No downloaded actions found");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    println!("{count} real actions preserved on import/export and metadata editing");
}

fn sample_document() -> Value {
    json!({
        "ActionType": 24, "Title": "Imported", "Id": "stable-id",
        "Association": {"MatchProcess": "firefox", "FutureAssociationFlag": true},
        "FutureMetadata": {"revision": 9},
        "Data": json!({
            "LimitSingleInstance": true, "SummaryExpression": "$result",
            "SubPrograms": [{"Id": "sub", "FutureCode": [1, 2]}],
            "FutureData": [false, 42],
            "Variables": [{"Key": "result", "Type": 2, "DefaultValue": "true",
                           "SaveState": true, "Options": {"secret": false}}],
            "Steps": [
                {"StepRunnerKey": "sys:openUrl", "Id": "first", "Disabled": true,
                 "DelayMs": 71, "Collapsed": true, "Note": "keep me",
                 "InputParams": {"url": {"Value": "https://example.com/first", "Extra": 7},
                                 "custom": {"Value": [1, 2]}},
                 "OutputParams": {"custom": "result"}},
                {"StepRunnerKey": "sys:openUrl", "Id": "second",
                 "InputParams": {"url": {"Value": "https://example.com/second"}}},
                {"StepRunnerKey": "sys:simpleIf", "CustomFlag": false,
                 "InputParams": {"condition": {"VarKey": "result"}},
                 "IfSteps": [{"StepRunnerKey": "vendor:future", "CustomPayload": [1, 2],
                              "Disabled": true}], "ElseSteps": []}
            ]
        }).to_string()
    })
}

fn export(draft: &LowCodePluginDraft) -> Value {
    serde_json::from_str(&draft.to_quicker_json().unwrap()).unwrap()
}

fn data(document: &Value) -> Value {
    serde_json::from_str(document["Data"].as_str().unwrap()).unwrap()
}

#[test]
fn unchanged_builder_round_trip_preserves_entire_document() {
    for original in [
        sample_document(),
        serde_json::from_str(include_str!("../../../tests/fixtures/formula-image.json")).unwrap(),
        serde_json::from_str(include_str!("../../../tests/fixtures/key-macro.json")).unwrap(),
        serde_json::from_str(include_str!("../../../tests/fixtures/launch.json")).unwrap(),
    ] {
        let draft = LowCodePluginDraft::from_quicker_plugin_json(&original.to_string()).unwrap();
        assert_eq!(export(&draft), original);
    }
}

#[test]
fn editing_and_moving_steps_changes_only_requested_fields() {
    let original = sample_document();
    let mut draft = LowCodePluginDraft::from_quicker_plugin_json(&original.to_string()).unwrap();
    draft.title = "Edited".into();
    let LowCodePluginStep::OpenUrl { url } = draft.steps[0].editable_mut() else {
        panic!()
    };
    *url = "https://example.com/edited".into();
    draft.steps.swap(0, 1);
    let mut expected = original.clone();
    expected["Title"] = json!("Edited");
    let mut expected_data = data(&original);
    expected_data["Steps"][0]["InputParams"]["url"]["Value"] = json!("https://example.com/edited");
    expected_data["Steps"].as_array_mut().unwrap().swap(0, 1);
    let actual = export(&draft);
    assert_eq!(data(&actual), expected_data);
    expected["Data"] = actual["Data"].clone();
    assert_eq!(actual, expected);
    assert_eq!(
        export(&LowCodePluginDraft::from_quicker_plugin_json(&actual.to_string()).unwrap()),
        actual
    );
}

#[test]
fn nested_unknown_steps_survive_neighbor_edits_and_deletion() {
    let original = sample_document();
    let mut draft = LowCodePluginDraft::from_quicker_plugin_json(&original.to_string()).unwrap();
    draft.steps.remove(0);
    let LowCodePluginStep::SimpleIf {
        condition,
        if_steps,
        ..
    } = draft.steps[1].editable_mut()
    else {
        panic!()
    };
    assert!(matches!(&if_steps[0], LowCodePluginStep::Raw { .. }));
    *condition = "true".into();
    let mut expected = data(&original);
    expected["Steps"].as_array_mut().unwrap().remove(0);
    expected["Steps"][1]["InputParams"]["condition"] = json!({"VarKey": null, "Value": "true"});
    assert_eq!(data(&export(&draft)), expected);
}

#[test]
fn adding_outputs_preserves_existing_variable_definitions() {
    let original = sample_document();
    let mut draft = LowCodePluginDraft::from_quicker_plugin_json(&original.to_string()).unwrap();
    draft.steps.push(LowCodePluginStep::Assign {
        expression: "value".into(),
        output: "new_output".into(),
    });
    let actual = data(&export(&draft));
    assert_eq!(actual["Variables"][0], data(&original)["Variables"][0]);
    assert_eq!(actual["Variables"][1]["Key"], "new_output");
    assert_eq!(actual["Variables"].as_array().unwrap().len(), 2);
    assert_eq!(actual["SubPrograms"], data(&original)["SubPrograms"]);
}

#[test]
fn launcher_edit_preserves_flags_and_extra_payload() {
    let payload = json!({"FileName": "old.exe", "Arguments": "--test", "WaitForExit": true,
                         "RunAsAdmin": true, "FutureOption": {"a": 1}});
    let original = json!({"ActionType": 11, "Title": "Launch", "Data": format!("json:{payload}")});
    let mut draft = LowCodePluginDraft::from_quicker_plugin_json(&original.to_string()).unwrap();
    draft.launch_path = "/usr/bin/true".into();
    let actual = export(&draft);
    let actual_payload: Value = serde_json::from_str(
        actual["Data"]
            .as_str()
            .unwrap()
            .strip_prefix("json:")
            .unwrap(),
    )
    .unwrap();
    let mut expected = payload;
    expected["FileName"] = json!("/usr/bin/true");
    assert_eq!(actual_payload, expected);
}
