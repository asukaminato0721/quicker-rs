use super::*;
use serde_json::json;

fn rt() -> QuickerRuntime {
    reset_action_test_runtime();
    let mut rt = QuickerRuntime::new(
        &serde_json::from_value(json!({})).unwrap(),
        "form".into(),
        None,
    )
    .unwrap();
    rt.vars.extend([
        ("name".into(), json!("原值  ")),
        ("count".into(), json!(2)),
        ("yes".into(), json!(false)),
        ("choice".into(), json!("b")),
        ("choices".into(), json!("First(tooltip)|a\r\n\"第二项\"|b")),
    ]);
    rt.variable_types.extend([
        ("name".into(), 0),
        ("count".into(), 12),
        ("yes".into(), 2),
        ("choice".into(), 0),
    ]);
    rt
}
fn step(fields: Value) -> QuickerPluginStepDocument {
    serde_json::from_value(json!({"StepRunnerKey":"sys:form", "InputParams": {
        "formDef":{"Value":json!({"Fields":fields}).to_string()}, "stopIfFail":{"Value":false}
    },"OutputParams":{"isSuccess":"ok","button":"button","errMessage":"error"}}))
    .unwrap()
}

#[test]
fn commits_all_typed_fields_and_resolves_choices_after_json_parsing() {
    let mut rt = rt();
    rt.vars.insert(
        "choices".into(),
        json!(["First(tooltip)|a", "\"第二项\"|b"]),
    );
    let step = step(json!([
        {"InputMethod":100,"Label":"Section"},
        {"InputMethod":1,"FieldKey":"name","Label":"Name","IsRequired":true},
        {"InputMethod":7,"FieldKey":"count","Label":"Count","MinValue":"1","MaxValue":"9"},
        {"InputMethod":6,"FieldKey":"yes","Label":"Yes"},
        {"InputMethod":3,"FieldKey":"choice","Label":"Choice","SelectionItems":"$${choices}"}
    ]));
    rt.form_with(&step, |options, _, _| {
        assert_eq!(options.fields[4].choices[1].title, "\"第二项\"");
        assert_eq!(options.fields[4].initial, "b");
        assert_eq!(options.fields[4].choices[0].help, "tooltip");
        Ok(Some(
            ["", "新值  ", "3", "true", "a"].map(String::from).to_vec(),
        ))
    })
    .unwrap();
    assert_eq!(rt.vars["name"], "新值  ");
    assert_eq!(rt.vars["count"], 3);
    assert_eq!(rt.vars["yes"], true);
    assert_eq!(rt.vars["choice"], "a");
    assert_eq!(rt.vars["ok"], true);
    assert_eq!(rt.vars["button"], "");
}

#[test]
fn invalid_submission_and_cancellation_do_not_write_fields() {
    let mut rt = rt();
    let mut step = step(json!([
        {"InputMethod":1,"FieldKey":"name","Label":"Name"},
        {"InputMethod":7,"FieldKey":"count","Label":"Count","MinValue":"1","MaxValue":"9"}
    ]));
    rt.form_with(&step, |_, _, _| {
        Ok(Some(vec!["changed".into(), "10".into()]))
    })
    .unwrap();
    assert_eq!(rt.vars["name"], "原值  ");
    assert_eq!(rt.vars["count"], 2);
    assert_eq!(rt.vars["ok"], false);
    rt.form_with(&step, |_, _, _| Ok(None)).unwrap();
    assert_eq!(rt.vars["button"], "Cancel");
    step.input_params.remove("stopIfFail");
    assert!(rt.form_with(&step, |_, _, _| Ok(None)).is_err());
    step.input_params
        .insert("stopIfFail".into(), json!({"Value":false}));
    assert!(rt
        .form_with(&step, |_, _, control| {
            control.unwrap().cancel();
            Ok(Some(vec!["wrong".into(), "4".into()]))
        })
        .is_err());
    assert_eq!(rt.vars["name"], "原值  ");
}

