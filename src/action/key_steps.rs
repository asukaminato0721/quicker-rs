use super::*;

/// Accept Windows VirtualKeyCode names and decimal values. Never accept chords.
pub(super) fn key_code(value: &str) -> Result<u16, String> {
    let name = value.trim().to_ascii_uppercase();
    let virtual_name = name.starts_with("VK_");
    let name = name.strip_prefix("VK_").unwrap_or(&name);
    let code = if virtual_name && name.len() == 1 && name.as_bytes()[0].is_ascii_digit() {
        u16::from(name.as_bytes()[0])
    } else if let Some(hex) = name.strip_prefix("0X") {
        u16::from_str_radix(hex, 16).map_err(|_| format!("Invalid hexadecimal key: {value}"))?
    } else if let Ok(value) = name.parse::<u16>() {
        value
    } else if name.len() == 1 && name.as_bytes()[0].is_ascii_uppercase() {
        u16::from(name.as_bytes()[0])
    } else if let Some(n) = name
        .strip_prefix('F')
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|n| (1..=24).contains(n))
    {
        111 + n
    } else if let Some(n) = name
        .strip_prefix("NUMPAD")
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|n| *n <= 9)
    {
        96 + n
    } else {
        match name {
            "LBUTTON" => 1,
            "RBUTTON" => 2,
            "MBUTTON" => 4,
            "XBUTTON1" => 5,
            "XBUTTON2" => 6,
            "BACK" | "BACKSPACE" => 8,
            "TAB" => 9,
            "CLEAR" => 12,
            "RETURN" | "ENTER" => 13,
            "SHIFT" | "SHIFTKEY" => 16,
            "CONTROL" | "CONTROLKEY" | "CTRL" => 17,
            "MENU" | "ALT" => 18,
            "PAUSE" => 19,
            "CAPITAL" | "CAPSLOCK" => 20,
            "ESCAPE" | "ESC" => 27,
            "SPACE" => 32,
            "PRIOR" | "PAGEUP" => 33,
            "NEXT" | "PAGEDOWN" => 34,
            "END" => 35,
            "HOME" => 36,
            "LEFT" => 37,
            "UP" => 38,
            "RIGHT" => 39,
            "DOWN" => 40,
            "SELECT" => 41,
            "PRINT" => 42,
            "EXECUTE" => 43,
            "SNAPSHOT" | "PRINTSCREEN" => 44,
            "INSERT" => 45,
            "DELETE" => 46,
            "HELP" => 47,
            "LWIN" => 91,
            "RWIN" => 92,
            "APPS" => 93,
            "SLEEP" => 95,
            "MULTIPLY" => 106,
            "ADD" => 107,
            "SEPARATOR" => 108,
            "SUBTRACT" => 109,
            "DECIMAL" => 110,
            "DIVIDE" => 111,
            "NUMLOCK" => 144,
            "SCROLL" | "SCROLLLOCK" => 145,
            "LSHIFT" | "LSHIFTKEY" => 160,
            "RSHIFT" | "RSHIFTKEY" => 161,
            "LCONTROL" | "LCONTROLKEY" => 162,
            "RCONTROL" | "RCONTROLKEY" => 163,
            "LMENU" | "LALT" => 164,
            "RMENU" | "RALT" => 165,
            "BROWSER_BACK" => 166,
            "BROWSER_FORWARD" => 167,
            "BROWSER_REFRESH" => 168,
            "BROWSER_STOP" => 169,
            "BROWSER_SEARCH" => 170,
            "BROWSER_FAVORITES" => 171,
            "BROWSER_HOME" => 172,
            "VOLUME_MUTE" => 173,
            "VOLUME_DOWN" => 174,
            "VOLUME_UP" => 175,
            "MEDIA_NEXT_TRACK" => 176,
            "MEDIA_PREV_TRACK" => 177,
            "MEDIA_STOP" => 178,
            "MEDIA_PLAY_PAUSE" => 179,
            "LAUNCH_MAIL" => 180,
            "LAUNCH_MEDIA_SELECT" => 181,
            "LAUNCH_APP1" => 182,
            "LAUNCH_APP2" => 183,
            "OEM_1" | "OEMSEMICOLON" => 186,
            "OEM_PLUS" | "OEMPLUS" => 187,
            "OEM_COMMA" | "OEMCOMMA" => 188,
            "OEM_MINUS" | "OEMMINUS" => 189,
            "OEM_PERIOD" | "OEMPERIOD" => 190,
            "OEM_2" | "OEMQUESTION" => 191,
            "OEM_3" | "OEMTILDE" => 192,
            "OEM_4" | "OEMOPENBRACKETS" => 219,
            "OEM_5" | "OEMPIPE" => 220,
            "OEM_6" | "OEMCLOSEBRACKETS" => 221,
            "OEM_7" | "OEMQUOTES" => 222,
            "OEM_102" | "OEMBACKSLASH" => 226,
            _ => return Err(format!("Unknown Windows key: {value}")),
        }
    };
    if matches!(code, 1 | 2 | 4 | 5 | 6) || !key_symbols(code).is_empty() {
        Ok(code)
    } else {
        Err(format!("Windows key {value} has no Linux mapping"))
    }
}

