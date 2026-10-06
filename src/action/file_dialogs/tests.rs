use super::*;
use serde_json::json;

#[test]
fn filters_select_default_extension_and_translate_windows_wildcards() {
    let list = filters("All|*.*|Images|*.jpg;*.png|Text|*.txt", "png").unwrap();
    assert_eq!(
        list[0],
        Filter {
            name: "Images".into(),
            patterns: vec!["*.jpg".into(), "*.png".into()]
        }
    );
    assert_eq!(list[1].patterns, ["*"]);
    assert!(filters("Text|*.txt|broken", "").is_err());
    assert!(filters("Text|*.txt;", "").is_err());
    assert!(filters("Text\nOther|*.txt", "").is_err());
    assert!(filters("Text|[a-z].txt", "").is_err());
    assert!(filters("Exact|readme.txt", "").is_err());
    assert!(validate_option("defaultExt", ".tar.gz"));
    assert!(!validate_option("defaultExt", "../txt"));
}

#[test]
fn file_dialog_protocol_preserves_delimiters_unicode_and_whitespace() {
    assert_eq!(
        decode_paths(
            b"file:///tmp/a%20%23%25%7C%0A%22.txt\nfile:///tmp/%E4%B8%AD%20%20\n",
            "kdialog",
            "openMultiFile",
            "unused"
        )
        .unwrap(),
        ["/tmp/a #%|\n\".txt", "/tmp/中  "]
    );
    assert_eq!(
        decode_paths(
            "/tmp/a|\n中  <random>/tmp/quote\"\n".as_bytes(),
            "zenity",
            "openMultiFile",
            "<random>"
        )
        .unwrap(),
        ["/tmp/a|\n中  ", "/tmp/quote\""]
    );
    assert_eq!(
        decode_paths(b"/tmp/ends\n\n", "zenity", "openFile", "unused").unwrap(),
        ["/tmp/ends\n"]
    );
    for value in [
        b"https://example.org/file\n".as_slice(),
        b"file://remote/tmp/file\n",
        b"file:///tmp/a?query\n",
        b"file:///tmp/%ff\n",
    ] {
        assert!(decode_paths(value, "kdialog", "openFile", "unused").is_err());
    }
    assert!(decode_paths(b"\n", "zenity", "openFile", "unused").is_err());
}

#[test]
fn file_dialog_defaults_failures_and_cancellation_match_module_outputs() {
    let mut rt = QuickerRuntime::new(
        &serde_json::from_value(json!({})).unwrap(),
        "file-dialog".into(),
        None,
    )
    .unwrap();
    let mut step: QuickerPluginStepDocument = serde_json::from_value(json!({"StepRunnerKey":"sys:selectFile", "OutputParams":{"path":"path", "pathList":"paths", "isSuccess":"ok"}})).unwrap();
    let options = rt.file_dialog_options(&step).unwrap();
    assert_eq!(options.kind, "openFile");
    assert_eq!(options.extension, "txt");
    assert!(options.top_most);
    rt.vars.insert("path".into(), json!("previous"));
    rt.vars.insert("paths".into(), json!(["previous"]));
    step.input_params = serde_json::from_value(json!({"type":{"Value":"unsupported"}})).unwrap();
    assert!(rt.run_step(&step).unwrap_err().contains("type"));
    assert_eq!(rt.vars["ok"], false);
    step.input_params.insert(
        "stopIfFail".into(),
        serde_json::from_value(json!({"Value":"0"})).unwrap(),
    );
    rt.run_step(&step).unwrap();
    assert_eq!(rt.vars["path"], "previous");
    assert_eq!(rt.vars["paths"], json!(["previous"]));
    let control = ActionExecutionControl::new();
    control.cancel();
    rt.control = Some(control);
    assert_eq!(rt.run_step(&step).unwrap_err(), "Action cancelled");
}

#[test]
fn compatibility_report_checks_file_dialog_options_without_opening_windows() {
    for (inputs, code) in [
        (json!({}), 0),
        (json!({"type":{"Value":"openMultiFile"}}), 0),
        (json!({"filter":{"Value":"Text|*.txt|bad"}}), 1),
        (json!({"initDir":{"Value":"C:\\files"}}), 1),
    ] {
        let report = compatibility::inspect(&json!({"ActionType":24,"Title":"File selector", "Data":json!({"Steps":[{"StepRunnerKey":"sys:selectFile", "InputParams":inputs}]}).to_string()}).to_string());
        assert_eq!(compatibility::exit_code(&report), code, "{report}");
        assert_eq!(report["runtime"]["executed"], false);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn backend_arguments_keep_file_names_out_of_options_and_shells() {
    let options = FileDialog {
        kind: "saveFile".into(),
        title: "Title".into(),
        initial: "/tmp/--literal $(name).txt".into(),
        filters: filters("Text|*.txt;*.md|All|*.*", "txt").unwrap(),
        extension: "txt".into(),
        top_most: true,
    };
    let kde = command(&options, "kdialog", "separator");
    let args = kde
        .get_args()
        .map(|s| s.to_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        args,
        [
            "--title",
            "Title",
            "--getsaveurl",
            "--",
            "/tmp/--literal $(name).txt",
            "Text (*.txt *.md)\nAll (*)"
        ]
    );
    let gtk = command(&options, "zenity", "separator");
    let args = gtk
        .get_args()
        .map(|s| s.to_str().unwrap())
        .collect::<Vec<_>>();
    assert!(args.contains(&"--filename=/tmp/--literal $(name).txt"));
    assert!(args.contains(&"--file-filter=Text|*.txt *.md"));
}
