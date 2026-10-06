use super::*;

pub(super) const METHODS: &[&str] = &[
    "toLower",
    "toUpper",
    "trim",
    "trimStart",
    "trimEnd",
    "urlEncode",
];

impl QuickerRuntime {
    pub(super) fn run_string_process(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        let stop = self
            .input_value(&step.input_params, "stopIfFail")?
            .is_none_or(|v| truthy(Some(&v)));
        let result = (|| {
            ensure_not_cancelled(self.control.as_ref())?;
            let input = self
                .input_string_opt(&step.input_params, "data")?
                .unwrap_or_default();
            let method = self.input_string(&step.input_params, "method")?;
            match method.to_ascii_lowercase().as_str() {
                "tolower" => Ok(input.to_lowercase()),
                "toupper" => Ok(input.to_uppercase()),
                "trim" => Ok(input.trim().to_owned()),
                "trimstart" => Ok(input.trim_start().to_owned()),
                "trimend" => Ok(input.trim_end().to_owned()),
                "urlencode" => {
                    let encoding = self
                        .input_string_opt(&step.input_params, "srcEncoding")?
                        .unwrap_or_default();
                    if !matches!(
                        encoding.to_ascii_lowercase().as_str(),
                        "" | "utf8" | "utf-8"
                    ) {
                        return Err("URL encoding currently requires UTF-8".into());
                    }
                    Ok(urlencoding::encode(&input).into_owned())
                }
                _ => Err(format!("Unsupported stringProcess method: {method}")),
            }
        })();
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
        self.assign_output(
            &step.output_params,
            "output",
            Value::String(result.as_ref().cloned().unwrap_or_default()),
        )?;
        if stop {
            result?;
        }
        Ok(StepFlow::Continue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn text_operations_preserve_content_and_report_failures() {
        let data = serde_json::from_value(json!({})).unwrap();
        let mut runtime = QuickerRuntime::new(&data, "strings-test".into(), None).unwrap();
        for (method, input, expected) in [
            ("trim", "\u{2003}\t中\u{00a0}\r\n", "中"),
            ("TRIMSTART", "  中 \n", "中 \n"),
            ("trimEnd", " \t中\r\n", " \t中"),
            ("trim", "\u{200b}中\u{200b}", "\u{200b}中\u{200b}"),
            ("toUpper", "aBc 中", "ABC 中"),
            ("toLower", "ABC 中", "abc 中"),
            ("urlEncode", "a +中", "a%20%2B%E4%B8%AD"),
            ("trim", "", ""),
        ] {
            let step = serde_json::from_value(json!({"StepRunnerKey":"sys:stringProcess", "InputParams":{"method":{"Value":method}, "data":{"Value":input}}, "OutputParams":{"output":"text", "isSuccess":"ok"}})).unwrap();
            runtime.run_string_process(&step).unwrap();
            assert_eq!(runtime.vars["text"], expected);
            assert_eq!(runtime.vars["ok"], true);
        }
        let mut step: QuickerPluginStepDocument = serde_json::from_value(json!({"StepRunnerKey":"sys:stringProcess", "InputParams":{"method":{"Value":"urlEncode"}, "srcEncoding":{"Value":"gbk"}, "stopIfFail":{"Value":"0"}}, "OutputParams":{"output":"text", "isSuccess":"ok", "errMessage":"error"}})).unwrap();
        runtime.run_string_process(&step).unwrap();
        assert_eq!(runtime.vars["text"], "");
        assert_eq!(runtime.vars["ok"], false);
        assert!(runtime.vars["error"].as_str().unwrap().contains("UTF-8"));
        step.input_params.remove("stopIfFail");
        assert!(runtime.run_string_process(&step).is_err());
        step.input_params
            .insert("stopIfFail".into(), json!({"Value":"false"}));
        let control = ActionExecutionControl::new();
        control.cancel();
        runtime.control = Some(control);
        assert!(runtime
            .run_string_process(&step)
            .unwrap_err()
            .contains("cancelled"));
    }

    #[test]
    fn editor_retains_supported_methods_and_exposes_unknown_ones_as_json() {
        for method in LowCodeStringProcessMethod::ALL {
            let source = json!({"StepRunnerKey":"sys:stringProcess", "InputParams":{"data":{"Value":" text "}, "method":{"Value":method.key()}}, "OutputParams":{"output":"result"}});
            let mut imported = preservation::import_step(&source).unwrap();
            let LowCodePluginStep::Preserved { step, .. } = &mut imported else {
                panic!("Expected text editor")
            };
            let LowCodePluginStep::StringProcess { input, .. } = step.as_mut() else {
                panic!("Expected text editor")
            };
            *input = "new text".into();
            let result = imported.to_step_value(&mut BTreeSet::new()).unwrap();
            assert_eq!(result["InputParams"]["method"]["Value"], method.key());
            assert_eq!(result["InputParams"]["data"]["Value"], "new text");
        }
        let unknown = json!({"StepRunnerKey":"sys:stringProcess", "InputParams":{"method":{"Value":"future"}}});
        assert!(matches!(
            preservation::import_step(&unknown).unwrap(),
            LowCodePluginStep::Raw { .. }
        ));
        let regex = json!({"StepRunnerKey":"sys:regexExtract", "InputParams":{"getGroup":{"Value":"1"}}, "OutputParams":{"match1 ":"name", "match2 ":"id"}});
        let imported = preservation::import_step(&regex).unwrap();
        assert!(matches!(imported, LowCodePluginStep::Raw { .. }));
        assert_eq!(imported.to_step_value(&mut BTreeSet::new()).unwrap(), regex);
    }
}