/// X keysyms for a virtual key. Generic modifiers include both sides.
pub(super) fn key_symbols(code: u16) -> Vec<u32> {
    let symbol = match code {
        8 => 0xff08,
        9 => 0xff09,
        12 => 0xff0b,
        13 => 0xff0d,
        16 => return vec![0xffe1, 0xffe2],
        17 => return vec![0xffe3, 0xffe4],
        18 => return vec![0xffe9, 0xffea],
        19 => 0xff13,
        20 => 0xffe5,
        27 => 0xff1b,
        32 => 0x20,
        33 => 0xff55,
        34 => 0xff56,
        35 => 0xff57,
        36 => 0xff50,
        37 => 0xff51,
        38 => 0xff52,
        39 => 0xff53,
        40 => 0xff54,
        41 => 0xff60,
        42 | 44 => 0xff61,
        43 => 0xff62,
        45 => 0xff63,
        46 => 0xffff,
        47 => 0xff6a,
        48..=57 => u32::from(code),
        65..=90 => u32::from(code + 32),
        91 => 0xffeb,
        92 => 0xffec,
        93 => 0xff67,
        95 => 0x1008ff2f,
        96..=105 => 0xffb0 + u32::from(code - 96),
        106..=111 => 0xffaa + u32::from(code - 106),
        112..=135 => 0xffbe + u32::from(code - 112),
        144 => 0xff7f,
        145 => 0xff14,
        160 => 0xffe1,
        161 => 0xffe2,
        162 => 0xffe3,
        163 => 0xffe4,
        164 => 0xffe9,
        165 => 0xffea,
        166 => 0x1008ff26,
        167 => 0x1008ff27,
        168 => 0x1008ff29,
        169 => 0x1008ff28,
        170 => 0x1008ff1b,
        171 => 0x1008ff30,
        172 => 0x1008ff18,
        173 => 0x1008ff12,
        174 => 0x1008ff11,
        175 => 0x1008ff13,
        176 => 0x1008ff17,
        177 => 0x1008ff16,
        178 => 0x1008ff15,
        179 => 0x1008ff14,
        180 => 0x1008ff19,
        181 => 0x1008ff32,
        182 => 0x1008ff40,
        183 => 0x1008ff41,
        186 => b';' as u32,
        187 => b'=' as u32,
        188 => b',' as u32,
        189 => b'-' as u32,
        190 => b'.' as u32,
        191 => b'/' as u32,
        192 => b'`' as u32,
        219 => b'[' as u32,
        220 => b'\\' as u32,
        221 => b']' as u32,
        222 => b'\'' as u32,
        226 => b'<' as u32,
        _ => return vec![],
    };
    vec![symbol]
}

