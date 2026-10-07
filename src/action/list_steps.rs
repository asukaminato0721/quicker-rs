use super::*;
use std::collections::HashSet;

const MAX_ITEMS: usize = 100_000;
const MAX_BYTES: usize = 16 * 1024 * 1024;

pub(super) const OPERATIONS: &[&str] = &[
    "none",
    "getAt",
    "append",
    "insertAt",
    "setAt",
    "remove",
    "removeAllByValue",
    "removeAt",
    "removeByMatch",
    "removeByNotMatch",
    "clear",
    "sortAsc",
    "sortDesc",
    "sortAscNature",
    "FileSizeAsc",
    "FileSizeDesc",
    "CreationTimeAsc",
    "CreationTimeDesc",
    "LastAccessTimeAsc",
    "LastAccessTimeDesc",
    "LastWriteTimeAsc",
    "LastWriteTimeDesc",
    "reverse",
    "sub",
    "concat",
    "distinct",
    "indexOf",
    "filterByRegex",
    "filterByContains",
    "filterByStarts",
    "filterByEnds",
];

fn mutates(operation: &str) -> bool {
    matches!(
        operation,
        "append"
            | "insertAt"
            | "setAt"
            | "remove"
            | "removeAllByValue"
            | "removeAt"
            | "removeByMatch"
            | "removeByNotMatch"
            | "clear"
            | "reverse"
    )
}

fn strings(value: Value) -> Result<Vec<String>, String> {
    let value = expression::convert(value, Some(4))?;
    let values = value.as_array().ok_or("Input is not a list")?;
    let result: Vec<String> = values
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| "List operations require text items".to_owned())
        })
        .collect::<Result<_, _>>()?;
    check_size(&result)?;
    Ok(result)
}

pub(super) fn check_size(items: &[String]) -> Result<(), String> {
    if items.len() > MAX_ITEMS || items.iter().map(String::len).sum::<usize>() > MAX_BYTES {
        return Err("List exceeds 100000 items or 16 MiB of text".into());
    }
    Ok(())
}

fn array(items: Vec<String>) -> Value {
    Value::Array(items.into_iter().map(Value::String).collect())
}

// A simple uppercase mapping avoids expanding one character into multiple
// characters. Unicode tables can still differ from .NET Framework.
pub(super) fn ordinal_fold(text: &str) -> String {
    text.chars()
        .map(|c| {
            let mut upper = c.to_uppercase();
            let first = upper.next().unwrap();
            if upper.next().is_none() {
                first
            } else {
                c
            }
        })
        .collect()
}

impl QuickerRuntime {
    pub(super) fn run_list_operation(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        let result = self.list_operation(step);
        ensure_not_cancelled(self.control.as_ref())?;
        self.assign_output(
            &step.output_params,
            "errMessage",
            Value::String(result.as_ref().err().cloned().unwrap_or_default()),
        )?;
        result.map(|()| StepFlow::Continue)
    }

