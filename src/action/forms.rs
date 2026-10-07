//! Quicker forms. Parse the definition before evaluating field expressions.
use super::*;

mod definition;
pub(super) use definition::{definition, validate_option, OPTIONS};
pub(crate) use definition::{Field, Options};

impl QuickerRuntime {
    pub(super) fn run_form(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        self.form_with(step, show)
    }

    fn form_with(
        &mut self,
        step: &QuickerPluginStepDocument,
        show: impl FnOnce(
            Options,
            bool,
            Option<&ActionExecutionControl>,
        ) -> Result<Option<Vec<String>>, String>,
    ) -> Result<StepFlow, String> {
        let stop = self
            .input_value(&step.input_params, "stopIfFail")?
            .is_none_or(|v| truthy(Some(&v)));
        let mut button = String::new();
        let result = (|| {
            ensure_not_cancelled(self.control.as_ref())?;
            for key in OPTIONS {
                if let Some(value) = self.input_value(&step.input_params, key)? {
                    if !validate_option(key, &value) {
                        return Err(format!("Unsupported form option: {key}"));
                    }
                }
            }
            let operation = self
                .input_string_opt(&step.input_params, "operation")?
                .unwrap_or_else(|| "variables".into());
            let dict_key = if matches!(operation.as_str(), "dict" | "dict_dynamic") {
                Some(
                    step.input_params
                        .get("dictVar")
                        .and_then(|b| b["VarKey"].as_str())
                        .filter(|s| !s.is_empty())
                        .ok_or("Form requires a dictionary variable")?
                        .to_owned(),
                )
            } else {
                None
            };
            let context = match &dict_key {
                Some(key) => self
                    .vars
                    .get(key)
                    .and_then(Value::as_object)
                    .ok_or("Form requires a dictionary variable")?
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                None => self.vars.clone(),
            };
            let param = match operation.as_str() {
                "dict" => "formForDictDef",
                "dict_dynamic" => "dynamicFormForDictDef",
                _ => "formDef",
            };
            // Static definitions are JSON documents, not interpolation templates.
            let literal_dynamic = step.input_params.get(param).is_some_and(|b| {
                !b["VarKey"].is_string()
                    && (b["Value"].is_array()
                        || b["Value"]
                            .as_str()
                            .is_some_and(|s| s.trim_start().starts_with('[')))
            });
            let data = if operation == "dict_dynamic" && !literal_dynamic {
                self.input_value(&step.input_params, param)?
                    .ok_or("Missing form definition")?
            } else {
                step.input_params
                    .get(param)
                    .and_then(|b| b.get("Value"))
                    .cloned()
                    .ok_or("Missing form definition")?
            };
            let fields = definition(&data, operation == "dict_dynamic")?;
            let fields = fields
                .into_iter()
                .map(|field| {
                    Field::build(&field, &context, &self.variable_types, dict_key.is_some())
                })
                .collect::<Result<Vec<_>, _>>()?;
            let text = |key: &str, default: &str| {
                self.input_string_opt(&step.input_params, key)
                    .map(|v| v.unwrap_or_else(|| default.into()))
            };
            let options = Options {
                fields,
                title: text("title", "填写表单")?,
                help: text("help", "")?,
                width: definition::dimension(&text("windowWidth", "500")?, 500.0)?.max(400.0),
                height: definition::dimension(&text("windowHeight", "0")?, 0.0)?,
                label_width: definition::dimension(&text("titleColumnWidth", "100")?, 100.0)?,
                input_width: definition::dimension(&text("defaultInputWidth", "0")?, 0.0)?,
                topmost: self.input_bool(&step.input_params, "topMost")?,
                disable_enter: self.input_bool(&step.input_params, "disableEnterSubmit")?,
            };
            options.check_size()?;
            let restore = self.input_bool(&step.input_params, "restoreFocus")?;
            let _dialog = dialogs::DialogSession::new(self.control.as_ref());
            let outcome = show(options.clone(), restore, self.control.as_ref())?;
            ensure_not_cancelled(self.control.as_ref())?;
            let Some(values) = outcome else {
                button = "Cancel".into();
                return Err("Form cancelled".into());
            };
            // Convert all fields before writing any variable.
            let values = options.values(&values)?;
            ensure_not_cancelled(self.control.as_ref())?;
            if let Some(key) = dict_key {
                let mut dict = self.vars[&key].as_object().unwrap().clone();
                dict.extend(values);
                self.vars.insert(key, Value::Object(dict));
            } else {
                self.vars.extend(values);
            }
            Ok(())
        })();
        ensure_not_cancelled(self.control.as_ref())?;
        self.assign_output(
            &step.output_params,
            "isSuccess",
            Value::Bool(result.is_ok()),
        )?;
        self.assign_output(&step.output_params, "button", Value::String(button))?;
        self.assign_output(
            &step.output_params,
            "selectedGroup",
            Value::String(String::new()),
        )?;
        self.assign_output(
            &step.output_params,
            "errMessage",
            Value::String(result.as_ref().err().cloned().unwrap_or_default()),
        )?;
        if stop {
            result?;
        }
        Ok(StepFlow::Continue)
    }
}

fn show(
    options: Options,
    restore: bool,
    control: Option<&ActionExecutionControl>,
) -> Result<Option<Vec<String>>, String> {
    #[cfg(target_os = "linux")]
    {
        let focus = dialogs::DialogFocus::capture(restore)?;
        let result = crate::form_windows::show(options, control.cloned().unwrap_or_default());
        ensure_not_cancelled(control)?;
        focus.restore(control)?;
        result
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (options, restore, control);
        Err("Forms require the native Linux application".into())
    }
}

#[cfg(test)]
mod tests;
