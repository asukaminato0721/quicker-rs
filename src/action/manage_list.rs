//! Manage a list copy. Only confirmation changes the bound workflow variable.
use super::*;

pub(super) const OPTIONS: &[&str] = &[
    "parseData",
    "windowSize",
    "titleDelegate",
    "help",
    "addSubprogram",
    "editSubprogram",
];

pub(super) fn width(text: &str) -> Result<f32, String> {
    if text.is_empty() {
        return Ok(640.0);
    }
    let number = text
        .trim()
        .parse::<f32>()
        .map_err(|_| "Invalid list window width")?;
    if !number.is_finite() || number > 4096.0 {
        return Err("List window width must be finite and at most 4096".into());
    }
    Ok(number.max(200.0))
}

pub(super) fn validate_option(key: &str, value: &Value) -> bool {
    match key {
        "parseData" => !truthy(Some(value)),
        "windowSize" => width(&value_to_string(value)).is_ok(),
        _ => value_to_string(value).is_empty(),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Options {
    pub items: Vec<String>,
    pub title: String,
    pub note: String,
    pub width: f32,
    pub allow_add: bool,
    pub allow_edit: bool,
    pub allow_delete: bool,
}

impl QuickerRuntime {
    pub(super) fn run_manage_list(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        self.manage_list_with(step, show)
    }

    fn manage_list_with(
        &mut self,
        step: &QuickerPluginStepDocument,
        show: impl FnOnce(
            Options,
            Option<&ActionExecutionControl>,
        ) -> Result<Option<Vec<String>>, String>,
    ) -> Result<StepFlow, String> {
        let stop = self.input_bool(&step.input_params, "stopIfFail")?;
        let result = (|| {
            ensure_not_cancelled(self.control.as_ref())?;
            let key = step
                .input_params
                .get("list")
                .and_then(|b| b.get("VarKey"))
                .and_then(Value::as_str)
                .filter(|key| !key.is_empty())
                .ok_or("Manage list requires a list variable")?
                .to_owned();
            let list = self
                .vars
                .get(&key)
                .and_then(Value::as_array)
                .ok_or("Manage list requires a list variable")?;
            let items = list
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| "Manage list requires text items".to_owned())
                })
                .collect::<Result<Vec<_>, _>>()?;
            list_steps::check_size(&items)?;
            for option in OPTIONS {
                // Newer custom editor settings are inactive when editing is disabled.
                let enabled = match *option {
                    "addSubprogram" => self
                        .input_value(&step.input_params, "allowAdd")?
                        .is_none_or(|v| truthy(Some(&v))),
                    "editSubprogram" => self
                        .input_value(&step.input_params, "allowEdit")?
                        .is_none_or(|v| truthy(Some(&v))),
                    _ => true,
                };
                if enabled {
                    if let Some(value) = self.input_value(&step.input_params, option)? {
                        if !validate_option(option, &value) {
                            return Err(format!("Unsupported manageList option: {option}"));
                        }
                    }
                }
            }
            let text = |key| {
                self.input_string_opt(&step.input_params, key)
                    .map(Option::unwrap_or_default)
            };
            let title = text("winTitle")?;
            let note = text("note")?;
            if title.len() > 65536 || note.len() > 1024 * 1024 {
                return Err("List window title or note exceeds its size limit".into());
            }
            let enabled = |key| {
                self.input_value(&step.input_params, key)
                    .map(|v| v.is_none_or(|v| truthy(Some(&v))))
            };
            let options = Options {
                items,
                title: if title.is_empty() {
                    "Manage list".into()
                } else {
                    title
                },
                note,
                width: width(&text("windowSize")?)?,
                allow_add: enabled("allowAdd")?,
                allow_edit: enabled("allowEdit")?,
                allow_delete: enabled("allowDelete")?,
            };
            let _dialog = dialogs::DialogSession::new(self.control.as_ref());
            let edited = show(options, self.control.as_ref())?;
            ensure_not_cancelled(self.control.as_ref())?;
            let items = edited.ok_or("List editing cancelled")?;
            list_steps::check_size(&items)?;
            self.vars.insert(
                key,
                Value::Array(items.into_iter().map(Value::String).collect()),
            );
            Ok::<(), String>(())
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
        if stop {
            result?;
        }
        Ok(StepFlow::Continue)
    }
}

fn show(
    options: Options,
    control: Option<&ActionExecutionControl>,
) -> Result<Option<Vec<String>>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::list_windows::show(options, control.cloned().unwrap_or_default())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (options, control);
        Err("List editing requires the native application".into())
    }
}

pub(crate) fn check_items(items: &[String]) -> Result<(), String> {
    list_steps::check_size(items)
}

#[cfg(test)]
mod tests;
