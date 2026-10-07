use super::*;
use serde_json::json;
use std::time::Instant;

fn runtime() -> QuickerRuntime {
    QuickerRuntime::new(
        &serde_json::from_value(json!({})).unwrap(),
        "scripts".into(),
        None,
    )
    .unwrap()
}

fn step(script: &str, directory: &Path) -> (QuickerRuntime, QuickerPluginStepDocument) {
    let mut rt = runtime();
    // Variable bindings pass script source without Quicker text interpolation.
    rt.vars.insert("code".into(), json!(script));
    let step = serde_json::from_value(json!({"StepRunnerKey":"sys:runScript", "InputParams": {
        "type":{"Value":"CUSTOM"}, "runner":{"Value":"/bin/sh"}, "ext":{"Value":".sh"},
        "script":{"VarKey":"code"}, "encoding":{"Value":"UTF8-NOBOM"},
        "workingDir":{"Value":directory}, "waitToExit":{"Value":false}
    }, "OutputParams":{"stdout":"out","stdoutOnly":"raw","stderr":"err","isSuccess":"ok","errMessage":"error"}})).unwrap();
    (rt, step)
}

fn wait_for(mut ready: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ready() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "Script did not reach the expected state"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn capture_waits_preserves_text_and_accepts_nonzero_exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rt, step) = step(
        "sleep 0.05; printf '中文  \\r\\n'; printf 'error\\n' >&2; exit 7",
        dir.path(),
    );
    rt.run_step(&step).unwrap();
    assert_eq!(rt.vars["out"], "中文  \r\n");
    assert_eq!(rt.vars["raw"], "中文  \r\n");
    assert_eq!(rt.vars["err"], "error\n");
    assert_eq!(rt.vars["ok"], true);
    rt.vars
        .insert("code".into(), json!("printf 'only error' >&2"));
    rt.run_step(&step).unwrap();
    assert_eq!(rt.vars["out"], "only error");
    assert_eq!(rt.vars["raw"], "");
}

#[test]
fn arguments_are_literal_and_script_file_is_removed_after_capture() {
    let dir = tempfile::Builder::new()
        .prefix("script 中文 ")
        .tempdir()
        .unwrap();
    let (mut rt, mut step) = step(
        "printf '%s\\n' \"$0\" \"$PWD\" \"$1\" \"$2\" \"$3\"",
        dir.path(),
    );
    step.input_params.insert(
        "argTemplate".into(),
        json!({"Value":"\"%FILE%\" \"first arg\""}),
    );
    step.input_params.insert(
        "scriptParams".into(),
        json!({"Value":"\"中文 \\\"quote\\\"\" \"; touch forbidden\""}),
    );
    rt.run_step(&step).unwrap();
    let output = rt.vars["raw"].as_str().unwrap();
    let lines: Vec<_> = output.lines().collect();
    assert!(!Path::new(lines[0]).exists());
    assert!(!Path::new(lines[0]).parent().unwrap().exists());
    assert_eq!(lines[1], dir.path().to_str().unwrap());
    assert_eq!(
        &lines[2..],
        &["first arg", "中文 \"quote\"", "; touch forbidden"]
    );
    assert!(!dir.path().join("forbidden").exists());
}

#[test]
fn script_files_use_requested_encoding_and_bom() {
    let dir = tempfile::tempdir().unwrap();
    let saved = dir.path().join("saved bytes");
    let (mut rt, mut step) = step("A中😀", dir.path());
    step.input_params
        .insert("runner".into(), json!({"Value":"/bin/cp"}));
    step.input_params.insert(
        "argTemplate".into(),
        json!({"Value":format!("\"%FILE%\" \"{}\"", saved.display())}),
    );
    for (encoding, bytes) in [
        (
            "utf-8",
            vec![239, 187, 191, 65, 228, 184, 173, 240, 159, 152, 128],
        ),
        ("UTF8-NOBOM", vec![65, 228, 184, 173, 240, 159, 152, 128]),
        ("default", vec![65, 228, 184, 173, 240, 159, 152, 128]),
        ("utf-16", vec![255, 254, 65, 0, 45, 78, 61, 216, 0, 222]),
        ("utf-16BE", vec![254, 255, 0, 65, 78, 45, 216, 61, 222, 0]),
        (
            "utf-32",
            vec![255, 254, 0, 0, 65, 0, 0, 0, 45, 78, 0, 0, 0, 246, 1, 0],
        ),
    ] {
        step.input_params
            .insert("encoding".into(), json!({"Value":encoding}));
        rt.run_step(&step).unwrap();
        assert_eq!(fs::read(&saved).unwrap(), bytes, "{encoding}");
    }
    step.input_params
        .insert("encoding".into(), json!({"Value":"us-ascii"}));
    assert!(rt.run_step(&step).unwrap_err().contains("ASCII"));
    rt.vars.insert("code".into(), json!("ASCII"));
    rt.run_step(&step).unwrap();
    assert_eq!(fs::read(saved).unwrap(), b"ASCII");
}

