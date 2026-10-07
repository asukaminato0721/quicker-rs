use super::*;

pub(super) const LIMIT: usize = 1024 * 1024;
pub(crate) const FIELD_LIMIT: usize = 256 * 1024;
pub(in crate::action) const OPTIONS: &[&str] = &[
    "operation",
    "windowWidth",
    "windowHeight",
    "titleColumnWidth",
    "defaultInputWidth",
    "markdownhelp",
    "confirm",
    "customButtons",
    "selectedGroup",
    "winLocation",
    "winSize",
];

pub(in crate::action) fn validate_option(key: &str, value: &Value) -> bool {
    let text = value_to_string(value);
    match key {
        "operation" => matches!(text.as_str(), "variables" | "dict" | "dict_dynamic"),
        "windowWidth" | "windowHeight" | "titleColumnWidth" | "defaultInputWidth" => {
            dimension(&text, 0.0).is_ok()
        }
        "winLocation" => matches!(text.as_str(), "" | "CenterScreen"),
        _ => text.is_empty(),
    }
}

pub(super) fn dimension(text: &str, default: f32) -> Result<f32, String> {
    if text.is_empty() {
        return Ok(default);
    }
    let value = text
        .trim()
        .parse::<f32>()
        .map_err(|_| "Invalid form dimension")?;
    if !value.is_finite() || !(0.0..=4096.0).contains(&value) {
        return Err("Form dimensions must be between 0 and 4096".into());
    }
    Ok(value)
}

pub(in crate::action) fn definition(value: &Value, dynamic: bool) -> Result<Vec<Value>, String> {
    if value_to_string(value).len() > LIMIT {
        return Err("Form definition exceeds 1 MiB".into());
    }
    let document = if let Some(text) = value.as_str() {
        serde_json::from_str::<Value>(text).map_err(|e| format!("Invalid form JSON: {e}"))?
    } else {
        value.clone()
    };
    let fields = if dynamic {
        document.as_array()
    } else {
        document["Fields"].as_array()
    }
    .ok_or("Form definition requires a field list")?;
    if fields.is_empty() || fields.len() > 128 {
        return Err("A form requires 1 to 128 fields".into());
    }
    let mut keys = BTreeSet::new();
    for field in fields {
        let raw = raw_field(field)?;
        if raw.input_method != 100 && !keys.insert(raw.field_key) {
            return Err("Form field keys must be unique".into());
        }
    }
    Ok(fields.clone())
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct RawField {
    field_key: Option<String>,
    dict_var_type: Option<u8>,
    label: Option<String>,
    help_text: Option<String>,
    input_method: u8,
    selection_items: Option<String>,
    is_required: bool,
    min_value: Option<String>,
    max_value: Option<String>,
    pattern: Option<String>,
    max_length: usize,
    input_width: Option<String>,
    read_only: bool,
    text_tools: Option<String>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

fn raw_field(value: &Value) -> Result<RawField, String> {
    let raw: RawField =
        serde_json::from_value(value.clone()).map_err(|e| format!("Invalid form field: {e}"))?;
    if !matches!(raw.input_method, 1 | 2 | 3 | 6 | 7 | 10 | 41 | 100) {
        return Err(format!(
            "Unsupported form input method: {}",
            raw.input_method
        ));
    }
    if raw.input_method != 100
        && (raw.field_key.as_deref().is_none_or(str::is_empty)
            || raw.label.as_deref().is_none_or(str::is_empty))
    {
        return Err("Form fields require FieldKey and Label".into());
    }
    for (key, value) in &raw.extra {
        let inactive = match key.as_str() {
            "OnlyDate" => value.is_null() || value == &Value::Bool(false),
            "ColumnWidth" => value.is_null() || value.as_f64() == Some(0.0),
            "ImeState" => value.is_null() || matches!(value.as_str(), Some("" | "NO_CONTROL")),
            "HelpLink" | "ExtraSettings" | "VisibleExpression" | "DefaultValue" | "Group" => {
                value.is_null() || value.as_str() == Some("")
            }
            _ => false,
        };
        if !inactive {
            return Err(format!("Unsupported form field option: {key}"));
        }
    }
    if raw.max_length > FIELD_LIMIT {
        return Err("Form MaxLength exceeds 262144".into());
    }
    dimension(raw.input_width.as_deref().unwrap_or(""), 0.0)?;
    if let Some(pattern) = raw.pattern.as_deref().filter(|s| !s.is_empty()) {
        if pattern.starts_with("$=") {
            return Err("Form expression validation is not supported".into());
        }
        regex_steps::compile(pattern, false, false, false)?;
    }
    for range in [&raw.min_value, &raw.max_value] {
        if let Some(value) = range.as_deref().filter(|s| !s.is_empty()) {
            if !value.parse::<f64>().is_ok_and(f64::is_finite) {
                return Err("Invalid form numeric bound".into());
            }
        }
    }
    input_tools::parse(raw.text_tools.as_deref().unwrap_or(""))?;
    if !matches!(raw.input_method, 1 | 2)
        && raw.text_tools.as_deref().is_some_and(|s| !s.is_empty())
    {
        return Err("Text tools require a text field".into());
    }
    if raw.input_method == 3 {
        let text = raw.selection_items.as_deref().unwrap_or("");
        if !text.starts_with("$$") && !text.starts_with("$=") {
            choices(text)?;
        }
    }
    Ok(raw)
}

#[derive(Clone, Debug)]
pub(crate) struct Choice {
    pub title: String,
    pub value: String,
    pub help: String,
}

fn choices(text: &str) -> Result<Vec<Choice>, String> {
    if text.len() > FIELD_LIMIT {
        return Err("Form choices exceed 256 KiB".into());
    }
    let mut result = Vec::new();
    let mut delimiter = "|";
    for line in text
        .lines()
        .filter(|s| !s.is_empty() && !s.starts_with("////"))
    {
        if result.is_empty() {
            if let Some(separator) = line.strip_prefix("|=") {
                if separator.is_empty() {
                    return Err("Form choice delimiter is empty".into());
                }
                delimiter = separator;
                continue;
            }
        }
        let (label, value) = line.split_once(delimiter).unwrap_or((line, line));
        if label.starts_with('[') && line.contains(delimiter) {
            return Err("Form choice icons are not supported".into());
        }
        let (title, help) = if line.contains(delimiter) {
            label
                .strip_suffix(')')
                .and_then(|s| s.rsplit_once('('))
                .filter(|(_, tip)| tip.chars().count() >= 3 && !tip.contains(['(', ')']))
                .unwrap_or((label, ""))
        } else {
            (label, "")
        };
        result.push(Choice {
            title: title.into(),
            value: value.into(),
            help: help.into(),
        });
        if result.len() > 10_000 {
            return Err("Form choices exceed 10000 items".into());
        }
    }
    Ok(result)
}

fn resolve(text: &str, vars: &HashMap<String, Value>) -> Result<String, String> {
    if text.starts_with("$=") {
        expression::evaluate(text, vars).map(|v| form_text(&v))
    } else if let Some(text) = text.strip_prefix("$$") {
        // The MSI's text converter joins a text list with LF, not commas.
        let vars = vars
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(form_text(v))))
            .collect();
        Ok(expand_runtime_vars(text, &vars))
    } else {
        Ok(text.into())
    }
}

