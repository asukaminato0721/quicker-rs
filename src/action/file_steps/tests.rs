use super::*;
use serde_json::json;

fn runtime() -> QuickerRuntime {
    QuickerRuntime::new(
        &serde_json::from_value(json!({})).unwrap(),
        "files".into(),
        None,
    )
    .unwrap()
}

fn step(runner: &str, inputs: Value) -> QuickerPluginStepDocument {
    serde_json::from_value(json!({"StepRunnerKey": runner, "InputParams": inputs,
        "OutputParams": {"isSuccess":"ok", "txt":"text"}}))
    .unwrap()
}

#[test]
fn unicode_files_have_expected_bytes_and_bom_detection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("父目录/ text  ");
    let mut rt = runtime();
    for (encoding, bom, expected) in [
        (
            "utf-8",
            false,
            vec![0x41, 0xe4, 0xb8, 0xad, 0xf0, 0x9f, 0x98, 0x80],
        ),
        (
            "utf-8",
            true,
            vec![
                0xef, 0xbb, 0xbf, 0x41, 0xe4, 0xb8, 0xad, 0xf0, 0x9f, 0x98, 0x80,
            ],
        ),
        (
            "utf-16",
            false,
            vec![0xff, 0xfe, 0x41, 0, 0x2d, 0x4e, 0x3d, 0xd8, 0, 0xde],
        ),
        (
            "utf-16BE",
            false,
            vec![0xfe, 0xff, 0, 0x41, 0x4e, 0x2d, 0xd8, 0x3d, 0xde, 0],
        ),
        (
            "utf-32",
            false,
            vec![
                0xff, 0xfe, 0, 0, 0x41, 0, 0, 0, 0x2d, 0x4e, 0, 0, 0, 0xf6, 1, 0,
            ],
        ),
        (
            "utf-32BE",
            false,
            vec![
                0, 0, 0xfe, 0xff, 0, 0, 0, 0x41, 0, 0, 0x4e, 0x2d, 0, 1, 0xf6, 0,
            ],
        ),
    ] {
        rt.run_step(&step("sys:WriteTextFile", json!({"filePath":{"Value":path},
            "content":{"Value":"A中😀"}, "encoding":{"Value":encoding}, "addUtf8Bom":{"Value":bom}}))).unwrap();
        assert_eq!(fs::read(&path).unwrap(), expected, "{encoding}");
        rt.run_step(&step("sys:readFile", json!({"path":{"Value":path}})))
            .unwrap();
        assert_eq!(rt.vars["text"], "A中😀");
        assert_eq!(rt.vars["ok"], true);
    }
}

#[test]
fn append_writes_one_bom_and_normalizes_newlines_before_adding_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("text");
    let mut rt = runtime();
    let write = step(
        "sys:WriteTextFile",
        json!({"filePath":{"Value":path},
        "content":{"Value":"a\r\nb\rc\n"}, "addUtf8Bom":{"Value":"1"},
        "appendMode":{"Value":"1"}, "newLineChars":{"Value":"\n"}, "addNewLine":{"Value":"1"}}),
    );
    rt.run_step(&write).unwrap();
    rt.run_step(&write).unwrap();
    assert_eq!(
        fs::read(&path).unwrap(),
        b"\xef\xbb\xbfa\nb\nc\n\na\nb\nc\n\n"
    );
    rt.run_step(&step(
        "sys:WriteTextFile",
        json!({"filePath":{"Value":path}, "content":{"Value":"x"}, "addNewLine":{"Value":"1"}}),
    ))
    .unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"x\r\n");
    let script = dir.path().join("empty.PS1");
    rt.run_step(&step(
        "sys:WriteTextFile",
        json!({"filePath":{"Value":script}}),
    ))
    .unwrap();
    assert_eq!(fs::read(&script).unwrap(), b"\xef\xbb\xbf");
}

#[test]
fn failures_stop_by_default_and_do_not_destroy_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("text");
    fs::write(&path, "original").unwrap();
    let mut rt = runtime();
    for (encoding, content) in [
        ("default", "new"),
        ("auto", "new"),
        ("utf-7", "new"),
        ("us-ascii", "中文"),
    ] {
        assert!(rt
            .run_step(&step(
                "sys:WriteTextFile",
                json!({"filePath":{"Value":path},
            "content":{"Value":content}, "encoding":{"Value":encoding}})
            ))
            .is_err());
        assert_eq!(rt.vars["ok"], false);
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
    }
    rt.vars.insert("text".into(), json!("previous"));
    let missing = dir.path().join("missing");
    assert!(rt
        .run_step(&step("sys:readFile", json!({"path":{"Value":missing}})))
        .is_err());
    rt.run_step(&step(
        "sys:readFile",
        json!({"path":{"Value":missing},"stopIfFail":{"Value":"0"}}),
    ))
    .unwrap();
    assert_eq!(rt.vars["ok"], false);
    assert_eq!(rt.vars["text"], "previous");
    rt.run_step(&step(
        "sys:WriteTextFile",
        json!({"filePath":{"Value":dir.path()},"stopIfFail":{"Value":"0"}}),
    ))
    .unwrap();
    assert_eq!(rt.vars["ok"], false);
}