#[test]
fn detached_scripts_keep_their_file_until_exit_then_remove_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rt, mut step) = step("printf '%s' \"$0\" > started; while [ ! -f release ]; do sleep 0.01; done; cat \"$0\" > copied", dir.path());
    step.output_params.clear();
    rt.run_step(&step).unwrap();
    let started = dir.path().join("started");
    wait_for(|| fs::read_to_string(&started).is_ok_and(|v| !v.is_empty()));
    let script = std::path::PathBuf::from(fs::read_to_string(started).unwrap());
    assert!(script.exists());
    fs::write(dir.path().join("release"), "").unwrap();
    wait_for(|| !script.parent().unwrap().exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("copied")).unwrap(),
        rt.vars["code"].as_str().unwrap()
    );
}

#[test]
fn explicit_wait_and_failure_policy_match_process_creation_result() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rt, mut step) = step("sleep 0.05; touch finished; exit 5", dir.path());
    step.output_params.remove("stdout");
    step.output_params.remove("stdoutOnly");
    step.output_params.remove("stderr");
    step.input_params
        .insert("waitToExit".into(), json!({"Value":true}));
    rt.run_step(&step).unwrap();
    assert!(dir.path().join("finished").exists());
    assert_eq!(rt.vars["ok"], true);
    step.input_params.insert(
        "runner".into(),
        json!({"Value":"/missing-quicker-test-interpreter"}),
    );
    assert!(rt.run_step(&step).unwrap_err().contains("not available"));
    step.input_params
        .insert("stopIfFail".into(), json!({"Value":false}));
    rt.run_step(&step).unwrap();
    assert_eq!(rt.vars["ok"], false);
    assert!(rt.vars["error"].as_str().unwrap().contains("not available"));
}

#[test]
fn cancellation_kills_descendants_and_removes_script_even_when_failure_can_continue() {
    let dir = tempfile::tempdir().unwrap();
    for capture in [false, true] {
        let (mut rt, mut step) = step(
            "(sleep 0.4; touch forbidden) & printf '%s' \"$0\" > started; wait",
            dir.path(),
        );
        if !capture {
            step.output_params.clear();
        }
        step.input_params
            .insert("waitToExit".into(), json!({"Value":true}));
        step.input_params
            .insert("stopIfFail".into(), json!({"Value":false}));
        let control = ActionExecutionControl::new();
        rt.control = Some(control.clone());
        let started = dir.path().join("started");
        let signal = thread::spawn(move || {
            wait_for(|| fs::read_to_string(&started).is_ok_and(|s| !s.is_empty()));
            let script = fs::read_to_string(&started).unwrap();
            control.cancel();
            script
        });
        assert!(rt.run_step(&step).unwrap_err().contains("cancelled"));
        let script = signal.join().unwrap();
        assert!(!Path::new(&script).parent().unwrap().exists());
        thread::sleep(Duration::from_millis(450));
        assert!(!dir.path().join("forbidden").exists());
        fs::remove_file(dir.path().join("started")).unwrap();
    }
}

#[test]
fn rejects_windows_modes_invalid_options_and_pre_cancelled_execution() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rt, step) = step("touch forbidden", dir.path());
    for (key, value) in [
        ("type", "CMD_H"),
        ("type", "CMD_K"),
        ("type", "CMD_C"),
        ("type", "BAT"),
        ("type", "CMD_F"),
        ("type", "AHK"),
        ("type", "unknown"),
        ("ext", "../sh"),
        ("runner", ""),
        ("runner", "python.exe"),
        ("encoding", "utf-7"),
        ("runAsAdmin", "true"),
        ("outputEncoding", "gbk"),
        ("workingDir", "C:\\Temp"),
        ("argTemplate", "\0"),
    ] {
        let mut s = step.clone();
        s.input_params.insert(key.into(), json!({"Value":value}));
        assert!(rt.run_step(&s).is_err(), "{key}: {value}");
    }
    let control = ActionExecutionControl::new();
    control.cancel();
    rt.control = Some(control);
    assert!(rt.run_step(&step).unwrap_err().contains("cancelled"));
    assert!(!dir.path().join("forbidden").exists());
}