fn form_text(value: &Value) -> String {
    match value {
        Value::Array(items) => items.iter().map(form_text).collect::<Vec<_>>().join("\n"),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        _ => value_to_string(value),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Field {
    pub key: String,
    pub label: String,
    pub help: String,
    pub method: u8,
    pub initial: String,
    pub readonly: bool,
    pub choices: Vec<Choice>,
    pub tools: Vec<input_tools::Tool>,
    pub width: f32,
    pub max_length: usize,
    kind: u8,
    original: Value,
    required: bool,
    pattern: Option<Regex>,
    min: Option<f64>,
    max: Option<f64>,
}

impl Field {
    pub(super) fn build(
        value: &Value,
        vars: &HashMap<String, Value>,
        types: &HashMap<String, u8>,
        dict: bool,
    ) -> Result<Self, String> {
        let raw = raw_field(value)?;
        let key = raw.field_key.unwrap_or_default();
        let original = if raw.input_method == 100 {
            Value::Null
        } else {
            vars.get(&key)
                .cloned()
                .ok_or_else(|| format!("Unknown form variable: {key}"))?
        };
        let kind = if dict {
            raw.dict_var_type.unwrap_or(0)
        } else {
            types.get(&key).copied().unwrap_or(0)
        };
        let supported = match raw.input_method {
            1 | 3 => matches!(kind, 0 | 1 | 12),
            2 => matches!(kind, 0 | 4),
            6 => kind == 2,
            7 => matches!(kind, 1 | 12),
            10 => kind == 0,
            41 => matches!(kind, 0 | 1 | 2 | 12),
            100 => true,
            _ => false,
        };
        if !supported {
            return Err(format!("Unsupported form type {kind} for {key}"));
        }
        let choices = if raw.input_method == 3 {
            choices(&resolve(
                raw.selection_items.as_deref().unwrap_or(""),
                vars,
            )?)?
        } else {
            Vec::new()
        };
        let mut initial = if kind == 4 {
            original
                .as_array()
                .ok_or("Form text list requires an array")?
                .iter()
                .map(|v| v.as_str().ok_or("Form list requires text items"))
                .collect::<Result<Vec<_>, _>>()?
                .join("\r\n")
        } else {
            value_to_string(&original)
        };
        if raw.input_method == 3 {
            initial = choices
                .iter()
                .find(|c| c.value == initial)
                .or_else(|| {
                    choices.iter().find(|c| {
                        list_steps::ordinal_fold(&c.value) == list_steps::ordinal_fold(&initial)
                    })
                })
                .map(|c| c.value.clone())
                .unwrap_or_default();
        }
        let min = raw
            .min_value
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<f64>().unwrap());
        let max = raw
            .max_value
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<f64>().unwrap());
        if min.zip(max).is_some_and(|(min, max)| min > max) {
            return Err("Form minimum exceeds maximum".into());
        }
        if (min.is_some() || max.is_some()) && !matches!(kind, 1 | 12) {
            return Err("Numeric bounds require a numeric form variable".into());
        }
        Ok(Self {
            key,
            label: resolve(raw.label.as_deref().unwrap_or(""), vars)?,
            help: resolve(raw.help_text.as_deref().unwrap_or(""), vars)?,
            method: raw.input_method,
            initial,
            original,
            kind,
            readonly: raw.read_only || raw.input_method == 41,
            choices,
            tools: input_tools::parse(raw.text_tools.as_deref().unwrap_or(""))?,
            width: dimension(raw.input_width.as_deref().unwrap_or(""), 0.0)?,
            max_length: raw.max_length,
            required: raw.is_required,
            pattern: raw
                .pattern
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| regex_steps::compile(s, false, false, false))
                .transpose()?,
            min,
            max,
        })
    }

    fn value(&self, text: &str) -> Result<Value, String> {
        if text.len() > FIELD_LIMIT {
            return Err("Field value exceeds 256 KiB".into());
        }
        if self.readonly {
            return Ok(self.original.clone());
        }
        if self.required && (text.is_empty() || (self.method == 6 && !matches!(text, "true" | "1")))
        {
            return Err("A value is required".into());
        }
        if self.max_length > 0 && text.encode_utf16().count() > self.max_length {
            return Err("Text exceeds MaxLength".into());
        }
        if self.method == 3 && !text.is_empty() && !self.choices.iter().any(|c| c.value == text) {
            return Err("Select an available option".into());
        }
        if let Some(pattern) = &self.pattern {
            if !pattern
                .is_match(text)
                .map_err(|e| format!("Form pattern failed: {e}"))?
            {
                return Err("Text does not match the required pattern".into());
            }
        }
        let result = expression::convert(Value::String(text.into()), Some(self.kind))?;
        if let Some(value) = result.as_f64() {
            if self.min.is_some_and(|min| value < min) || self.max.is_some_and(|max| value > max) {
                return Err("Number is outside the allowed range".into());
            }
        }
        Ok(result)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Options {
    pub fields: Vec<Field>,
    pub title: String,
    pub help: String,
    pub width: f32,
    pub height: f32,
    pub label_width: f32,
    pub input_width: f32,
    pub topmost: bool,
    pub disable_enter: bool,
}

impl Options {
    pub(crate) fn check_size(&self) -> Result<(), String> {
        if self.title.len() > 65536
            || self.help.len() > FIELD_LIMIT
            || self.fields.iter().any(|f| {
                f.initial.len() > FIELD_LIMIT || f.label.len() > 65536 || f.help.len() > FIELD_LIMIT
            })
            || self
                .fields
                .iter()
                .map(|f| {
                    f.initial.len()
                        + f.label.len()
                        + f.help.len()
                        + f.choices
                            .iter()
                            .map(|c| c.value.len() + c.title.len() + c.help.len())
                            .sum::<usize>()
                })
                .sum::<usize>()
                > 4 * LIMIT
        {
            return Err("Form content exceeds its size limit".into());
        }
        Ok(())
    }

    pub(crate) fn values(&self, values: &[String]) -> Result<Map<String, Value>, String> {
        if values.len() != self.fields.len()
            || values.iter().map(String::len).sum::<usize>() > 4 * LIMIT
        {
            return Err("Invalid form result size".into());
        }
        let mut result = Map::new();
        for (field, value) in self.fields.iter().zip(values) {
            if field.method != 100 {
                result.insert(
                    field.key.clone(),
                    field
                        .value(value)
                        .map_err(|e| format!("{}: {e}", field.label))?,
                );
            }
        }
        Ok(result)
    }
}
