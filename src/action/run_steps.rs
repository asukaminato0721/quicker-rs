use super::*;

/// Parse the Windows CRT argument convention used by imported Quicker actions.
/// Shell operators and single quotes have no special meaning here.
pub(super) fn parse_arguments(input: &str) -> Result<Vec<String>, String> {
    if input.contains('\0') {
        return Err("Program arguments contain a NUL character".into());
    }
    let chars: Vec<char> = input.chars().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && matches!(chars[i], ' ' | '\t') {
            i += 1;
        }
        if i == chars.len() {
            break;
        }
        let mut argument = String::new();
        let mut quoted = false;
        while i < chars.len() && (quoted || !matches!(chars[i], ' ' | '\t')) {
            let mut slashes = 0;
            while i < chars.len() && chars[i] == '\\' {
                slashes += 1;
                i += 1;
            }
            if i < chars.len() && chars[i] == '"' {
                argument.extend(std::iter::repeat_n('\\', slashes / 2));
                if slashes % 2 == 1 {
                    argument.push('"');
                } else if quoted && chars.get(i + 1) == Some(&'"') {
                    argument.push('"');
                    i += 1;
                } else {
                    quoted = !quoted;
                }
                i += 1;
            } else {
                argument.extend(std::iter::repeat_n('\\', slashes));
                if i < chars.len() && (quoted || !matches!(chars[i], ' ' | '\t')) {
                    argument.push(chars[i]);
                    i += 1;
                }
            }
        }
        result.push(argument);
    }
    Ok(result)
}

pub(super) fn environment(input: &str) -> Result<Vec<(String, String)>, String> {
    let mut result = Vec::new();
    for line in input
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with("//"))
    {
        let (key, value) = line
            .split_once('=')
            .ok_or("Environment entry requires NAME=value")?;
        let key = key.trim();
        if key.is_empty() || key.contains('\0') || value.contains('\0') {
            return Err("Environment entry has an invalid name or value".into());
        }
        result.push((key.into(), value.into()));
    }
    Ok(result)
}

#[derive(Default)]
struct RunResult {
    pid: u32,
    handle: u32,
    title: String,
    stdout: String,
    stderr: String,
    exit_code: i32,
}

impl QuickerRuntime {
    pub(super) fn run_program_step(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        let stop_on_failure = self
            .input_value(&step.input_params, "stopIfFail")?
            .is_none_or(|v| truthy(Some(&v)));
        let result = self.start_program_step(step);
        ensure_not_cancelled(self.control.as_ref())?;
        self.assign_output(
            &step.output_params,
            "isSuccess",
            Value::Bool(result.is_ok()),
        )?;
        self.assign_output(
            &step.output_params,
            "errMessage",
            Value::String(result.as_ref().err().cloned().unwrap_or_default()),
        )?;
        let error = result.as_ref().err().cloned();
        let result = result.unwrap_or_default();
        self.assign_output(&step.output_params, "pid", Value::from(result.pid))?;
        self.assign_output(
            &step.output_params,
            "mainWinHandle",
            Value::from(result.handle),
        )?;
        self.assign_output(
            &step.output_params,
            "mainWinTitle",
            Value::String(result.title),
        )?;
        self.assign_output(
            &step.output_params,
            "stdout",
            Value::String(if result.stdout.is_empty() {
                result.stderr.clone()
            } else {
                result.stdout.clone()
            }),
        )?;
        self.assign_output(
            &step.output_params,
            "stdoutOnly",
            Value::String(result.stdout),
        )?;
        self.assign_output(&step.output_params, "stderr", Value::String(result.stderr))?;
        self.assign_output(
            &step.output_params,
            "exitCode",
            Value::from(result.exit_code),
        )?;
        if stop_on_failure {
            if let Some(error) = error {
                return Err(error);
            }
        }
        Ok(StepFlow::Continue)
    }

