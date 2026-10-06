use super::*;

/// A single .NET SendKeys chord, as used by tray-window activation hotkeys.
pub(super) fn activation_hotkey(text: &str) -> Result<(Vec<String>, String), String> {
    let mut rest = text;
    let mut modifiers = Vec::new();
    while let Some(byte) = rest.as_bytes().first() {
        let modifier = match byte {
            b'^' => "ctrl",
            b'%' => "alt",
            b'+' => "shift",
            _ => break,
        };
        if !modifiers.iter().any(|m| m == modifier) {
            modifiers.push(modifier.into())
        }
        rest = &rest[1..];
    }
    let key = if rest.starts_with('{') && rest.ends_with('}') {
        let name = &rest[1..rest.len() - 1];
        match name.to_ascii_uppercase().as_str() {
            "ENTER" => "Return".into(),
            "ESC" | "ESCAPE" => "Escape".into(),
            "TAB" => "Tab".into(),
            "SPACE" => "space".into(),
            "HOME" => "Home".into(),
            "END" => "End".into(),
            "PGUP" => "Prior".into(),
            "PGDN" => "Next".into(),
            "UP" => "Up".into(),
            "DOWN" => "Down".into(),
            "LEFT" => "Left".into(),
            "RIGHT" => "Right".into(),
            other
                if other
                    .strip_prefix('F')
                    .and_then(|n| n.parse::<u8>().ok())
                    .is_some_and(|n| (1..=24).contains(&n)) =>
            {
                other.into()
            }
            _ => return Err("Unsupported activation hotkey key".into()),
        }
    } else if rest.len() == 1 && rest.as_bytes()[0].is_ascii_alphanumeric() {
        rest.to_ascii_lowercase()
    } else {
        return Err("Activation hotkey requires one SendKeys chord".into());
    };
    Ok((modifiers, key))
}

impl QuickerRuntime {
    pub(super) fn run_activate_window(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        let result = self.activate_window(step);
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
            Ok((pid, handle, title)) => {
                self.assign_output(&step.output_params, "pid", Value::from(pid))?;
                self.assign_output(&step.output_params, "mainWinHandle", Value::from(handle))?;
                self.assign_output(&step.output_params, "mainWinTitle", Value::String(title))?;
            }
            Err(error) => {
                self.assign_output(&step.output_params, "pid", Value::from(0))?;
                self.assign_output(&step.output_params, "mainWinHandle", Value::from(0))?;
                self.assign_output(
                    &step.output_params,
                    "mainWinTitle",
                    Value::String(String::new()),
                )?;
                if self
                    .input_value(&step.input_params, "stopIfFail")?
                    .is_none_or(|v| truthy(Some(&v)))
                {
                    return Err(error);
                }
            }
        }
        Ok(StepFlow::Continue)
    }

    #[cfg(target_os = "linux")]
    fn activate_window(
        &self,
        step: &QuickerPluginStepDocument,
    ) -> Result<(u32, u32, String), String> {
        let process = self.input_string(&step.input_params, "process")?;
        let class = self
            .input_string_opt(&step.input_params, "className")?
            .unwrap_or_default();
        let title = self
            .input_string_opt(&step.input_params, "windowTitle")?
            .unwrap_or_default();
        let hotkey = self
            .input_string_opt(&step.input_params, "hotkey")?
            .unwrap_or_default();
        let hotkey = (!hotkey.is_empty())
            .then(|| activation_hotkey(&hotkey))
            .transpose()?;
        let path = self
            .input_string_opt(&step.input_params, "path")?
            .unwrap_or_default();
        let query = crate::x11::WindowQuery::new(&process, &class, &title)?;
        let control = self.control.clone().unwrap_or_default();
        let activate = |window: crate::x11::WindowMatch| {
            ensure_not_cancelled(Some(&control))?;
            crate::x11::restore_focus(&window.process, &control)?;
            Ok((
                window.process.process_id,
                window
                    .process
                    .window_id
                    .parse()
                    .map_err(|_| "Invalid window ID")?,
                window.title,
            ))
        };
        ensure_not_cancelled(Some(&control))?;
        if let Some(window) = query.find()? {
            return activate(window);
        }
        let mut wait_ms = 0;
        if !query.process_running()? && !path.is_empty() {
            if let ExecResult::Err(error) = spawn_program(&path, &[], None) {
                return Err(error);
            }
            wait_ms = 5000;
        } else if let Some((modifiers, key)) = hotkey {
            send_key_combo(&modifiers, &key)?;
            wait_ms = 1000;
        }
        let start = std::time::Instant::now();
        while start.elapsed().as_millis() < wait_ms {
            ensure_not_cancelled(Some(&control))?;
            if let Some(window) = query.find()? {
                return activate(window);
            }
            sleep_millis(25, Some(&control))?;
        }
        Err(format!("No matching application window: {process}"))
    }

    #[cfg(not(target_os = "linux"))]
    fn activate_window(
        &self,
        _step: &QuickerPluginStepDocument,
    ) -> Result<(u32, u32, String), String> {
        Err("Process window activation requires the Linux X11 backend".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activation_hotkeys_accept_single_chords_and_reject_sequences() {
        assert_eq!(
            activation_hotkey("^%q").unwrap(),
            (vec!["ctrl".into(), "alt".into()], "q".into())
        );
        assert_eq!(
            activation_hotkey("+{F12}").unwrap(),
            (vec!["shift".into()], "F12".into())
        );
        for invalid in ["", "^(ab)", "ab", "{F25}", "{ENTER 4}", "Ctrl+Q"] {
            assert!(activation_hotkey(invalid).is_err(), "{invalid}");
        }
    }
}