#[test]
fn invalid_encoding_bytes_and_large_files_fail_without_replacement_text() {
    for (name, bytes) in [
        ("utf-8", vec![255]),
        ("utf-16", vec![0, 216]),
        ("utf-16", vec![0]),
        ("utf-32", vec![0, 0, 17, 0]),
        ("utf-32", vec![0]),
        ("ascii", vec![128]),
    ] {
        assert!(Encoding::parse(name).unwrap().decode(&bytes).is_err());
    }
    assert_eq!(
        Encoding::parse("ascii").unwrap().decode(b"ABC").unwrap(),
        "ABC"
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large");
    fs::File::create(&path)
        .unwrap()
        .set_len(MAX_TEXT_BYTES as u64 + 1)
        .unwrap();
    assert!(read_text_bytes(path.to_str().unwrap(), None)
        .unwrap_err()
        .contains("16 MiB"));
    assert!(
        write_text_bytes(path.to_str().unwrap(), b"x", b"", true, None)
            .unwrap_err()
            .contains("16 MiB")
    );
    assert_eq!(fs::metadata(path).unwrap().len(), MAX_TEXT_BYTES as u64 + 1);
}

#[test]
fn cancellation_is_not_suppressed_and_keeps_existing_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("text");
    fs::write(&path, "original").unwrap();
    let mut rt = runtime();
    let control = ActionExecutionControl::new();
    control.cancel();
    rt.control = Some(control);
    for (runner, key) in [("sys:WriteTextFile", "filePath"), ("sys:readFile", "path")] {
        let s = step(
            runner,
            json!({key:{"Value":path}, "stopIfFail":{"Value":"0"}}),
        );
        assert_eq!(rt.run_step(&s).unwrap_err(), "Action cancelled");
    }
    assert_eq!(fs::read_to_string(path).unwrap(), "original");
}

#[cfg(unix)]
#[test]
fn special_files_do_not_block_or_receive_writes() {
    use std::ffi::CString;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fifo");
    let name = CString::new(path.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert!(read_text_bytes(path.to_str().unwrap(), None).is_err());
    assert!(write_text_bytes(path.to_str().unwrap(), b"test", b"", false, None).is_err());
    assert!(read_text_bytes("/dev/null", None).is_err());
    assert!(write_text_bytes("/dev/null", b"test", b"", false, None).is_err());
}

#[test]
fn static_report_rejects_unsupported_file_options_and_windows_paths() {
    let workflow = |inputs: Value| {
        json!({"ActionType":24,"Title":"File tests","Data":json!({"Steps":[
        {"StepRunnerKey":"sys:WriteTextFile","InputParams":inputs},
        {"StepRunnerKey":"sys:readFile"}]}).to_string()})
        .to_string()
    };
    let report = compatibility::inspect(&workflow(json!({"filePath":{"Value":"/tmp/test"}})));
    assert_eq!(compatibility::exit_code(&report), 0);
    for inputs in [
        json!({"encoding":{"Value":"auto"}}),
        json!({"newLineChars":{"Value":"\\n"}}),
        json!({"filePath":{"Value":"C:\\test.txt"}}),
    ] {
        let report = compatibility::inspect(&workflow(inputs));
        assert_eq!(compatibility::exit_code(&report), 1);
    }
}

#[test]
#[ignore = "requires the downloaded OpenCC export in QUICKER_COMPAT_CORPUS"]
fn downloaded_opencc_file_steps_read_and_write_native_paths() {
    fn collect(value: &Value, steps: &mut Vec<Value>) {
        if let Some(object) = value.as_object() {
            if object.get("Disabled") == Some(&Value::Bool(true)) {
                return;
            }
            if matches!(
                object.get("StepRunnerKey").and_then(Value::as_str),
                Some("sys:readFile" | "sys:WriteTextFile")
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
    let corpus = std::env::var("QUICKER_COMPAT_CORPUS").expect("Set QUICKER_COMPAT_CORPUS");
    let doc: Value =
        serde_json::from_slice(&fs::read(Path::new(&corpus).join("opencc.json")).unwrap()).unwrap();
    let data: Value = serde_json::from_str(doc["Data"].as_str().unwrap()).unwrap();
    let mut steps = Vec::new();
    collect(&data["Steps"], &mut steps);
    assert_eq!(steps.len(), 12);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input 中文.txt");
    let output = dir.path().join("output.txt");
    let saved = dir.path().join("saved.txt");
    fs::write(&path, "输入\r\n").unwrap();
    fs::write(&output, "輸出\n").unwrap();
    let mut rt = runtime();
    for (key, value) in [
        ("path", &path),
        ("output路径", &output),
        ("另存为路径", &saved),
        ("build目录", &dir.path().to_path_buf()),
    ] {
        rt.vars.insert(key.into(), json!(value));
    }
    rt.vars.insert("Input".into(), json!("输入\r\n"));
    rt.vars.insert("Output".into(), json!("輸出\n"));
    let mut executed = 0;
    let mut rejected = 0;
    for raw in steps {
        let s: QuickerPluginStepDocument = serde_json::from_value(raw.clone()).unwrap();
        if raw["InputParams"]["filePath"]["Value"]
            .as_str()
            .is_some_and(|v| v.contains('\\'))
        {
            assert!(rt.run_step(&s).unwrap_err().contains("native path"));
            rejected += 1;
        } else {
            rt.run_step(&s).unwrap();
            executed += 1;
        }
    }
    assert_eq!((executed, rejected), (10, 2));
    assert_eq!(fs::read_to_string(path).unwrap(), "输入\r\n");
    assert_eq!(fs::read_to_string(saved).unwrap(), "輸出\n");
    assert_eq!(rt.vars["Input"], "输入\r\n");
    assert_eq!(rt.vars["Output"], "輸出\n");
    eprintln!("OpenCC: 10 unchanged file steps executed with native path variables; 2 Windows path steps rejected");
}
