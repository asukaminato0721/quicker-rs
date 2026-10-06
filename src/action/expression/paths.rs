//! Pure Linux path operations. No filesystem, environment, or current directory.
use super::*;

const MAX_PATH_BYTES: usize = 1024 * 1024;

pub(super) fn supports(name: &str, args: Option<usize>) -> bool {
    matches!(
        (name, args),
        (
            "GetDirectoryName"
                | "GetFileName"
                | "GetFileNameWithoutExtension"
                | "GetExtension"
                | "HasExtension"
                | "GetPathRoot"
                | "IsPathRooted",
            Some(1)
        ) | ("ChangeExtension", Some(2))
            | ("Combine", Some(_))
    )
}

fn check(path: &str) -> Result<(), String> {
    if path.len() > MAX_PATH_BYTES {
        return Err("Path expression exceeds 1 MiB".into());
    }
    if path.contains('\0') {
        return Err("Path contains a NUL character".into());
    }
    if super::super::file_steps::windows_path(path) {
        return Err("Path expression requires a Linux path with forward slashes".into());
    }
    Ok(())
}

fn filename(path: &str) -> &str {
    path.rsplit('/').next().unwrap()
}

fn extension(path: &str) -> &str {
    let file = filename(path);
    file.rfind('.')
        .filter(|&i| i + 1 < file.len())
        .map_or("", |i| &file[i..])
}

fn directory(path: &str) -> Value {
    if path.is_empty() || path == "/" {
        return Value::Null;
    }
    let end = path.rfind('/').unwrap_or(0);
    let root = usize::from(path.starts_with('/'));
    let prefix = &path[..end.max(root)];
    let prefix = prefix.trim_end_matches('/');
    let prefix = if prefix.is_empty() && root == 1 {
        "/"
    } else {
        prefix
    };
    // Collapse separator runs, but keep '.' and '..' segments unchanged.
    let mut result = String::new();
    for c in prefix.chars() {
        if c != '/' || !result.ends_with('/') {
            result.push(c);
        }
    }
    Value::String(result)
}

pub(super) fn evaluate(name: &str, args: Vec<Value>) -> Result<Value, String> {
    if name == "Combine" {
        let parts = if args.len() == 1 && args[0].is_array() {
            args[0].as_array().unwrap().as_slice()
        } else {
            &args
        };
        let mut result = String::new();
        for value in parts {
            let path = text(value)?;
            check(path)?;
            if path.is_empty() {
                continue;
            }
            if path.starts_with('/') {
                result.clear();
            } else if !result.is_empty() && !result.ends_with('/') {
                result.push('/');
            }
            if result.len().saturating_add(path.len()) > MAX_PATH_BYTES {
                return Err("Path expression exceeds 1 MiB".into());
            }
            result.push_str(path);
        }
        return Ok(Value::String(result));
    }
    if name == "ChangeExtension" && !args[1].is_null() {
        text(&args[1])?;
    }
    if args[0].is_null() {
        return Ok(if matches!(name, "HasExtension" | "IsPathRooted") {
            Value::Bool(false)
        } else {
            Value::Null
        });
    }
    let path = text(&args[0])?;
    check(path)?;
    Ok(match name {
        "GetDirectoryName" => directory(path),
        "GetPathRoot" => {
            if path.is_empty() {
                Value::Null
            } else {
                Value::String(if path.starts_with('/') { "/" } else { "" }.into())
            }
        }
        "IsPathRooted" => Value::Bool(path.starts_with('/')),
        "GetFileName" => Value::String(filename(path).into()),
        "GetFileNameWithoutExtension" => {
            let file = filename(path);
            Value::String(file[..file.rfind('.').unwrap_or(file.len())].into())
        }
        "GetExtension" => Value::String(extension(path).into()),
        "HasExtension" => Value::Bool(!extension(path).is_empty()),
        "ChangeExtension" => {
            if path.is_empty() {
                return Ok(Value::String(String::new()));
            }
            let file = filename(path);
            let prefix = &path[..path.len() - file.len() + file.rfind('.').unwrap_or(file.len())];
            if args[1].is_null() {
                return Ok(Value::String(prefix.into()));
            }
            let ext = text(&args[1])?;
            check(ext)?;
            let dot = if ext.starts_with('.') { "" } else { "." };
            if prefix
                .len()
                .saturating_add(dot.len())
                .saturating_add(ext.len())
                > MAX_PATH_BYTES
            {
                return Err("Path expression exceeds 1 MiB".into());
            }
            Value::String(format!("{prefix}{dot}{ext}"))
        }
        _ => return Err("Unsupported Path member".into()),
    })
}

#[cfg(test)]
mod tests;
