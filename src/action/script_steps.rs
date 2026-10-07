//! Native interpreters for the Quicker sys:runScript module.
use super::*;

#[cfg(all(test, target_os = "linux"))]
mod tests;

pub(super) fn validate_option(key: &str, value: &str) -> bool {
    match key {
        "type" => matches!(value.to_ascii_uppercase().as_str(), "CUSTOM" | "PS"),
        "encoding" => script_encoding(value).is_ok(),
        "outputEncoding" => matches!(value, "" | "utf8" | "oem"),
        "ext" => {
            value.starts_with('.')
                && value.len() <= 32
                && value[1..]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
                && value.len() > 1
        }
        "runner" => {
            !value.trim().is_empty()
                && !file_steps::windows_path(value)
                && !value.to_ascii_lowercase().ends_with(".exe")
        }
        "workingDir" => !file_steps::windows_path(value),
        "argTemplate" | "scriptParams" => run_steps::parse_arguments(value).is_ok(),
        _ => true,
    }
}

fn script_encoding(name: &str) -> Result<(file_steps::Encoding, bool), String> {
    // Linux has no Windows ANSI code page. Default means UTF-8 without a BOM.
    match name.to_ascii_lowercase().as_str() {
        "" | "default" | "utf8-nobom" => Ok((file_steps::Encoding::Utf8, false)),
        _ => Ok((file_steps::Encoding::parse(name)?, true)),
    }
}

impl QuickerRuntime {
    pub(super) fn run_script_step(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        let stop = self
            .input_value(&step.input_params, "stopIfFail")?
            .is_none_or(|value| truthy(Some(&value)));
        let result = self.start_script(step);
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
        match result {
            Ok(Some((stdout, stderr))) => {
                self.assign_output(
                    &step.output_params,
                    "stdout",
                    Value::String(if stdout.is_empty() {
                        stderr.clone()
                    } else {
                        stdout.clone()
                    }),
                )?;
                self.assign_output(&step.output_params, "stderr", Value::String(stderr))?;
                self.assign_output(&step.output_params, "stdoutOnly", Value::String(stdout))?;
            }
            Err(error) if stop => return Err(error),
            _ => {}
        }
        Ok(StepFlow::Continue)
    }

    #[cfg(target_os = "linux")]
    fn start_script(
        &self,
        step: &QuickerPluginStepDocument,
    ) -> Result<Option<(String, String)>, String> {
        use std::io::Write;
        use std::process::Stdio;

        ensure_not_cancelled(self.control.as_ref())?;
        let text = |key: &str, default: &str| -> Result<String, String> {
            Ok(self
                .input_string_opt(&step.input_params, key)?
                .unwrap_or_else(|| default.into()))
        };
        let mode = text("type", "CMD_K")?.to_ascii_uppercase();
        if !validate_option("type", &mode) {
            return Err(format!("Script type {mode} requires a Windows interpreter. Use CUSTOM with a Linux interpreter or PS with pwsh"));
        }
        if self.input_bool(&step.input_params, "runAsAdmin")? {
            return Err("Script runAsAdmin is not supported on Linux".into());
        }
        if !validate_option("outputEncoding", &text("outputEncoding", "oem")?) {
            return Err(
                "Script outputEncoding must be utf8 or oem. Both use UTF-8 on Linux".into(),
            );
        }
        let encoding = text("encoding", "default")?;
        let (encoding, bom) = script_encoding(&encoding)?;
        let content = text("script", "")?;
        if content.len() > 16 * 1024 * 1024 {
            return Err("Script exceeds the 16 MiB limit".into());
        }
        let bytes = encoding.encode(&content)?;
        let extension = if mode == "PS" {
            ".ps1".into()
        } else {
            text("ext", "")?
        };
        if !validate_option("ext", &extension) {
            return Err(
                "Script ext requires a dot and 1 to 31 ASCII letters, digits, or underscores"
                    .into(),
            );
        }
        let runner = if mode == "PS" {
            "pwsh".into()
        } else {
            text("runner", "")?
        };
        if !validate_option("runner", &runner) {
            return Err(
                "Script runner requires a Linux interpreter. File associations are not supported"
                    .into(),
            );
        }
        let interpreter = which::which(&runner)
            .map_err(|_| format!("Script interpreter is not available: {runner}"))?;
        let working = text("workingDir", "")?;
        if !validate_option("workingDir", &working) {
            return Err("Script workingDir requires a Linux directory".into());
        }
        let working = if working.is_empty() {
            dirs::desktop_dir()
                .filter(|p| p.is_dir())
                .or_else(dirs::home_dir)
                .ok_or("No desktop or home directory is available for scripts")?
        } else {
            std::path::PathBuf::from(working)
        };
        if !working.is_dir() {
            return Err("Script workingDir does not exist or is not a directory".into());
        }

        // A private directory prevents other users from replacing the script.
        // Keep it alive until the interpreter exits, including detached execution.
        let directory = tempfile::Builder::new()
            .prefix("quicker-script-")
            .tempdir()
            .map_err(|e| format!("Failed to create script directory: {e}"))?;
        let script = directory.path().join(format!("script{extension}"));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&script)
            .map_err(|e| format!("Failed to create script file: {e}"))?;
        file.write_all(encoding.preamble(bom))
            .and_then(|()| file.write_all(&bytes))
            .map_err(|e| format!("Failed to write script file: {e}"))?;
        drop(file);
        let mut command = Command::new(interpreter);
        if mode == "PS" {
            command
                .args(["-NoProfile", "-NonInteractive", "-File"])
                .arg(&script);
        } else {
            let template = text("argTemplate", "%FILE%")?;
            // Replace after parsing. A path with spaces remains one argument.
            // Use OsString so a non-UTF-8 temporary directory remains valid.
            for argument in run_steps::parse_arguments(&template)? {
                let mut replaced = std::ffi::OsString::new();
                for (index, part) in argument.split("%FILE%").enumerate() {
                    if index > 0 {
                        replaced.push(&script);
                    }
                    replaced.push(part);
                }
                command.arg(replaced);
            }
        }
        // The MSI appends these parameters for custom interpreters as well.
        command.args(run_steps::parse_arguments(&text("scriptParams", "")?)?);
        command.current_dir(working);
        let capture = ["stdout", "stdoutOnly", "stderr"]
            .iter()
            .any(|key| output_var_name(&step.output_params, key).is_some());
        if capture {
            let output =
                crate::process::output(command, self.control.as_ref(), "script interpreter")?;
            // Quicker reports process creation errors, not nonzero script exit codes.
            // Preserve native line endings and trailing whitespace.
            Ok(Some((
                String::from_utf8_lossy(&output.stdout).into_owned(),
                String::from_utf8_lossy(&output.stderr).into_owned(),
            )))
        } else {
            command.stdout(Stdio::null()).stderr(Stdio::null());
            if self.input_bool(&step.input_params, "waitToExit")? {
                crate::process::status(command, self.control.as_ref(), "script interpreter")?;
            } else {
                crate::process::detached_with_resource(command, self.control.as_ref(), directory)?;
            }
            Ok(None)
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn start_script(
        &self,
        _step: &QuickerPluginStepDocument,
    ) -> Result<Option<(String, String)>, String> {
        Err("Plugin scripts require the native Linux application".into())
    }
}
