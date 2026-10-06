use super::*;

const MAX_CALL_DEPTH: usize = 32;
const MAX_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;

#[cfg(target_os = "linux")]
pub(super) fn needs_input_target(
    data: &QuickerPluginData,
    parents: &[Vec<Value>],
    depth: usize,
) -> bool {
    if depth >= MAX_CALL_DEPTH {
        return true;
    }
    let mut scopes = parents.to_vec();
    scopes.push(data.sub_programs.clone());
    let mut pending = vec![data.steps.as_slice()];
    while let Some(steps) = pending.pop() {
        for step in steps.iter().filter(|s| !s.disabled) {
            if matches!(
                step.step_runner_key.as_str(),
                "sys:keyInput" | "sys:outputText" | "sys:getSelectedText" | "sys:getSelectedFiles"
            ) {
                return true;
            }
            if step.step_runner_key == "sys:subprogram" {
                let Some(binding) = step.input_params.get("subProgram") else {
                    return true;
                };
                let Some(name) = binding["Value"].as_str() else {
                    return true;
                };
                if binding["VarKey"].is_string() || name.starts_with("$=") || name.contains('{') {
                    return true;
                }
                match resolve(name, &scopes, dependency_dir().as_deref()) {
                    Ok(child) if !needs_input_target(&child, &scopes, depth + 1) => {}
                    _ => return true,
                }
            }
            if step.step_runner_key == "sys:keyoperation" {
                let binding = &step.input_params.get("type");
                if binding.is_some_and(|b| {
                    b["VarKey"].is_string()
                        || b["Value"].as_str().is_some_and(|s| s != "get_key_state")
                }) {
                    return true;
                }
            }
            if let Some(steps) = step.if_steps.as_deref() {
                pending.push(steps);
            }
            if let Some(steps) = step.else_steps.as_deref() {
                pending.push(steps);
            }
        }
    }
    false
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Reference<'a> {
    Internal(&'a str),
    Global(&'a str),
    Shared { id: &'a str, revision: u32 },
}

fn is_guid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

pub(super) fn reference(value: &str) -> Result<Reference<'_>, String> {
    if let Some(value) = value.strip_prefix("@@") {
        let parts: Vec<_> = value.splitn(3, '@').collect();
        if parts.len() != 3 || !is_guid(parts[0]) {
            return Err("Shared subprogram requires @@GUID@revision@title".into());
        }
        let revision = parts[1]
            .parse::<u32>()
            .ok()
            .filter(|v| *v > 0)
            .ok_or("Shared subprogram revision must be positive")?;
        Ok(Reference::Shared {
            id: parts[0],
            revision,
        })
    } else if let Some(id) = value.strip_prefix("%%") {
        if !is_guid(id) {
            return Err("Global subprogram requires %%GUID".into());
        }
        Ok(Reference::Global(id))
    } else if value.is_empty() {
        Err("Subprogram name is empty".into())
    } else {
        Ok(Reference::Internal(value))
    }
}

pub(super) fn dependency_dir() -> Option<std::path::PathBuf> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var_os("QUICKER_SUBPROGRAM_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                crate::config::Config::config_path()
                    .parent()
                    .map(|p| p.join("subprograms"))
            })
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

pub(super) fn resolve(
    name: &str,
    scopes: &[Vec<Value>],
    directory: Option<&Path>,
) -> Result<QuickerPluginData, String> {
    match reference(name)? {
        Reference::Internal(name) => {
            // The MSI resolves by Name, starting with the current subprogram.
            for scope in scopes.iter().rev() {
                if let Some(value) = scope.iter().find(|v| v["Name"].as_str() == Some(name)) {
                    if value["UseServerVersion"] == true {
                        return Err(format!("Subprogram {name} requires its server template"));
                    }
                    return serde_json::from_value(value.clone())
                        .map_err(|e| format!("Invalid subprogram {name}: {e}"));
                }
            }
            Err(format!("Subprogram was not found: {name}"))
        }
        external => load_external(external, directory),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn load_external(
    reference: Reference<'_>,
    directory: Option<&Path>,
) -> Result<QuickerPluginData, String> {
    let directory = directory.ok_or("Subprogram directory is unavailable")?;
    let (id, revision, relative) = match reference {
        Reference::Shared { id, revision } => (
            id,
            Some(revision),
            format!("shared/{}/{revision}.json", id.to_ascii_lowercase()),
        ),
        Reference::Global(id) => (id, None, format!("global/{}.json", id.to_ascii_lowercase())),
        Reference::Internal(_) => unreachable!(),
    };
    let path = directory.join(relative);
    let file = fs::File::open(&path)
        .map_err(|e| format!("Cannot read subprogram dependency {}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err("Subprogram document exceeds 16 MiB".into());
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid subprogram JSON: {e}"))?;
    if !value["Id"]
        .as_str()
        .is_some_and(|actual| actual.eq_ignore_ascii_case(id))
    {
        return Err("Subprogram dependency ID does not match the reference".into());
    }
    if let Some(revision) = revision {
        if value["Revision"].as_u64() != Some(u64::from(revision)) {
            return Err("Subprogram dependency revision does not match the reference".into());
        }
        if value["ActionType"] != 25 {
            return Err("Shared dependency is not a Quicker XSubProgram (type 25)".into());
        }
        if value["UseTemplate"] == true {
            return Err("Shared subprogram requires a server template".into());
        }
        parse_json_lenient(
            value["Data"]
                .as_str()
                .ok_or("Shared subprogram has no Data")?,
            "Invalid subprogram data",
        )
    } else {
        if value["UseServerVersion"] == true {
            return Err("Global subprogram requires its server template".into());
        }
        serde_json::from_value(value).map_err(|e| format!("Invalid global subprogram: {e}"))
    }
}

#[cfg(target_arch = "wasm32")]
fn load_external(_: Reference<'_>, _: Option<&Path>) -> Result<QuickerPluginData, String> {
    Err("External subprograms require the native application".into())
}

impl QuickerRuntime {
    pub(super) fn run_subprogram(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        let stop_on_failure = self
            .input_value(&step.input_params, "stopIfFail")?
            .is_none_or(|v| truthy(Some(&v)));
        let result = self.call_subprogram(step);
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
            Err(_) if !stop_on_failure => Ok(StepFlow::Continue),
            other => other,
        }
    }

    fn call_subprogram(&mut self, step: &QuickerPluginStepDocument) -> Result<StepFlow, String> {
        ensure_not_cancelled(self.control.as_ref())?;
        if self.call_depth >= MAX_CALL_DEPTH {
            return Err(format!("Subprogram call depth exceeds {MAX_CALL_DEPTH}"));
        }
        let name = self.input_string(&step.input_params, "subProgram")?;
        let data = resolve(
            &name,
            &self.subprogram_scopes,
            self.dependency_dir.as_deref(),
        )?;
        let mut inputs = HashMap::new();
        for variable in data.variables.iter().filter(|v| v.is_input) {
            if let Some(value) =
                self.input_value(&step.input_params, &format!("var:{}", variable.key))?
            {
                inputs.insert(variable.key.clone(), value);
            }
        }
        let mut child = Self::with_inputs(
            &data,
            self.state_scope.clone(),
            self.control.clone(),
            &inputs,
        )?;
        child.subprogram_scopes = self.subprogram_scopes.clone();
        child.subprogram_scopes.push(data.sub_programs.clone());
        child.call_depth = self.call_depth + 1;
        child.dependency_dir = self.dependency_dir.clone();
        child.action_state = self.action_state.clone();
        child.clipboard_before_copy = self.clipboard_before_copy;
        #[cfg(target_os = "linux")]
        {
            child.keyboard = self.keyboard.clone();
        }
        let result = child.run_steps(&data.steps);
        // State belongs to the action, including changes made before a failure.
        self.action_state = child.action_state;
        self.clipboard_before_copy = child.clipboard_before_copy;
        ensure_not_cancelled(self.control.as_ref())?;
        // Quicker exposes output variables even when execution fails after initialization.
        for variable in data.variables.iter().filter(|v| v.is_output) {
            if let Some(value) = child.vars.get(&variable.key) {
                self.assign_output(
                    &step.output_params,
                    &format!("var:{}", variable.key),
                    value.clone(),
                )?;
            }
        }
        if child.last_message.is_some() {
            self.last_message = child.last_message;
        }
        match result? {
            StepFlow::Continue => Ok(StepFlow::Continue),
            StepFlow::Stop(message) => {
                if message.is_some() {
                    self.last_message = message;
                }
                Ok(StepFlow::Continue)
            }
            stop @ StepFlow::StopAction(_) => Ok(stop),
            StepFlow::BreakLoop | StepFlow::NextIteration => {
                Err("Loop control cannot leave a subprogram".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn runtime(data: Value) -> (QuickerRuntime, QuickerPluginData) {
        let data = serde_json::from_value(data).unwrap();
        (
            QuickerRuntime::new(&data, "subprogram-tests".into(), None).unwrap(),
            data,
        )
    }

    fn call(name: &str) -> Value {
        json!({"StepRunnerKey":"sys:subprogram", "InputParams":{"subProgram":{"Value":name}}, "OutputParams":{"isSuccess":"ok"}})
    }

    #[test]
    fn subprogram_typed_inputs_defaults_outputs_and_fresh_variables() {
        let mut invocation = call("increment");
        invocation["InputParams"]["var:number"] = json!({"VarKey":"number"});
        invocation["InputParams"]["var:private"] = json!({"Value":"must not enter"});
        invocation["OutputParams"]["var:number"] = json!("result");
        invocation["OutputParams"]["var:private"] = json!("leaked");
        let (mut runtime, data) = runtime(json!({
            "Variables":[{"Key":"number","Type":12,"DefaultValue":"7"}],
            "SubPrograms":[{"Name":"increment", "Variables":[
                {"Key":"number","Type":12,"DefaultValue":"$= {missing}","IsInput":true,"IsOutput":true},
                {"Key":"private","Type":0,"DefaultValue":"secret"}
            ], "Steps":[
                {"StepRunnerKey":"sys:assign","InputParams":{"input":{"Value":"$= {number} + 2"}},"OutputParams":{"output":"number"}},
                {"StepRunnerKey":"sys:stop"}, {"StepRunnerKey":"must:not:run"}
            ]}], "Steps":[invocation.clone(), invocation]
        }));
        assert_eq!(runtime.run_steps(&data.steps).unwrap(), StepFlow::Continue);
        assert_eq!(runtime.vars["number"], json!(7));
        assert_eq!(runtime.vars["result"], json!(9));
        assert_eq!(runtime.vars["ok"], json!(true));
        assert!(!runtime.vars.contains_key("leaked"));
    }

    #[test]
    fn subprogram_nested_lookup_uses_nearest_scope_then_parent() {
        let (mut runtime, data) = runtime(json!({"SubPrograms":[
            {"Name":"outer", "SubPrograms":[{"Name":"inner","Steps":[call("root")]}], "Steps":[call("inner")]},
            {"Name":"inner","Steps":[{"StepRunnerKey":"must:not:run"}]},
            {"Name":"root","Steps":[]}
        ], "Steps":[call("outer")]}));
        assert_eq!(runtime.run_steps(&data.steps).unwrap(), StepFlow::Continue);
        assert_eq!(runtime.vars["ok"], json!(true));
        #[cfg(target_os = "linux")]
        {
            assert!(!needs_input_target(&data, &[], 0));
            let mut data = data;
            data.sub_programs[2]["Steps"] = json!([{"StepRunnerKey":"sys:keyInput"}]);
            assert!(needs_input_target(&data, &[], 0));
        }
    }

    #[test]
    fn subprogram_failure_outputs_loop_boundary_and_force_stop() {
        let mut invocation = call("broken");
        invocation["InputParams"]["stopIfFail"] = json!({"Value":"0"});
        invocation["OutputParams"]["errMessage"] = json!("error");
        invocation["OutputParams"]["var:output"] = json!("result");
        let (mut runtime, data) = runtime(json!({"SubPrograms":[
            {"Name":"broken","Variables":[{"Key":"output","Type":12,"DefaultValue":"23","IsOutput":true}],"Steps":[{"StepRunnerKey":"sys:break"}]},
            {"Name":"force","Steps":[{"StepRunnerKey":"sys:stop","InputParams":{"method":{"Value":"forcestop"},"showMessage":{"Value":"done"}}}]}
        ],"Steps":[invocation]}));
        assert_eq!(runtime.run_steps(&data.steps).unwrap(), StepFlow::Continue);
        assert_eq!(runtime.vars["ok"], json!(false));
        assert_eq!(runtime.vars["result"], json!(23));
        assert!(runtime.vars["error"]
            .as_str()
            .unwrap()
            .contains("Loop control"));
        let force = serde_json::from_value(call("force")).unwrap();
        assert_eq!(
            runtime.run_step(&force).unwrap(),
            StepFlow::StopAction(Some("done".into()))
        );
    }

    #[test]
    fn subprogram_recursion_is_bounded_and_cancellation_propagates() {
        // Use the default stack size, as the native action worker does.
        std::thread::Builder::new().stack_size(8 * 1024 * 1024).spawn(|| {
            let (mut runtime, data) = runtime(json!({"SubPrograms":[{"Name":"recur", "Steps":[call("recur")]}],"Steps":[call("recur")]}));
            assert!(runtime.run_steps(&data.steps).unwrap_err().contains("call depth"));
            let control = ActionExecutionControl::new();
            control.cancel();
            runtime.control = Some(control);
            let mut invocation = call("recur");
            invocation["InputParams"]["stopIfFail"] = json!({"Value":"0"});
            assert!(runtime.run_subprogram(&serde_json::from_value(invocation).unwrap()).is_err());
        }).unwrap().join().unwrap();
    }

    #[test]
    fn subprogram_reference_rejects_paths_and_invalid_revisions() {
        for name in [
            "@@../../file@1@bad",
            "%%../file",
            "@@3748cecd-84b6-47f7-191e-08ddfab0d924@0@bad",
            "@@3748cecd-84b6-47f7-191e-08ddfab0d924@-1@bad",
        ] {
            assert!(reference(name).is_err());
        }
        assert_eq!(reference("local").unwrap(), Reference::Internal("local"));
    }

    #[test]
    fn subprogram_cancels_during_child_execution() {
        let control = ActionExecutionControl::new();
        let child_control = control.clone();
        let worker = std::thread::spawn(move || {
            let mut invocation = call("wait");
            invocation["InputParams"]["stopIfFail"] = json!({"Value":"0"});
            let (mut runtime, data) = runtime(json!({"SubPrograms":[{"Name":"wait","Steps":[
                {"StepRunnerKey":"sys:repeat","InputParams":{"count":{"Value":"-1"},"repeatDelayMs":{"Value":"10"}}}
            ]}], "Steps":[invocation]}));
            runtime.control = Some(child_control);
            runtime.run_steps(&data.steps)
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        control.cancel();
        assert!(worker.join().unwrap().is_err());
    }

    #[test]
    fn subprogram_shared_cache_checks_identity_and_executes_pinned_body() {
        let dir = tempfile::tempdir().unwrap();
        let id = "3748cecd-84b6-47f7-191e-08ddfab0d924";
        let target = dir.path().join(format!("shared/{id}/4.json"));
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        let body = json!({"Variables":[{"Key":"answer","Type":12,"DefaultValue":"42","IsOutput":true}],"Steps":[]});
        let mut document = json!({"Id":id,"Revision":4,"ActionType":25,"Data":body.to_string()});
        fs::write(&target, document.to_string()).unwrap();
        let mut invocation = call(&format!("@@{id}@4@title@with-at"));
        invocation["OutputParams"]["var:answer"] = json!("answer");
        let (mut runtime, data) = runtime(json!({"Steps":[invocation]}));
        runtime.dependency_dir = Some(dir.path().into());
        runtime.run_steps(&data.steps).unwrap();
        assert_eq!(runtime.vars["answer"], json!(42));
        document["Revision"] = json!(5);
        fs::write(&target, document.to_string()).unwrap();
        assert!(runtime
            .run_steps(&data.steps)
            .unwrap_err()
            .contains("revision"));
        document["Revision"] = json!(4);
        document["Id"] = json!("00000000-0000-0000-0000-000000000000");
        fs::write(&target, document.to_string()).unwrap();
        assert!(runtime.run_steps(&data.steps).unwrap_err().contains("ID"));
    }
}