impl QuickerRuntime {
    pub(super) fn run_key_operation(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        ensure_not_cancelled(self.control.as_ref())?;
        let operation = self
            .input_string_opt(&step.input_params, "type")?
            .unwrap_or_else(|| "get_key_state".into());
        if !matches!(operation.as_str(), "get_key_state" | "key_down" | "key_up") {
            return Err(format!("Unsupported key operation: {operation}"));
        }
        let key = key_code(&self.input_string(&step.input_params, "key")?)?;
        if operation == "get_key_state"
            && self.input_bool(&step.input_params, "getRealMouseState")?
        {
            return Err(
                "Physical device state is not available through the X11 key backend".into(),
            );
        }
        #[cfg(target_os = "linux")]
        {
            let state = {
                let mut keyboard = self.keyboard.lock().map_err(|_| "Keyboard lock failed")?;
                if keyboard.is_none() {
                    *keyboard = Some(crate::x11::Keyboard::connect()?);
                }
                let keyboard = keyboard.as_mut().unwrap();
                match operation.as_str() {
                    "key_down" => {
                        keyboard.change(key, &key_symbols(key), true)?;
                        None
                    }
                    "key_up" => {
                        keyboard.change(key, &key_symbols(key), false)?;
                        None
                    }
                    _ => Some(keyboard.state(key, &key_symbols(key))?),
                }
            };
            if let Some((down, toggled)) = state {
                self.assign_output(&step.output_params, "isDown", Value::Bool(down))?;
                self.assign_output(&step.output_params, "isToggled", Value::Bool(toggled))?;
            }
            Ok(StepFlow::Continue)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = key;
            Err("Key operations require the Linux application".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn key_names_and_codes_preserve_windows_meaning() {
        for (name, code) in [
            ("Space", 32),
            ("32", 32),
            ("0x20", 32),
            ("VK_A", 65),
            ("VK_0", 48),
            ("F24", 135),
            ("RCONTROL", 163),
            ("NUMPAD8", 104),
            ("OEM_5", 220),
            ("LBUTTON", 1),
        ] {
            // VK_0 is an enum name. Bare 0 is a numeric virtual key.
            assert_eq!(key_code(name).unwrap(), code);
        }
        for name in ["Ctrl+A", "--help", "0", "65535", "F25", "NUMPAD10", "bad"] {
            assert!(key_code(name).is_err(), "{name}");
        }
        assert_eq!(key_symbols(17), vec![0xffe3, 0xffe4]);
        assert_eq!(key_symbols(163), vec![0xffe4]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires an isolated X11 display with XTEST and XKB"]
    fn key_operations_query_state_and_release_owned_keys_after_stop_and_cancel() {
        use serde_json::json;
        let observer = crate::x11::Keyboard::connect().unwrap();
        let state = |key| observer.state(key, &key_symbols(key)).unwrap();
        let step = |operation: &str, key: &str| {
            json!({"StepRunnerKey":"sys:keyoperation", "InputParams":{
            "type":{"Value":operation}, "key":{"Value":key}}, "OutputParams":{"isDown":"down", "isToggled":"toggled"}})
        };
        let data: QuickerPluginData = serde_json::from_value(json!({"Steps":[
            {"StepRunnerKey":"sys:subprogram", "InputParams":{"subProgram":{"Value":"hold"}}},
            step("get_key_state", "SHIFT")],
            "SubPrograms":[{"Name":"hold", "Steps":[step("key_down", "RSHIFT")]}]
        }))
        .unwrap();
        let mut runtime = QuickerRuntime::new(&data, "key-state".into(), None).unwrap();
        runtime.run_steps(&data.steps).unwrap();
        assert_eq!(runtime.vars["down"], json!(true));
        assert_eq!(runtime.vars["toggled"], json!(false));
        assert!(state(161).0);
        assert!(!state(160).0);
        drop(runtime);
        assert!(!state(16).0);

        // Cleanup also runs after an execution error, cancellation, and normal stop.
        for tail in [
            json!({"StepRunnerKey":"vendor:fail"}),
            json!({"StepRunnerKey":"sys:repeat", "InputParams":{"count":{"Value":"-1"}}}),
            json!({"StepRunnerKey":"sys:stop"}),
        ] {
            let cancel = tail["StepRunnerKey"] == "sys:repeat";
            let data: QuickerPluginData =
                serde_json::from_value(json!({"Steps":[step("key_down", "LCONTROL"), tail]}))
                    .unwrap();
            let control = ActionExecutionControl::default();
            let mut runtime =
                QuickerRuntime::new(&data, "key-cleanup".into(), Some(control.clone())).unwrap();
            let timer = cancel.then(|| {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(70));
                    control.cancel();
                })
            });
            let result = runtime.run_steps(&data.steps);
            if cancel {
                assert_eq!(result.unwrap_err(), "Action cancelled");
            }
            assert!(state(162).0);
            drop(runtime);
            assert!(!state(17).0);
            if let Some(timer) = timer {
                timer.join().unwrap();
            }
        }
        // A key held before this action is not owned by its cleanup guard.
        let mut external = crate::x11::Keyboard::connect().unwrap();
        external.change(160, &key_symbols(160), true).unwrap();
        {
            let mut own = crate::x11::Keyboard::connect().unwrap();
            own.change(160, &key_symbols(160), true).unwrap();
        }
        assert!(state(160).0);
        drop(external);
        assert!(!state(160).0);

        let mut keyboard = crate::x11::Keyboard::connect().unwrap();
        let initial = state(20).1;
        keyboard.change(20, &key_symbols(20), true).unwrap();
        keyboard.change(20, &key_symbols(20), false).unwrap();
        assert_eq!(state(20), (false, !initial));
        keyboard.change(20, &key_symbols(20), true).unwrap();
        keyboard.change(20, &key_symbols(20), false).unwrap();
        assert_eq!(state(20), (false, initial));
        assert_eq!(state(1), (false, false));
        assert!(keyboard.change(1, &[], true).is_err());
    }
}
