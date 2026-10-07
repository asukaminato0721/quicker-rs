use super::*;
use serde_json::json;

fn runtime() -> QuickerRuntime {
    reset_action_test_runtime();
    let mut rt = QuickerRuntime::new(
        &serde_json::from_value(json!({})).unwrap(),
        "lists".into(),
        None,
    )
    .unwrap();
    rt.vars
        .insert("items".into(), json!(["中🙂  ", "same", "same", ""]));
    rt
}

fn step() -> QuickerPluginStepDocument {
    serde_json::from_value(json!({"StepRunnerKey":"sys:manageList", "InputParams":{"list":{"VarKey":"items"}}, "OutputParams":{"isSuccess":"ok", "errMessage":"error"}})).unwrap()
}

#[test]
fn confirms_list_copy_and_cancel_preserves_original() {
    let mut rt = runtime();
    let step = step();
    let original = rt.vars["items"].clone();
    rt.manage_list_with(&step, |mut options, _| {
        assert!(options.allow_add && options.allow_edit && options.allow_delete);
        options.items.reverse();
        Ok(None)
    })
    .unwrap();
    assert_eq!(rt.vars["items"], original);
    assert_eq!(rt.vars["ok"], false);
    assert_eq!(rt.vars["error"], "List editing cancelled");
    rt.manage_list_with(&step, |mut options, _| {
        options.items.reverse();
        Ok(Some(options.items))
    })
    .unwrap();
    assert_eq!(rt.vars["items"], json!(["", "same", "same", "中🙂  "]));
    assert_eq!(rt.vars["ok"], true);
    assert_eq!(rt.vars["error"], "");
}

#[test]
fn cancellation_and_invalid_confirmation_never_change_the_list() {
    let mut rt = runtime();
    let mut step = step();
    let original = rt.vars["items"].clone();
    step.input_params
        .insert("stopIfFail".into(), json!({"Value":"1"}));
    assert!(rt.manage_list_with(&step, |_, _| Ok(None)).is_err());
    assert!(rt
        .manage_list_with(&step, |_, _| Ok(Some(vec!["".into(); 100_001])))
        .is_err());
    assert_eq!(rt.vars["items"], original);
    step.input_params
        .insert("stopIfFail".into(), json!({"Value":"0"}));
    assert!(rt
        .manage_list_with(&step, |_, control| {
            control.unwrap().cancel();
            Ok(Some(vec!["wrong".into()]))
        })
        .is_err());
    assert_eq!(rt.vars["items"], original);
}

#[test]
fn validates_binding_items_and_active_advanced_options_before_showing() {
    for inputs in [
        json!({"list":{"Value":["constant"]}}),
        json!({"list":{"VarKey":"missing"}}),
        json!({"list":{"VarKey":"items"},"parseData":{"Value":"1"}}),
        json!({"list":{"VarKey":"items"},"windowSize":{"Value":"NaN"}}),
        json!({"list":{"VarKey":"items"},"addSubprogram":{"Value":"custom"}}),
    ] {
        let mut rt = runtime();
        let mut step = step();
        step.input_params = serde_json::from_value(inputs).unwrap();
        rt.manage_list_with(&step, |_, _| panic!("Invalid options opened a window"))
            .unwrap();
        assert_eq!(rt.vars["ok"], false);
    }
    let mut rt = runtime();
    let mut step = step();
    step.input_params.extend(serde_json::from_value::<Map<String,Value>>(json!({"allowAdd":{"Value":"0"},"addSubprogram":{"Value":"ignored"},"allowEdit":{"Value":"0"},"editSubprogram":{"Value":"ignored"}})).unwrap());
    rt.manage_list_with(&step, |options, _| {
        assert!(!options.allow_add && !options.allow_edit);
        Ok(Some(options.items))
    })
    .unwrap();
    assert_eq!(rt.vars["ok"], true);
    rt.vars.insert("items".into(), json!([1]));
    rt.manage_list_with(&step, |_, _| panic!("Invalid item opened a window"))
        .unwrap();
    assert_eq!(rt.vars["ok"], false);
}

#[test]
fn checker_reports_unsupported_options_and_variable_requirement() {
    for (inputs, code) in [
        (json!({"list":{"VarKey":"items"}}), 0),
        (json!({"list":{"Value":["constant"]}}), 1),
        (
            json!({"list":{"VarKey":"items"},"parseData":{"Value":"1"}}),
            1,
        ),
        (
            json!({"list":{"VarKey":"items"},"help":{"Value":"Help"}}),
            1,
        ),
        (
            json!({"list":{"VarKey":"items"},"allowEdit":{"Value":"0"},"editSubprogram":{"Value":"ignored"}}),
            0,
        ),
    ] {
        let report = compatibility::inspect(&json!({"ActionType":24,"Title":"List", "Data":json!({"Steps":[{"StepRunnerKey":"sys:manageList","InputParams":inputs}]}).to_string()}).to_string());
        assert_eq!(compatibility::exit_code(&report), code, "{report}");
        assert_eq!(report["runtime"]["executed"], false);
    }
}