#[test]
fn script_report_preserves_windows_blockers_and_checks_custom_parameters() {
    let report = |params: Value| {
        compatibility::inspect(
            &json!({"ActionType":24,"Data":json!({
        "Steps":[{"StepRunnerKey":"sys:runScript","InputParams":params}]
    }).to_string()})
            .to_string(),
        )
    };
    let valid =
        json!({"type":{"Value":"CUSTOM"},"ext":{"Value":".sh"},"runner":{"Value":"/bin/sh"}});
    assert_eq!(
        report(valid.clone())["runtime"]["status"],
        "needs_runtime_validation"
    );
    for (key, value) in [
        ("type", "CMD_H"),
        ("runner", ""),
        ("ext", "../sh"),
        ("encoding", "gbk"),
        ("runAsAdmin", "true"),
    ] {
        let mut params = valid.clone();
        params[key] = json!({"Value":value});
        assert_eq!(report(params)["runtime"]["status"], "blocked");
    }
    let ps = report(json!({"type":{"Value":"PS"}}));
    assert!(ps["runtime"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["severity"] != "blocker"));
    let dynamic = report(json!({"type":{"VarKey":"type"}}));
    assert!(dynamic["runtime"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["code"] == "dynamic_option_requires_validation"));
}

#[test]
#[ignore = "requires the downloaded OpenCC export in QUICKER_COMPAT_CORPUS"]
fn downloaded_opencc_cmd_scripts_remain_explicit_windows_blockers() {
    let corpus = std::env::var("QUICKER_COMPAT_CORPUS").unwrap();
    let source = fs::read_to_string(Path::new(&corpus).join("opencc.json")).unwrap();
    let document: Value = serde_json::from_str(&source).unwrap();
    let data: Value = serde_json::from_str(document["Data"].as_str().unwrap()).unwrap();
    let mut pending = vec![&data];
    let mut count = 0;
    while let Some(value) = pending.pop() {
        if value["StepRunnerKey"] == "sys:runScript" {
            let step = serde_json::from_value(value.clone()).unwrap();
            assert!(runtime()
                .run_step(&step)
                .unwrap_err()
                .contains("Windows interpreter"));
            count += 1;
        }
        if let Some(object) = value.as_object() {
            pending.extend(object.values());
        }
        if let Some(array) = value.as_array() {
            pending.extend(array);
        }
    }
    assert_eq!(count, 5);
    let report = compatibility::inspect(&source);
    assert_eq!(
        report["runtime"]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["code"] == "script_type_requires_linux_replacement")
            .count(),
        5
    );
}

#[test]
#[ignore = "requires PowerShell pwsh on PATH"]
fn powershell_script_uses_native_interpreter_and_script_parameters() {
    let dir = tempfile::Builder::new()
        .prefix("pwsh 中文 ")
        .tempdir()
        .unwrap();
    let (mut rt, mut step) = step(
        "param([string]$name)\n[Console]::Out.Write(\"Hello $name\"); [Console]::Error.Write('error'); exit 7",
        dir.path(),
    );
    step.input_params
        .insert("type".into(), json!({"Value":"PS"}));
    step.input_params
        .insert("encoding".into(), json!({"Value":"utf-8"}));
    step.input_params
        .insert("scriptParams".into(), json!({"Value":"\"中文 name\""}));
    // PS ignores custom interpreter and custom file-extension settings.
    step.input_params
        .insert("runner".into(), json!({"Value":"/missing"}));
    step.input_params
        .insert("ext".into(), json!({"Value":"../bad"}));
    rt.run_step(&step).unwrap();
    assert_eq!(rt.vars["out"], "Hello 中文 name");
    assert_eq!(rt.vars["err"], "error");
    assert_eq!(rt.vars["ok"], true);
}
