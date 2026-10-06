use super::super::*;
use serde_json::json;

fn eval(expr: &str, value: Value) -> Value {
    evaluate(expr, &HashMap::from([("path".into(), value)])).unwrap()
}

#[test]
fn linux_paths_keep_dot_segments_whitespace_and_dotfile_extensions() {
    // .NET string overloads retain lexical components instead of canonicalizing.
    for (path, dir, name, stem, ext) in [
        (
            "/tmp/中文.tar.gz",
            json!("/tmp"),
            "中文.tar.gz",
            "中文.tar",
            ".gz",
        ),
        ("/tmp/.config", json!("/tmp"), ".config", "", ".config"),
        ("/tmp/file.", json!("/tmp"), "file.", "file", ""),
        ("/tmp/", json!("/tmp"), "", "", ""),
        ("file", json!(""), "file", "file", ""),
        ("", Value::Null, "", "", ""),
        ("/", Value::Null, "", "", ""),
        ("///", json!("/"), "", "", ""),
        ("/tmp//a///b", json!("/tmp/a"), "b", "b", ""),
        ("a/../b", json!("a/.."), "b", "b", ""),
        ("./file", json!("."), "file", "file", ""),
        (".", json!(""), ".", "", ""),
        ("..", json!(""), "..", ".", ""),
        (
            " a / file.txt ",
            json!(" a "),
            " file.txt ",
            " file",
            ".txt ",
        ),
        ("a\nb.txt", json!(""), "a\nb.txt", "a\nb", ".txt"),
    ] {
        for (method, expected) in [
            ("GetDirectoryName", dir),
            ("GetFileName", json!(name)),
            ("GetFileNameWithoutExtension", json!(stem)),
            ("GetExtension", json!(ext)),
            ("HasExtension", json!(!ext.is_empty())),
            ("IsPathRooted", json!(path.starts_with('/'))),
        ] {
            assert_eq!(
                eval(&format!("$= Path.{method}({{path}})"), json!(path)),
                expected,
                "{method}({path:?})"
            );
        }
    }
}

