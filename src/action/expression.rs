//! A pure evaluator for common Quicker `$=` expressions. Unknown syntax fails.
//! It has no host functions, file access, or process access.
use serde_json::{Number, Value};
use std::collections::HashMap;

/// Convert defaults and outputs to the declared Quicker variable type.
pub(super) fn convert(value: Value, kind: Option<u8>) -> Result<Value, String> {
    match kind {
        Some(0) => Ok(Value::String(super::value_to_string(&value))),
        Some(1) => {
            let number = match value {
                Value::String(s) if s.trim().is_empty() => 0.0,
                Value::String(s) => s.trim().parse().map_err(|_| "Invalid number value")?,
                other => number(&other)?,
            };
            finite(number)
        }
        Some(12) => {
            let number = match value {
                Value::String(s) if s.trim().is_empty() => 0,
                Value::String(s) => s
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| "Invalid integer value")?,
                other => other.as_i64().ok_or("Expected integer value")?,
            };
            Ok(Value::from(number))
        }
        Some(2) => match value {
            Value::Bool(_) => Ok(value),
            Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
                "1" | "true" => Ok(Value::Bool(true)),
                "" | "0" | "false" => Ok(Value::Bool(false)),
                _ => Err("Invalid Boolean value".into()),
            },
            _ => Err("Expected Boolean value".into()),
        },
        Some(4) => match value {
            Value::Array(_) => Ok(value),
            Value::String(s) => Ok(Value::Array(
                s.lines().map(|s| Value::String(s.into())).collect(),
            )),
            _ => Err("Expected list or multiline text".into()),
        },
        Some(10) => match value {
            Value::Object(_) => Ok(value),
            Value::String(s) => {
                let s = s.trim();
                if s.starts_with("json:") || s.starts_with('{') {
                    let value: Value = serde_json::from_str(s.strip_prefix("json:").unwrap_or(s))
                        .map_err(|_| "Invalid dictionary JSON")?;
                    if !value.is_object() {
                        return Err("Dictionary JSON must be an object".into());
                    }
                    Ok(value)
                } else {
                    let mut values = serde_json::Map::new();
                    for line in s.lines() {
                        let (key, value) = line
                            .split_once(':')
                            .ok_or("Dictionary line requires key:value")?;
                        values.insert(key.into(), Value::String(value.into()));
                    }
                    Ok(Value::Object(values))
                }
            }
            _ => Err("Expected dictionary or text".into()),
        },
        _ => Ok(value),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Value(Value),
    Variable(String),
    Name(String),
    Symbol(String),
}

#[derive(Debug)]
enum Expr {
    Value(Value),
    Variable(String),
    Name(String),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    Member(Box<Expr>, String, Option<Vec<Expr>>),
    Index(Box<Expr>, Box<Expr>),
}

pub(super) fn evaluate(input: &str, vars: &HashMap<String, Value>) -> Result<Value, String> {
    parse(input)?.eval(vars)
}

pub(super) fn validate(input: &str) -> Result<(), String> {
    parse(input).map(|_| ())
}

fn parse(input: &str) -> Result<Expr, String> {
    let input = input
        .trim()
        .strip_prefix("$=")
        .ok_or("Expected $= expression")?;
    let tokens = lex(input)?;
    let mut parser = Parser {
        tokens,
        at: 0,
        depth: 0,
    };
    let expr = parser.expression(0)?;
    if parser.at != parser.tokens.len() {
        return Err("Unsupported expression syntax after value".into());
    }
    Ok(expr)
}