#[test]
fn dictionary_fields_preserve_other_keys_and_readonly_values() {
    let mut rt = rt();
    rt.vars
        .insert("dict".into(), json!({"name":"old","count":2,"other":[1]}));
    let fields = json!([
        {"InputMethod":1,"FieldKey":"name","Label":"Name","DictVarType":0},
        {"InputMethod":7,"FieldKey":"count","Label":"Count","DictVarType":12,"ReadOnly":true}
    ]);
    for mode in ["dict", "dict_dynamic"] {
        let mut step = step(json!([]));
        step.input_params
            .insert("operation".into(), json!({"Value":mode}));
        step.input_params
            .insert("dictVar".into(), json!({"VarKey":"dict"}));
        step.input_params.insert(if mode == "dict" {"formForDictDef"} else {"dynamicFormForDictDef"}.into(),
            json!({"Value":if mode == "dict" {json!({"Fields":fields}).to_string()} else {fields.to_string()}}));
        rt.form_with(&step, |_, _, _| {
            Ok(Some(vec!["new  ".into(), "999".into()]))
        })
        .unwrap();
        assert_eq!(
            rt.vars["dict"],
            json!({"name":"new  ","count":2,"other":[1]})
        );
        assert_eq!(rt.vars["ok"], true);
    }
}

#[test]
fn validates_required_pattern_utf16_limits_and_unsupported_fields() {
    let mut rt = rt();
    for (settings, value) in [
        (json!({"IsRequired":true}), ""),
        (json!({"Pattern":"^[A-Z]+$"}), "bad"),
        (json!({"MaxLength":1}), "🙂"),
    ] {
        let mut field = json!({"InputMethod":1,"FieldKey":"name","Label":"Name"});
        field
            .as_object_mut()
            .unwrap()
            .extend(settings.as_object().unwrap().clone());
        rt.form_with(&step(json!([field])), |_, _, _| {
            Ok(Some(vec![value.into()]))
        })
        .unwrap();
        assert_eq!(rt.vars["ok"], false);
        assert_eq!(rt.vars["name"], "原值  ");
    }
    for field in [
        json!({"InputMethod":1,"FieldKey":"missing","Label":"Missing"}),
        json!({"InputMethod":1,"FieldKey":"name","Label":"Name","ExtraSettings":"compute:1"}),
        json!({"InputMethod":5,"FieldKey":"name","Label":"Date"}),
        json!({"InputMethod":1,"FieldKey":"name","Label":"Name","Pattern":"$=true"}),
        json!({"InputMethod":3,"FieldKey":"name","Label":"Name","SelectionItems":"[icon]Choice|value"}),
    ] {
        rt.form_with(&step(json!([field])), |_, _, _| {
            panic!("Invalid form opened")
        })
        .unwrap();
        assert_eq!(rt.vars["ok"], false);
    }
}

#[test]
fn checker_inspects_static_json_and_reports_unsupported_options() {
    for (fields, options, code) in [
        (
            json!([{"InputMethod":3,"FieldKey":"name","Label":"Name","SelectionItems":"$${choices}"}]),
            json!({}),
            0,
        ),
        (
            json!([{"InputMethod":5,"FieldKey":"name","Label":"Date"}]),
            json!({}),
            1,
        ),
        (
            json!([{"InputMethod":1,"FieldKey":"name","Label":"Name","ExtraSettings":"compute:1"}]),
            json!({}),
            1,
        ),
        (
            json!([{"InputMethod":3,"FieldKey":"name","Label":"Name","SelectionItems":"$= System.IO.File.ReadAllText(\"file\")"}]),
            json!({}),
            1,
        ),
        (
            json!([{"InputMethod":1,"FieldKey":"name","Label":"Name"}]),
            json!({"customButtons":{"Value":"Finish|value"}}),
            1,
        ),
    ] {
        let mut step = serde_json::to_value(step(fields)).unwrap();
        step["InputParams"]
            .as_object_mut()
            .unwrap()
            .extend(options.as_object().unwrap().clone());
        let report = compatibility::inspect(
            &json!({"ActionType":24,"Title":"Form","Data":json!({"Steps":[step]}).to_string()})
                .to_string(),
        );
        assert_eq!(compatibility::exit_code(&report), code, "{report}");
        assert_eq!(report["runtime"]["executed"], false);
    }
}

#[test]
fn dropdown_defaults_use_exact_then_unicode_case_matching_and_unknown_values_clear() {
    for (current, expected) in [("é", "é"), ("É", "É"), ("café", "CAFÉ"), ("missing", "")] {
        let mut rt = rt();
        rt.vars.insert("choice".into(), json!(current));
        let step = step(
            json!([{"InputMethod":3,"FieldKey":"choice","Label":"Choice","SelectionItems":"First|é\nSecond|É\nThird|CAFÉ"}]),
        );
        rt.form_with(&step, |options, _, _| {
            assert_eq!(options.fields[0].initial, expected);
            Ok(None)
        })
        .unwrap();
    }
}