    fn list_operation(&mut self, step: &QuickerPluginStepDocument) -> Result<(), String> {
        ensure_not_cancelled(self.control.as_ref())?;
        let params = &step.input_params;
        let operation = self
            .input_string_opt(params, "type")?
            .unwrap_or_else(|| "none".into());
        if !OPERATIONS.contains(&operation.as_str()) {
            return Err(format!("Unsupported list operation: {operation}"));
        }
        // The supplied MSI passes no stop flag to ExecuteCommonAction. It stops
        // on failure. Do not silently claim support for a newer catalog option.
        if self
            .input_value(params, "stopIfFail")?
            .is_some_and(|v| !truthy(Some(&v)))
        {
            return Err(
                "List stopIfFail=false is not supported by the MSI compatibility runner".into(),
            );
        }
        let binding = params.get("list").ok_or("Missing list input")?;
        let variable = binding.get("VarKey").and_then(Value::as_str);
        if mutates(&operation)
            && !variable.is_some_and(|key| self.vars.get(key).is_some_and(Value::is_array))
        {
            return Err("A mutating list operation requires a list variable".into());
        }
        let mut items = strings(
            self.input_value(params, "list")?
                .ok_or("Missing list input")?,
        )?;
        let integer = |key, default| -> Result<i32, String> {
            self.input_string_opt(params, key)?
                .map_or(Ok(default), |s| {
                    s.parse::<i32>()
                        .map_err(|_| format!("List {key} must be a 32-bit integer"))
                })
        };
        let mut position = i64::from(integer("pos", 0)?);
        let length = integer("length", 1)?;
        let item = self.input_string_opt(params, "item")?.unwrap_or_default();
        if position < 0 {
            position += items.len() as i64;
        }
        let index = |allow_end: bool| -> Result<usize, String> {
            usize::try_from(position)
                .ok()
                .filter(|&p| p < items.len() || (allow_end && p == items.len()))
                .ok_or_else(|| "List position is out of range".into())
        };
        let mut value = None;
        let mut excluded = None;
        let mut found_index = None;
        let mut result_items = None;
        match operation.as_str() {
            "none" => {}
            "getAt" => value = Some(Value::String(items[index(false)?].clone())),
            "append" => items.push(item),
            "insertAt" => {
                let p = index(true)?;
                items.insert(p, item);
            }
            "setAt" => {
                let p = index(false)?;
                items[p] = item;
            }
            "removeAt" => {
                // The MSI adds Count again when the normalized index is negative.
                let p = if position < 0 {
                    position + items.len() as i64
                } else {
                    position
                };
                let p = usize::try_from(p)
                    .ok()
                    .filter(|&p| p < items.len())
                    .ok_or("List position is out of range")?;
                items.remove(p);
            }
            "remove" => {
                if let Some(p) = items.iter().position(|v| v == &item) {
                    items.remove(p);
                }
            }
            "removeAllByValue" => items.retain(|v| v != &item),
            "clear" => items.clear(),
            "reverse" => items.reverse(),
            "indexOf" => {
                found_index = Some(
                    items
                        .iter()
                        .position(|v| v == &item)
                        .map_or(-1, |p| p as i64),
                )
            }
            "sub" => {
                result_items = Some(
                    items
                        .iter()
                        .skip(position.max(0) as usize)
                        .take(length.max(0) as usize)
                        .cloned()
                        .collect(),
                )
            }
            "concat" => {
                let mut result = items.clone();
                result.extend(strings(
                    self.input_value(params, "list2")?
                        .unwrap_or_else(|| Value::String(String::new())),
                )?);
                result_items = Some(result);
            }
            "distinct" => {
                let mut seen = HashSet::new();
                result_items = Some(
                    items
                        .iter()
                        .filter(|v| seen.insert(v.as_str()))
                        .cloned()
                        .collect(),
                );
            }
            "removeByMatch" | "removeByNotMatch" | "filterByRegex" | "filterByContains"
            | "filterByStarts" | "filterByEnds" => {
                let regex = if matches!(
                    operation.as_str(),
                    "removeByMatch" | "removeByNotMatch" | "filterByRegex"
                ) {
                    let pattern = self
                        .input_string_opt(params, "pattern")?
                        .unwrap_or_default();
                    if pattern.is_empty() {
                        return Err("List regex pattern is empty".into());
                    }
                    Some(regex_steps::compile(&pattern, false, false, false)?)
                } else {
                    None
                };
                let needle = ordinal_fold(&item);
                let mut kept = Vec::new();
                let mut removed = Vec::new();
                let mut seen = HashSet::new();
                for text in &items {
                    ensure_not_cancelled(self.control.as_ref())?;
                    let matches = if let Some(regex) = &regex {
                        regex
                            .is_match(text)
                            .map_err(|e| format!("List regex failed: {e}"))?
                    } else {
                        let folded = ordinal_fold(text);
                        match operation.as_str() {
                            "filterByContains" => folded.contains(&needle),
                            "filterByStarts" => folded.starts_with(&needle),
                            _ => folded.ends_with(&needle),
                        }
                    };
                    let keep = if operation == "removeByMatch" {
                        !matches
                    } else {
                        matches
                    };
                    if keep {
                        kept.push(text.clone());
                    } else if seen.insert(text.as_str()) {
                        removed.push(text.clone());
                    }
                }
                if mutates(&operation) {
                    items = kept;
                } else {
                    result_items = Some(kept);
                    excluded = Some(removed);
                }
            }
            sort => {
                let mut sorted = items.clone();
                match sort {
                    "sortAsc" | "sortDesc" => {
                        // .NET uses CurrentCulture. Report this deterministic
                        // UTF-16 ordinal fallback in the compatibility report.
                        sorted.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
                        if sort == "sortDesc" {
                            sorted.reverse();
                        }
                    }
                    "sortAscNature" => sorted.sort_by(|a, b| file_selection::natural(a, b)),
                    _ => file_selection::sort_files(&mut sorted, sort, self.control.as_ref())?,
                }
                result_items = Some(sorted);
            }
        }
        let output_list = result_items.as_ref().unwrap_or(&items);
        check_size(output_list)?;
        let count = output_list.len();
        ensure_not_cancelled(self.control.as_ref())?;
        if mutates(&operation) {
            self.vars.insert(variable.unwrap().into(), array(items));
        }
        if let Some(result) = result_items {
            value = Some(array(result));
        }
        if let Some(value) = value {
            self.assign_output(&step.output_params, "value", value)?;
        }
        if let Some(index) = found_index {
            self.assign_output(&step.output_params, "index", Value::from(index))?;
        }
        self.assign_output(&step.output_params, "isEmpty", Value::Bool(count == 0))?;
        self.assign_output(&step.output_params, "length", Value::from(count))?;
        if let Some(items) = excluded {
            self.assign_output(&step.output_params, "filterOutItems", array(items))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