    #[cfg(target_os = "linux")]
    fn start_program_step(&self, step: &QuickerPluginStepDocument) -> Result<RunResult, String> {
        let text = |key| {
            self.input_string_opt(&step.input_params, key)
                .map(|v| v.unwrap_or_default())
        };
        for key in ["runas", "waitInputIdle"] {
            if self.input_bool(&step.input_params, key)? {
                return Err(format!("Run option {key} is not supported on Linux"));
            }
        }
        for key in ["username", "password"] {
            if !text(key)?.is_empty() {
                return Err("Running as another Windows account is not supported on Linux".into());
            }
        }
        if !matches!(text("windowStyle")?.as_str(), "" | "0") {
            return Err("Run windowStyle requires normal (0) on Linux".into());
        }
        if !matches!(text("outputEncoding")?.as_str(), "" | "oem" | "utf8") {
            return Err("Run outputEncoding must be utf8 or oem".into());
        }
        let env = environment(&text("envVariables")?)?;
        let expand = |text: String| -> Result<String, String> {
            let text = if text.contains("{cliptext}") {
                text.replace("{cliptext}", &read_clipboard_text()?)
            } else {
                text
            };
            Ok(expand_environment(&text))
        };
        let path = expand(text("path")?)?;
        if path.trim().is_empty() {
            return Err("Run path is empty".into());
        }
        let args = parse_arguments(&expand(text("arg")?)?)?;
        let alternatives = expand(text("alternativePath")?)?;
        let path_env = env
            .iter()
            .rev()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| std::ffi::OsString::from(v))
            .or_else(|| std::env::var_os("PATH"));
        let target = resolve_target(&path, &alternatives, path_env.as_deref())?;
        let capture = ["stdout", "stdoutOnly", "stderr"]
            .iter()
            .any(|k| output_var_name(&step.output_params, k).is_some());
        let wait = capture
            || self.input_bool(&step.input_params, "waitExit")?
            || output_var_name(&step.output_params, "exitCode").is_some();
        let working = expand(
            self.input_string_opt(&step.input_params, "setWorkingDir")?
                .unwrap_or_else(|| "1".into()),
        )?;
        let activate = self.input_bool(&step.input_params, "activateWindowIfRunning")?;
        let hotkey = text("activateWindowHotkey")?;
        if activate && !hotkey.is_empty() {
            window_steps::activation_hotkey(&hotkey)?;
        }
        let want_window = ["mainWinHandle", "mainWinTitle"]
            .iter()
            .any(|k| output_var_name(&step.output_params, k).is_some());
        match target {
            Target::Open(path) => {
                if !args.is_empty()
                    || wait
                    || activate
                    || want_window
                    || output_var_name(&step.output_params, "pid").is_some()
                {
                    return Err("Document and URI handlers do not expose program arguments or process outputs".into());
                }
                let mut last_error = "No document handler is available".to_string();
                for mut command in open::commands(path) {
                    command.envs(env.clone());
                    if !matches!(working.as_str(), "" | "0" | "1") {
                        command.current_dir(&working);
                    }
                    match crate::process::status(command, self.control.as_ref(), "document handler")
                    {
                        Ok(status) if status.success() => return Ok(RunResult::default()),
                        Ok(status) => last_error = format!("Document handler failed: {status}"),
                        Err(error) => last_error = error,
                    }
                    ensure_not_cancelled(self.control.as_ref())?;
                }
                Err(last_error)
            }
            Target::Program(path, mut inline_args) => {
                if activate {
                    let query = crate::x11::WindowQuery::new(&path.to_string_lossy(), "", "")?;
                    if let Some(window) = query.find()? {
                        if wait {
                            return Err(
                                "Cannot capture or wait for an already running program".into()
                            );
                        }
                        let control = self.control.clone().unwrap_or_default();
                        crate::x11::restore_focus(&window.process, &control)?;
                        return window_result(window);
                    }
                    if query.process_running()? && !hotkey.is_empty() {
                        if wait {
                            return Err(
                                "Cannot capture or wait for an already running program".into()
                            );
                        }
                        let mut activation = step.clone();
                        activation.input_params = serde_json::json!({"process":{"Value":path.to_string_lossy()},"hotkey":{"Value":hotkey}}).as_object().unwrap().clone();
                        let (pid, handle, title) = self.activate_window(&activation)?;
                        return Ok(RunResult {
                            pid,
                            handle,
                            title,
                            ..Default::default()
                        });
                    }
                }
                if want_window && crate::x11::is_wayland() {
                    return Err("Program window outputs require the X11 backend".into());
                }
                if want_window {
                    // Check the backend before starting an application.
                    crate::x11::WindowQuery::new("0", "", "")?.find()?;
                }
                inline_args.extend(args);
                let mut command = Command::new(&path);
                command.args(inline_args).envs(env);
                match working.as_str() {
                    "" | "0" => {}
                    "1" => {
                        if let Some(parent) = path.parent() {
                            command.current_dir(parent);
                        }
                    }
                    other => {
                        command.current_dir(other);
                    }
                }
                let mut result = RunResult::default();
                if capture {
                    let (pid, output) =
                        crate::process::output_with_pid(command, self.control.as_ref(), "program")?;
                    result.pid = pid;
                    result.exit_code = exit_code(output.status);
                    result.stdout = String::from_utf8(output.stdout)
                        .map_err(|_| "Program stdout is not UTF-8")?;
                    result.stderr = String::from_utf8(output.stderr)
                        .map_err(|_| "Program stderr is not UTF-8")?;
                } else if wait {
                    let (pid, status) =
                        crate::process::status_with_pid(command, self.control.as_ref(), "program")?;
                    result.pid = pid;
                    result.exit_code = exit_code(status);
                } else {
                    result.pid = crate::process::detached(command, self.control.as_ref())?;
                    if want_window {
                        if let Some(window) =
                            crate::x11::WindowQuery::new(&result.pid.to_string(), "", "")?.find()?
                        {
                            result = window_result(window)?;
                        }
                    }
                }
                Ok(result)
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn start_program_step(&self, _: &QuickerPluginStepDocument) -> Result<RunResult, String> {
        Err("The run module requires the Linux application".into())
    }
}

#[cfg(target_os = "linux")]
fn exit_code(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

#[cfg(target_os = "linux")]
fn window_result(window: crate::x11::WindowMatch) -> Result<RunResult, String> {
    Ok(RunResult {
        pid: window.process.process_id,
        handle: window
            .process
            .window_id
            .parse()
            .map_err(|_| "Invalid window ID")?,
        title: window.title,
        ..Default::default()
    })
}

#[cfg(target_os = "linux")]
fn expand_environment(text: &str) -> String {
    let mut result = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        result.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest[1..].find('%').map(|n| n + 1) else {
            break;
        };
        result.push_str(&std::env::var(&rest[1..end]).unwrap_or_else(|_| rest[..=end].into()));
        rest = &rest[end + 1..];
    }
    result.push_str(rest);
    result
}

#[cfg(target_os = "linux")]
enum Target {
    Program(std::path::PathBuf, Vec<String>),
    Open(String),
}

#[cfg(target_os = "linux")]
fn resolve_target(
    path: &str,
    alternatives: &str,
    path_env: Option<&std::ffi::OsStr>,
) -> Result<Target, String> {
    use std::os::unix::fs::PermissionsExt;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    for candidate in std::iter::once(path)
        .chain(alternatives.lines())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let candidate = candidate
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .filter(|s| !s.contains('"'))
            .unwrap_or(candidate);
        if let Some((scheme, _)) = candidate.split_once(':') {
            if scheme.len() > 1
                && scheme.chars().enumerate().all(|(i, c)| {
                    c.is_ascii_alphabetic()
                        || (i > 0 && (c.is_ascii_digit() || matches!(c, '+' | '-' | '.')))
                })
            {
                if !matches!(
                    scheme.to_ascii_lowercase().as_str(),
                    "shell" | "ms-settings" | "storeapp" | "quicker"
                ) {
                    return Ok(Target::Open(candidate.into()));
                }
                continue;
            }
        }
        let file = cwd.join(candidate);
        if let Ok(metadata) = file.metadata() {
            if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
                return Ok(Target::Program(file, vec![]));
            }
            return Ok(Target::Open(file.to_string_lossy().into()));
        }
        if let Ok(executable) = which::which_in(candidate, path_env, &cwd) {
            return Ok(Target::Program(executable, vec![]));
        }
        let mut parts = parse_arguments(candidate)?;
        if parts.len() > 1 {
            if let Ok(executable) = which::which_in(&parts[0], path_env, &cwd) {
                parts.remove(0);
                return Ok(Target::Program(executable, parts));
            }
        }
    }
    Err("Run target was not found. Provide an installed Linux program or an existing file.".into())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn step(path: &Path, args: &str) -> QuickerPluginStepDocument {
        serde_json::from_value(json!({"StepRunnerKey":"sys:run", "InputParams":{
            "path":{"Value":path.to_string_lossy()}, "arg":{"Value":args}
        }, "OutputParams":{"pid":"pid","stdout":"combined","stdoutOnly":"out","stderr":"err","exitCode":"code","isSuccess":"ok","errMessage":"error"}})).unwrap()
    }