#[test]
fn null_paths_empty_roots_and_extension_replacement_use_dotnet_string_rules() {
    for method in [
        "GetDirectoryName",
        "GetFileName",
        "GetFileNameWithoutExtension",
        "GetExtension",
        "GetPathRoot",
    ] {
        assert_eq!(
            eval(&format!("$= Path.{method}(null)"), Value::Null),
            Value::Null
        );
    }
    for (expr, expected) in [
        (r#"$= Path.GetPathRoot("a")"#, json!("")),
        (r#"$= Path.GetPathRoot("/a")"#, json!("/")),
        (r#"$= Path.GetPathRoot("")"#, Value::Null),
        (r#"$= Path.ChangeExtension(null, ".txt")"#, Value::Null),
        (r#"$= Path.ChangeExtension("", ".txt")"#, json!("")),
        (
            r#"$= Path.ChangeExtension("a.tar.gz", "txt")"#,
            json!("a.tar.txt"),
        ),
        (
            r#"$= Path.ChangeExtension("a.tar.gz", "")"#,
            json!("a.tar."),
        ),
        (
            r#"$= Path.ChangeExtension("a.tar.gz", null)"#,
            json!("a.tar"),
        ),
        (
            r#"$= Path.ChangeExtension("/a.b/file", ".txt")"#,
            json!("/a.b/file.txt"),
        ),
        (
            r#"$= Path.ChangeExtension("/a.b/", "txt")"#,
            json!("/a.b/.txt"),
        ),
        (r#"$= Path.HasExtension(null)"#, json!(false)),
        (r#"$= Path.IsPathRooted(null)"#, json!(false)),
    ] {
        assert_eq!(eval(expr, Value::Null), expected, "{expr}");
    }
}

#[test]
fn combine_keeps_segments_and_later_absolute_paths_replace_earlier_parts() {
    for (expr, expected) in [
        (r#"$= Path.Combine("/tmp", "file")"#, "/tmp/file"),
        (r#"$= Path.Combine("/tmp/", "file")"#, "/tmp/file"),
        (r#"$= Path.Combine("/tmp//", "file")"#, "/tmp//file"),
        (r#"$= Path.Combine("a", "/b", "c")"#, "/b/c"),
        (r#"$= Path.Combine("a", "", "b", "c")"#, "a/b/c"),
        (r#"$= Path.Combine("a", "", "b", "c", "d")"#, "a/b/c/d"),
        (r#"$= Path.Combine("a", "../b")"#, "a/../b"),
        (r#"$= Path.Combine("", "")"#, ""),
        (r#"$= Path.Combine({path})"#, "a/b"),
        (r#"$= Path.Combine()"#, ""),
        (r#"$= Path.Combine("a")"#, "a"),
    ] {
        assert_eq!(eval(expr, json!(["a", "b"])), json!(expected), "{expr}");
    }
    assert_eq!(eval("$= Path.Combine({path})", json!([])), "");
}

#[test]
fn windows_paths_and_host_methods_require_explicit_porting() {
    for path in [
        "C:\\dir\\file",
        "C:/file",
        "C:relative",
        "\\\\server\\file",
        "a\\b",
        "a\0b",
    ] {
        assert!(evaluate(
            "$= Path.GetFileName({path})",
            &HashMap::from([("path".into(), json!(path))])
        )
        .is_err());
    }
    for expr in [
        "$= Path.GetFullPath(\"a\")",
        "$= Path.GetTempFileName()",
        "$= Path.Exists(\"a\")",
        "$= Path.GetDirectoryName()",
        "$= Path.GetFileName(1)",
        "$= Path.Combine(null, \"a\")",
        "$= Path.Combine(1)",
        "$= Path.DirectorySeparatorChar",
        "$= Path.DirectorySeparatorChar()",
        "$= System.IO.File.Delete(\"a\")",
        "$= Path.GetDirectoryName(\"a\", \"b\")",
    ] {
        assert!(evaluate(expr, &HashMap::new()).is_err(), "{expr}");
    }
    let vars = HashMap::from([("path".into(), json!("/tmp/name.txt"))]);
    let value = evaluate(
        r#"$= Path.GetDirectoryName({path}) + @"\" + Path.GetFileName({path})"#,
        &vars,
    )
    .unwrap();
    assert_eq!(value, "/tmp\\name.txt");
    assert_eq!(
        evaluate("$= false ? Path.GetFileName({missing}) : null", &vars).unwrap(),
        Value::Null
    );
}

#[test]
fn matches_recorded_dotnet_linux_results() {
    let reference: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/dotnet-linux-paths.json"
    ))
    .unwrap();
    assert_eq!(reference["runtime"], ".NET 8.0.0");
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 234);
    for case in cases {
        let expr = case["expression"].as_str().unwrap();
        assert_eq!(
            evaluate(expr, &HashMap::new()).unwrap(),
            case["expected"],
            "{expr}"
        );
    }
}

#[test]
fn checker_identifies_linux_semantics_and_windows_path_construction() {
    use crate::action::compatibility;
    for (expr, path_calls, windows_paths) in [
        (r#"$= Path.GetDirectoryName({path})"#, true, false),
        (r#"$= Path.Combine("C:/old", "file")"#, true, true),
        (
            r#"$= Path.GetDirectoryName({path}) + @"\" + "file""#,
            true,
            true,
        ),
        (r#"$= @"\d+".Length"#, false, false),
        (
            r#"$= Path.GetFileName({path}).Replace(@"\x", "")"#,
            true,
            false,
        ),
    ] {
        let features = validate(expr).unwrap();
        assert_eq!(
            (features.path_calls, features.windows_paths),
            (path_calls, windows_paths)
        );
        let doc = json!({"Title":"Paths", "ActionType":24, "Data":json!({"Steps":[{
            "StepRunnerKey":"sys:assign", "InputParams":{"input":{"Value":expr}},
            "OutputParams":{"output":"result"}}]}).to_string()})
        .to_string();
        let report = compatibility::inspect(&doc);
        let issues = report["runtime"]["issues"].as_array().unwrap();
        for (code, expected) in [
            ("path_expression_uses_linux_semantics", path_calls),
            (
                "path_expression_contains_windows_path_literal",
                windows_paths,
            ),
        ] {
            assert_eq!(
                issues.iter().any(|i| i["code"] == code),
                expected,
                "{report}"
            );
        }
        assert_eq!(report["raw_round_trip"]["status"], "pass");
        assert_eq!(report["editor_round_trip"]["status"], "pass");
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "requires the downloaded OpenCC export in QUICKER_COMPAT_CORPUS"]
fn downloaded_opencc_path_expressions_and_save_steps_execute_unchanged() {
    use crate::action::{QuickerPluginData, QuickerPluginStepDocument, QuickerRuntime};
    use std::{fs, path::Path};
    fn collect(value: &Value, result: &mut Vec<Value>) {
        if value["Disabled"] == true {
            return;
        }
        if value["StepRunnerKey"].is_string() {
            result.push(value.clone());
        }
        match value {
            Value::Object(o) => {
                for v in o.values() {
                    collect(v, result);
                }
            }
            Value::Array(a) => {
                for v in a {
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
    let typed: QuickerPluginData = serde_json::from_value(data.clone()).unwrap();
    let mut rt = QuickerRuntime::new(&typed, "opencc-paths".into(), None).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("原始.tar.txt");
    let output_dir = dir.path().join("目标目录");
    fs::create_dir(&output_dir).unwrap();
    fs::write(&original, "原始内容").unwrap();
    for (key, value) in [
        ("path", json!(original)),
        ("firstFile", json!(original)),
        ("另存为目录", json!(output_dir)),
        ("Output", json!("轉換內容\r\n")),
    ] {
        rt.vars.insert(key.into(), value);
    }
    let mut steps = Vec::new();
    collect(&data["Steps"], &mut steps);
    let writer: QuickerPluginStepDocument = serde_json::from_value(
        steps
            .iter()
            .find(|s| {
                s["StepRunnerKey"] == "sys:WriteTextFile"
                    && s["InputParams"]["filePath"]["VarKey"] == "另存为路径"
                    && s["InputParams"]["content"]["VarKey"] == "Output"
            })
            .unwrap()
            .clone(),
    )
    .unwrap();
    let mut checked_inputs = 0;
    let mut assignments = 0;
    let mut saved = 0;
    let mut windows_paths = 0;
    for raw in &steps {
        let Some(inputs) = raw["InputParams"].as_object() else {
            continue;
        };
        let Some((key, binding)) = inputs.iter().find(|(_, v)| {
            v["Value"]
                .as_str()
                .is_some_and(|s| s.starts_with("$=") && s.contains("Path."))
        }) else {
            continue;
        };
        let step: QuickerPluginStepDocument = serde_json::from_value(raw.clone()).unwrap();
        if step.step_runner_key == "sys:assign" {
            rt.run_step(&step).unwrap();
            assignments += 1;
            if raw["OutputParams"]["output"] == "初始文件名" {
                assert_eq!(rt.vars["初始文件名"], "原始.tar_new.txt");
            } else if binding["Value"].as_str().unwrap().contains("@\"\\\"") {
                assert_eq!(
                    rt.vars["另存为路径"],
                    json!(format!("{}\\原始.tar_new.txt", dir.path().display()))
                );
                assert!(rt.run_step(&writer).unwrap_err().contains("native path"));
                windows_paths += 1;
            } else {
                let destination = output_dir.join("原始.tar_new.txt");
                assert_eq!(rt.vars["另存为路径"], json!(destination));
                rt.run_step(&writer).unwrap();
                assert_eq!(fs::read_to_string(destination).unwrap(), "轉換內容\r\n");
                saved += 1;
            }
        } else {
            // Validate original folder-dialog and launcher inputs without opening
            // a dialog or launching a desktop file manager in this unit test.
            rt.vars.insert(
                "另存为路径".into(),
                json!(output_dir.join("原始.tar_new.txt")),
            );
            let expected = if step.step_runner_key == "sys:selectFolder" {
                dir.path()
            } else {
                output_dir.as_path()
            };
            assert_eq!(
                rt.input_value(&step.input_params, key).unwrap().unwrap(),
                json!(expected)
            );
            checked_inputs += 1;
        }
    }
    assert_eq!(
        (assignments, checked_inputs, saved, windows_paths),
        (6, 2, 2, 2)
    );
    assert_eq!(fs::read_to_string(original).unwrap(), "原始内容");
    eprintln!("OpenCC: 6 original path assignments, 2 original input expressions, and 2 original writes passed; 2 Windows constructions retained and rejected for writing");
}
