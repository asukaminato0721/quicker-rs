//! Apply edits to imported JSON without reconstructing fields the builder does
//! not represent. Each step carries its own source through moves and deletion.
use super::*;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SourceDocument {
    pub original: Value,
    pub baseline: Value,
}

pub(super) fn apply_changes(source: &Value, before: &Value, after: &Value) -> Value {
    if before == after {
        return source.clone();
    }
    if let (Some(old), Some(new)) = (before.as_object(), after.as_object()) {
        let mut result = source.as_object().cloned().unwrap_or_default();
        for key in old.keys().filter(|key| !new.contains_key(*key)) {
            result.remove(key);
        }
        for (key, value) in new {
            if old.get(key) == Some(value) {
                continue;
            }
            result.insert(
                key.clone(),
                apply_changes(
                    result.get(key).unwrap_or(&Value::Null),
                    old.get(key).unwrap_or(&Value::Null),
                    value,
                ),
            );
        }
        Value::Object(result)
    } else {
        after.clone()
    }
}

pub(super) fn import_step(source: &Value) -> Result<LowCodePluginStep, String> {
    let document: QuickerPluginStepDocument = serde_json::from_value(source.clone())
        .map_err(|err| format!("Invalid workflow step: {err}"))?;
    let mut step = match low_code_step_from_document(&document) {
        Ok(step) => step,
        Err(reason) => {
            return Ok(LowCodePluginStep::Raw {
                json: serde_json::to_string_pretty(source).map_err(|err| err.to_string())?,
                reason,
            })
        }
    };
    if let LowCodePluginStep::SimpleIf {
        if_steps,
        else_steps,
        ..
    } = &mut step
    {
        for (key, steps) in [("IfSteps", if_steps), ("ElseSteps", else_steps)] {
            *steps = source
                .get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(import_step)
                .collect::<Result<_, _>>()?;
        }
    }
    let baseline = step.to_step_value(&mut BTreeSet::new())?;
    Ok(LowCodePluginStep::Preserved {
        source: source.clone(),
        baseline,
        step: Box::new(step),
    })
}

pub(super) fn merge_document(source: &SourceDocument, generated: Value) -> Result<Value, String> {
    let mut result = apply_changes(&source.original, &source.baseline, &generated);
    let kind = generated["ActionType"].as_u64();
    if source.baseline["ActionType"] != generated["ActionType"]
        || source.baseline["Data"] == generated["Data"]
        || !matches!(kind, Some(11 | 24))
    {
        return Ok(result);
    }
    let decode = |value: &Value| -> Result<Value, String> {
        let text = value.as_str().ok_or("Plugin Data must be a JSON string")?;
        parse_json_lenient(
            text.strip_prefix("json:").unwrap_or(text),
            "Invalid plugin Data",
        )
    };
    let original = decode(&source.original["Data"])?;
    let baseline = decode(&source.baseline["Data"])?;
    let generated = decode(&generated["Data"])?;
    let mut data = apply_changes(&original, &baseline, &generated);
    if kind == Some(24) {
        // Existing variable definitions retain types, defaults, persistence,
        // and custom fields. Only newly introduced output names are appended.
        let mut variables = original["Variables"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for variable in generated["Variables"].as_array().into_iter().flatten() {
            let key = &variable["Key"];
            let existed = baseline["Variables"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|entry| &entry["Key"] == key);
            if !existed && !variables.iter().any(|entry| &entry["Key"] == key) {
                variables.push(variable.clone());
            }
        }
        if original.get("Variables").is_some() || !variables.is_empty() {
            data["Variables"] = Value::Array(variables);
        } else if let Some(object) = data.as_object_mut() {
            object.remove("Variables");
        }
    }
    let prefix = if source.original["Data"]
        .as_str()
        .is_some_and(|s| s.starts_with("json:"))
    {
        "json:"
    } else {
        ""
    };
    result["Data"] = Value::String(format!(
        "{prefix}{}",
        serde_json::to_string(&data).map_err(|err| err.to_string())?
    ));
    Ok(result)
}