    fn runtime() -> QuickerRuntime {
        QuickerRuntime::new(
            &serde_json::from_value(json!({})).unwrap(),
            "run-tests".into(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn arguments_follow_windows_quotes_without_shell_expansion() {
        assert_eq!(
            parse_arguments(r#""a b c" d e"#).unwrap(),
            ["a b c", "d", "e"]
        );
        assert_eq!(
            parse_arguments(r#""ab\"c" "\\" d"#).unwrap(),
            ["ab\"c", "\\", "d"]
        );
        assert_eq!(
            parse_arguments(r#"a\\\b d"e f"g h"#).unwrap(),
            [r"a\\\b", "de fg", "h"]
        );
        assert_eq!(
            parse_arguments(r#"a\\\"b c d"#).unwrap(),
            [r#"a\"b"#, "c", "d"]
        );
        assert_eq!(
            parse_arguments(r#"a\\\\"b c" d e"#).unwrap(),
            [r"a\\b c", "d", "e"]
        );
        assert_eq!(parse_arguments(r#"a"b"" c d"#).unwrap(), ["ab\" c d"]);
        assert_eq!(
            parse_arguments(r#""" "中文 空格" $HOME *.txt ';'"#).unwrap(),
            ["", "中文 空格", "$HOME", "*.txt", "';'"]
        );
        assert!(parse_arguments("bad\0argument").is_err());
    }

    #[test]
    fn run_transfers_arguments_environment_directory_and_nonzero_exit() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "program with spaces", "printf '<%s>\\n' \"$@\"; printf 'env=%s\\ncwd=%s\\n' \"$QUICKER_RUN_TEST\" \"$PWD\"; printf error >&2; exit 7");
        let mut step = step(&program, r#""中文 空格" "" "$(touch forbidden)" "a\"b""#);
        step.input_params.insert(
            "envVariables".into(),
            json!({"Value":"// comment\nQUICKER_RUN_TEST=first\nQUICKER_RUN_TEST=value=tail"}),
        );
        let mut runtime = runtime();
        runtime.run_step(&step).unwrap();
        assert_eq!(runtime.vars["ok"], json!(true));
        assert_eq!(runtime.vars["code"], json!(7));
        assert!(runtime.vars["pid"].as_u64().unwrap() > 0);
        assert_eq!(
            runtime.vars["out"],
            json!(format!(
                "<中文 空格>\n<>\n<$(touch forbidden)>\n<a\"b>\nenv=value=tail\ncwd={}\n",
                dir.path().display()
            ))
        );
        assert_eq!(runtime.vars["combined"], runtime.vars["out"]);
        assert_eq!(runtime.vars["err"], json!("error"));
        assert!(!dir.path().join("forbidden").exists());
    }

    #[test]
    fn run_uses_alternative_paths_and_stderr_fallback_and_clears_failures() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "fallback", "printf fallback >&2");
        let mut step = step(Path::new("C:\\missing\\program.exe"), "");
        step.input_params.insert(
            "alternativePath".into(),
            json!({"Value":format!("/missing\n{}", program.display())}),
        );
        step.input_params.insert(
            "setWorkingDir".into(),
            json!({"Value":dir.path().to_string_lossy()}),
        );
        let mut runtime = runtime();
        runtime.run_step(&step).unwrap();
        assert_eq!(runtime.vars["combined"], json!("fallback"));
        assert_eq!(runtime.vars["out"], json!(""));
        step.input_params
            .insert("stopIfFail".into(), json!({"Value":"0"}));
        step.input_params
            .insert("envVariables".into(), json!({"Value":"malformed"}));
        runtime.run_step(&step).unwrap();
        assert_eq!(runtime.vars["ok"], json!(false));
        assert_eq!(runtime.vars["pid"], json!(0));
        assert_eq!(runtime.vars["combined"], json!(""));
        assert!(runtime.vars["error"]
            .as_str()
            .unwrap()
            .contains("NAME=value"));
    }

    #[test]
    fn run_capture_cancels_descendants_even_when_failure_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("late-write");
        let program = script(
            dir.path(),
            "waiter",
            "(sleep 0.4; printf wrong > late-write) & wait",
        );
        let mut step = step(&program, "");
        step.input_params
            .insert("stopIfFail".into(), json!({"Value":"0"}));
        let control = ActionExecutionControl::new();
        let cancel = control.clone();
        let worker = thread::spawn(move || {
            let mut runtime = runtime();
            runtime.control = Some(control);
            runtime.run_step(&step)
        });
        thread::sleep(Duration::from_millis(70));
        cancel.cancel();
        assert!(worker.join().unwrap().unwrap_err().contains("cancelled"));
        thread::sleep(Duration::from_millis(450));
        assert!(!marker.exists());
    }

    #[test]
    fn run_detached_returns_pid_and_wait_exit_keeps_order() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "writer", "sleep 0.15; printf done > complete");
        let mut step = step(&program, "");
        step.output_params = json!({"pid":"pid"}).as_object().unwrap().clone();
        let mut runtime = runtime();
        runtime.run_step(&step).unwrap();
        assert!(!dir.path().join("complete").exists());
        let pid = runtime.vars["pid"].as_u64().unwrap();
        assert!(pid > 0);
        step.input_params
            .insert("waitExit".into(), json!({"Value":"1"}));
        runtime.run_step(&step).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("complete")).unwrap(),
            "done"
        );
    }

    #[test]
    fn run_rejects_unsupported_options_before_starting() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "must-not-start", "printf wrong > started");
        for (key, value) in [
            ("runas", "1"),
            ("waitInputIdle", "1"),
            ("windowStyle", "2"),
            ("username", "other"),
            ("outputEncoding", "unknown"),
        ] {
            let mut step = step(&program, "");
            step.input_params.insert(key.into(), json!({"Value":value}));
            assert!(runtime().run_step(&step).is_err());
        }
        assert!(!dir.path().join("started").exists());
    }

    #[test]
    fn run_document_and_uri_use_handler_without_shell_interpolation() {
        let dir = tempfile::tempdir().unwrap();
        script(
            dir.path(),
            "xdg-open",
            "printf '%s' \"$1\" > \"$QUICKER_HANDLER_RESULT\"",
        );
        let result = dir.path().join("received");
        let document = dir.path().join("document with spaces.txt");
        fs::write(&document, "document").unwrap();
        for target in [
            document.to_string_lossy().into_owned(),
            "https://example.invalid/a?q=$(literal)".into(),
        ] {
            let mut step = step(Path::new(&target), "");
            step.output_params = json!({"isSuccess":"ok"}).as_object().unwrap().clone();
            step.input_params.insert("envVariables".into(), json!({"Value":format!("PATH={}\nQUICKER_HANDLER_RESULT={}",dir.path().display(),result.display())}));
            let mut runtime = runtime();
            runtime.run_step(&step).unwrap();
            assert_eq!(runtime.vars["ok"], json!(true));
            assert_eq!(fs::read_to_string(&result).unwrap(), target);
        }
    }

    #[test]
    fn run_resolves_quoted_inline_program_and_child_path() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "quoted program", "printf '%s' \"$1\"");
        let inline = format!("\"{}\" \"a b\"", program.display());
        let mut runtime = runtime();
        runtime.run_step(&step(Path::new(&inline), "")).unwrap();
        assert_eq!(runtime.vars["out"], json!("a b"));
        let mut step = step(Path::new("quoted program"), "done");
        step.input_params.insert(
            "envVariables".into(),
            json!({"Value":format!("PATH={}",dir.path().display())}),
        );
        runtime.run_step(&step).unwrap();
        assert_eq!(runtime.vars["out"], json!("done"));
    }
}