fn lex(mut input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    while !input.trim_start().is_empty() {
        input = input.trim_start();
        if tokens.len() >= 512 {
            return Err("Expression exceeds 512 tokens".into());
        }
        let ch = input.chars().next().unwrap();
        if ch == '{' {
            let end = input.find('}').ok_or("Unclosed variable reference")?;
            let name = &input[1..end];
            if name.is_empty() {
                return Err("Empty variable reference".into());
            }
            tokens.push(Token::Variable(name.into()));
            input = &input[end + 1..];
        } else if ch == '"' {
            let mut stream = serde_json::Deserializer::from_str(input).into_iter::<String>();
            let text = stream
                .next()
                .ok_or("Missing string")?
                .map_err(|_| "Invalid string literal")?;
            input = &input[stream.byte_offset()..];
            tokens.push(Token::Value(Value::String(text)));
        } else if ch.is_ascii_digit() {
            let end = input
                .char_indices()
                .take_while(|(_, c)| c.is_ascii_digit() || *c == '.')
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap();
            let number = input[..end]
                .parse::<Number>()
                .map_err(|_| "Invalid number literal")?;
            tokens.push(Token::Value(Value::Number(number)));
            input = &input[end..];
        } else if ch.is_alphabetic() || ch == '_' {
            let end = input
                .char_indices()
                .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap();
            tokens.push(match &input[..end] {
                "true" => Token::Value(Value::Bool(true)),
                "false" => Token::Value(Value::Bool(false)),
                "null" => Token::Value(Value::Null),
                name => Token::Name(name.into()),
            });
            input = &input[end..];
        } else {
            let symbol = ["&&", "||", "==", "!=", "<=", ">=", "??"]
                .into_iter()
                .find(|symbol| input.starts_with(symbol));
            let symbol = match symbol {
                Some(symbol) => symbol,
                None if "()+-*/%!<>?:.,[]".contains(ch) => &input[..ch.len_utf8()],
                _ => return Err(format!("Unsupported expression character: {ch}")),
            };
            tokens.push(Token::Symbol(symbol.into()));
            input = &input[symbol.len()..];
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
    depth: usize,
}

impl Parser {
    fn take(&mut self, symbol: &str) -> bool {
        if self.tokens.get(self.at) == Some(&Token::Symbol(symbol.into())) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn require(&mut self, symbol: &str) -> Result<(), String> {
        if self.take(symbol) {
            Ok(())
        } else {
            Err(format!("Expected {symbol}"))
        }
    }

    fn expression(&mut self, min: u8) -> Result<Expr, String> {
        self.depth += 1;
        if self.depth > 64 {
            return Err("Expression exceeds 64 nesting levels".into());
        }
        let mut left = if self.take("(") {
            let expr = self.expression(0)?;
            self.require(")")?;
            expr
        } else if let Some(Token::Symbol(op)) = self
            .tokens
            .get(self.at)
            .cloned()
            .filter(|t| matches!(t, Token::Symbol(s) if ["!", "-", "+"].contains(&s.as_str())))
        {
            self.at += 1;
            Expr::Unary(op, Box::new(self.expression(8)?))
        } else {
            let token = self
                .tokens
                .get(self.at)
                .ok_or("Expected expression value")?
                .clone();
            self.at += 1;
            match token {
                Token::Value(value) => Expr::Value(value),
                Token::Variable(name) => Expr::Variable(name),
                Token::Name(name)
                    if ["String", "string", "StringComparison"].contains(&name.as_str()) =>
                {
                    Expr::Name(name)
                }
                _ => return Err("Unsupported expression value or function".into()),
            }
        };
        loop {
            if self.take(".") {
                let Some(Token::Name(name)) = self.tokens.get(self.at).cloned() else {
                    return Err("Expected member name".into());
                };
                self.at += 1;
                let args = if self.take("(") {
                    let mut args = Vec::new();
                    if !self.take(")") {
                        loop {
                            args.push(self.expression(0)?);
                            if self.take(")") {
                                break;
                            }
                            self.require(",")?;
                        }
                    }
                    Some(args)
                } else {
                    None
                };
                validate_member(&left, &name, args.as_ref().map(Vec::len))?;
                left = Expr::Member(Box::new(left), name, args);
                continue;
            }
            if self.take("[") {
                let index = self.expression(0)?;
                self.require("]")?;
                left = Expr::Index(Box::new(left), Box::new(index));
                continue;
            }
            if min == 0 && self.take("?") {
                let yes = self.expression(0)?;
                self.require(":")?;
                let no = self.expression(0)?;
                left = Expr::Conditional(Box::new(left), Box::new(yes), Box::new(no));
                continue;
            }
            let Some(Token::Symbol(op)) = self.tokens.get(self.at) else {
                break;
            };
            let precedence = match op.as_str() {
                "??" => 1,
                "||" => 2,
                "&&" => 3,
                "==" | "!=" => 4,
                "<" | "<=" | ">" | ">=" => 5,
                "+" | "-" => 6,
                "*" | "/" | "%" => 7,
                _ => break,
            };
            if precedence < min {
                break;
            }
            let op = op.clone();
            self.at += 1;
            let right = self.expression(if op == "??" {
                precedence
            } else {
                precedence + 1
            })?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        self.depth -= 1;
        Ok(left)
    }
}

fn validate_member(receiver: &Expr, name: &str, args: Option<usize>) -> Result<(), String> {
    let valid = match receiver {
        Expr::Name(class) if class == "String" || class == "string" => matches!(
            (name, args),
            ("IsNullOrEmpty" | "IsNullOrWhiteSpace", Some(1)) | ("Empty", None)
        ),
        Expr::Name(class) if class == "StringComparison" => {
            args.is_none() && matches!(name, "Ordinal" | "OrdinalIgnoreCase")
        }
        _ => matches!(
            (name, args),
            ("Length" | "Count", None)
                | (
                    "Contains" | "StartsWith" | "EndsWith" | "IndexOf",
                    Some(1 | 2)
                )
                | (
                    "ToLower" | "ToUpper" | "ToLowerInvariant" | "ToUpperInvariant" | "Trim",
                    Some(0)
                )
                | ("Replace", Some(2))
                | ("Substring", Some(1 | 2))
        ),
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "Unsupported expression member or argument count: {name}"
        ))
    }
}

fn boolean(value: &Value) -> Result<bool, String> {
    value
        .as_bool()
        .ok_or_else(|| "Expression requires a Boolean value".into())
}

fn number(value: &Value) -> Result<f64, String> {
    value
        .as_f64()
        .ok_or_else(|| "Expression requires a Number value".into())
}

fn text(value: &Value) -> Result<&str, String> {
    value
        .as_str()
        .ok_or_else(|| "Expression requires a Text value".into())
}

fn finite(value: f64) -> Result<Value, String> {
    Number::from_f64(value)
        .map(Value::Number)
        .ok_or_else(|| "Non-finite expression result".into())
}

impl Expr {
    fn eval(&self, vars: &HashMap<String, Value>) -> Result<Value, String> {
        match self {
            Self::Value(value) => Ok(value.clone()),
            Self::Variable(name) => vars
                .get(name)
                .cloned()
                .ok_or_else(|| format!("Unknown variable: {name}")),
            Self::Name(_) => Err("Expected a supported static member".into()),
            Self::Unary(op, expr) => {
                let value = expr.eval(vars)?;
                match op.as_str() {
                    "!" => Ok(Value::Bool(!boolean(&value)?)),
                    "+" => {
                        number(&value)?;
                        Ok(value)
                    }
                    "-" => {
                        if let Some(n) = value.as_i64() {
                            n.checked_neg()
                                .map(Value::from)
                                .ok_or_else(|| "Integer overflow".into())
                        } else {
                            finite(-number(&value)?)
                        }
                    }
                    _ => unreachable!(),
                }
            }
            Self::Binary(op, lhs, rhs) => {
                let left = lhs.eval(vars)?;
                // Do not evaluate a skipped branch. This also preserves null guards.
                if op == "&&" && !boolean(&left)? {
                    return Ok(Value::Bool(false));
                }
                if op == "||" && boolean(&left)? {
                    return Ok(Value::Bool(true));
                }
                if op == "??" && !left.is_null() {
                    return Ok(left);
                }
                let right = rhs.eval(vars)?;
                binary(op, left, right)
            }
            Self::Conditional(condition, yes, no) => {
                if boolean(&condition.eval(vars)?)? {
                    yes.eval(vars)
                } else {
                    no.eval(vars)
                }
            }
            Self::Index(receiver, index) => {
                let value = receiver.eval(vars)?;
                let index = index.eval(vars)?;
                if let Some(values) = value.as_array() {
                    let index = index
                        .as_u64()
                        .and_then(|i| usize::try_from(i).ok())
                        .ok_or("Invalid list index")?;
                    values
                        .get(index)
                        .cloned()
                        .ok_or_else(|| "List index is out of range".into())
                } else if let Some(values) = value.as_object() {
                    values
                        .get(text(&index)?)
                        .cloned()
                        .ok_or_else(|| "Dictionary key does not exist".into())
                } else {
                    Err("Expression index requires a list or dictionary".into())
                }
            }
            Self::Member(receiver, name, args) => {
                let args = args
                    .as_ref()
                    .map(|a| {
                        a.iter()
                            .map(|a| a.eval(vars))
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                if let Expr::Name(class) = receiver.as_ref() {
                    return match (class.as_str(), name.as_str()) {
                        ("String" | "string", "IsNullOrEmpty") => {
                            Ok(Value::Bool(args[0].is_null() || text(&args[0])?.is_empty()))
                        }
                        ("String" | "string", "IsNullOrWhiteSpace") => Ok(Value::Bool(
                            args[0].is_null() || text(&args[0])?.trim().is_empty(),
                        )),
                        ("String" | "string", "Empty") => Ok(Value::String(String::new())),
                        ("StringComparison", _) => Ok(Value::String(name.clone())),
                        _ => Err("Unsupported static member".into()),
                    };
                }
                member(receiver.eval(vars)?, name, args)
            }
        }
    }
}

fn binary(op: &str, left: Value, right: Value) -> Result<Value, String> {
    match op {
        "&&" | "||" => Ok(Value::Bool(boolean(&right)?)),
        "??" => Ok(right),
        "==" | "!=" => {
            let equal = if let (Some(a), Some(b)) = (left.as_i64(), right.as_i64()) {
                a == b
            } else if left.is_number() && right.is_number() {
                number(&left)? == number(&right)?
            } else {
                left == right
            };
            Ok(Value::Bool(if op == "==" { equal } else { !equal }))
        }
        "<" | "<=" | ">" | ">=" => {
            let order = if let (Some(a), Some(b)) = (left.as_i64(), right.as_i64()) {
                a.cmp(&b)
            } else {
                number(&left)?
                    .partial_cmp(&number(&right)?)
                    .ok_or("Invalid numeric comparison")?
            };
            Ok(Value::Bool(match op {
                "<" => order.is_lt(),
                "<=" => order.is_le(),
                ">" => order.is_gt(),
                _ => order.is_ge(),
            }))
        }
        "+" if left.is_string() || right.is_string() => Ok(Value::String(
            super::value_to_string(&left) + &super::value_to_string(&right),
        )),
        "+" | "-" | "*" | "/" | "%" => {
            if let (Some(a), Some(b)) = (left.as_i64(), right.as_i64()) {
                let value = match op {
                    "+" => a.checked_add(b),
                    "-" => a.checked_sub(b),
                    "*" => a.checked_mul(b),
                    "/" => a.checked_div(b),
                    _ => a.checked_rem(b),
                };
                value
                    .map(Value::from)
                    .ok_or_else(|| "Integer overflow or division by zero".into())
            } else {
                let (a, b) = (number(&left)?, number(&right)?);
                finite(match op {
                    "+" => a + b,
                    "-" => a - b,
                    "*" => a * b,
                    "/" => a / b,
                    _ => a % b,
                })
            }
        }
        _ => Err("Unsupported expression operator".into()),
    }
}

fn member(receiver: Value, name: &str, args: Vec<Value>) -> Result<Value, String> {
    if matches!(name, "Length" | "Count") {
        return match receiver {
            Value::String(s) if name == "Length" => Ok(Value::from(s.encode_utf16().count())),
            Value::Array(v) => Ok(Value::from(v.len())),
            Value::Object(v) if name == "Count" => Ok(Value::from(v.len())),
            _ => Err("Length or Count is not valid for this value".into()),
        };
    }
    if let Value::Array(values) = &receiver {
        if name == "Contains" && args.len() == 1 {
            return Ok(Value::Bool(values.contains(&args[0])));
        }
    }
    let source = text(&receiver)?;
    match name {
        "Trim" => Ok(Value::String(source.trim().into())),
        "ToLower" | "ToLowerInvariant" => Ok(Value::String(source.to_lowercase())),
        "ToUpper" | "ToUpperInvariant" => Ok(Value::String(source.to_uppercase())),
        "Replace" => {
            let old = text(&args[0])?;
            if old.is_empty() {
                return Err("Replace requires nonempty old text".into());
            }
            Ok(Value::String(source.replace(
                old,
                if args[1].is_null() {
                    ""
                } else {
                    text(&args[1])?
                },
            )))
        }
        "Substring" => {
            let start = args[0]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or("Invalid substring start")?;
            let units: Vec<_> = source.encode_utf16().collect();
            let end = if args.len() == 2 {
                let len = args[1]
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or("Invalid substring length")?;
                start.checked_add(len).ok_or("Substring range overflow")?
            } else {
                units.len()
            };
            let slice = units.get(start..end).ok_or("Substring is out of range")?;
            String::from_utf16(slice)
                .map(Value::String)
                .map_err(|_| "Substring splits a UTF-16 surrogate pair".into())
        }
        "Contains" | "StartsWith" | "EndsWith" | "IndexOf" => {
            let needle = text(&args[0])?;
            let comparison = args.get(1).map(text).transpose()?.unwrap_or("Ordinal");
            let (source_cmp, needle_cmp) = match comparison {
                "Ordinal" => (source.to_string(), needle.to_string()),
                "OrdinalIgnoreCase" if source.is_ascii() && needle.is_ascii() => {
                    (source.to_ascii_lowercase(), needle.to_ascii_lowercase())
                }
                "OrdinalIgnoreCase" => {
                    return Err("Non-ASCII OrdinalIgnoreCase is not implemented".into())
                }
                _ => return Err("Unsupported string comparison mode".into()),
            };
            Ok(match name {
                "Contains" => Value::Bool(source_cmp.contains(&needle_cmp)),
                "StartsWith" => Value::Bool(source_cmp.starts_with(&needle_cmp)),
                "EndsWith" => Value::Bool(source_cmp.ends_with(&needle_cmp)),
                _ => Value::from(
                    source_cmp
                        .find(&needle_cmp)
                        .map(|i| source_cmp[..i].encode_utf16().count() as i64)
                        .unwrap_or(-1),
                ),
            })
        }
        _ => Err(format!("Unsupported member: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn evaluates_conditions_arithmetic_and_typed_values() {
        let vars = HashMap::from([
            ("次数".into(), json!(3)),
            ("ok".into(), json!(false)),
            ("items".into(), json!(["a", "b"])),
        ]);
        for (expr, expected) in [
            ("$= {次数} >= 0 && {次数} <= 100", json!(true)),
            ("$= !{ok} && (2 + 3 * 4 == 14)", json!(true)),
            ("$= {items}[1]", json!("b")),
            ("$= {次数} + 1", json!(4)),
            ("$= {ok} ? 10 : 20", json!(20)),
            ("$= null ?? 7", json!(7)),
        ] {
            assert_eq!(evaluate(expr, &vars).unwrap(), expected, "{expr}");
        }
    }

    #[test]
    fn short_circuit_skips_missing_variables_and_bad_indexes() {
        let vars = HashMap::new();
        assert_eq!(
            evaluate("$= false && {missing}[8] == 1", &vars).unwrap(),
            json!(false)
        );
        assert_eq!(
            evaluate("$= true || {missing}", &vars).unwrap(),
            json!(true)
        );
        assert_eq!(
            evaluate("$= true ? 1 : {missing}", &vars).unwrap(),
            json!(1)
        );
    }

    #[test]
    fn supports_real_citavi_condition_and_utf16_offsets() {
        let vars = HashMap::from([
            ("matchName".into(), json!("annotation")),
            ("text".into(), json!("😀世界")),
        ]);
        assert_eq!(
            evaluate(
                "$= {matchName}.IndexOf(\"Annot\", StringComparison.OrdinalIgnoreCase) >= 0",
                &vars
            )
            .unwrap(),
            json!(true)
        );
        assert_eq!(evaluate("$= {text}.Length", &vars).unwrap(), json!(4));
        assert_eq!(
            evaluate("$= {text}.IndexOf(\"世\")", &vars).unwrap(),
            json!(2)
        );
        assert_eq!(
            evaluate("$= {text}.Substring(2, 1)", &vars).unwrap(),
            json!("世")
        );
        assert_eq!(
            evaluate("$= String.IsNullOrEmpty(null)", &vars).unwrap(),
            json!(true)
        );
    }

    #[test]
    fn rejects_unknown_code_and_errors_without_truthy_fallback() {
        for expr in [
            "$= System.IO.File.Delete(\"x\")",
            "$= {x}.Unknown()",
            "$= 1 / 0",
            "$= 1 +",
            "$= true && 1",
            "$= {missing}",
            "$= \"unterminated",
            "$= 9223372036854775807 + 1",
        ] {
            assert!(evaluate(expr, &HashMap::new()).is_err(), "{expr}");
        }
        assert!(validate(&format!("$= {}true{}", "(".repeat(65), ")".repeat(65))).is_err());
    }

    #[test]
    fn integer_comparisons_do_not_lose_precision() {
        assert_eq!(
            evaluate("$= 9007199254740993 > 9007199254740992", &HashMap::new()).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            evaluate("$= 9007199254740993 == 9007199254740992", &HashMap::new()).unwrap(),
            Value::Bool(false)
        );
    }
}
