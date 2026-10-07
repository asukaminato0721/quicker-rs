//! File modules verified against Quicker 1.45.5.0 and the OpenCC author export.
use super::*;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub(super) enum Encoding {
    Utf8,
    Utf16(bool), // Big endian when true.
    Utf32(bool),
    Ascii,
}

impl Encoding {
    pub(super) fn parse(name: &str) -> Result<Self, String> {
        match name.to_ascii_lowercase().as_str() {
            "" | "utf-8" | "utf8" => Ok(Self::Utf8),
            "utf-16" | "unicode" | "utf-16le" => Ok(Self::Utf16(false)),
            "utf-16be" | "unicodefffe" => Ok(Self::Utf16(true)),
            "utf-32" | "utf-32le" => Ok(Self::Utf32(false)),
            "utf-32be" => Ok(Self::Utf32(true)),
            "us-ascii" | "ascii" => Ok(Self::Ascii),
            _ => Err(format!("Unsupported text file encoding: {name}")),
        }
    }

    pub(super) fn preamble(self, utf8_bom: bool) -> &'static [u8] {
        match self {
            Self::Utf8 if utf8_bom => b"\xef\xbb\xbf",
            Self::Utf16(false) => b"\xff\xfe",
            Self::Utf16(true) => b"\xfe\xff",
            Self::Utf32(false) => b"\xff\xfe\0\0",
            Self::Utf32(true) => b"\0\0\xfe\xff",
            _ => b"",
        }
    }

    pub(super) fn encode(self, text: &str) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        match self {
            Self::Utf8 => bytes.extend_from_slice(text.as_bytes()),
            Self::Utf16(big) => {
                for unit in text.encode_utf16() {
                    bytes.extend_from_slice(&if big {
                        unit.to_be_bytes()
                    } else {
                        unit.to_le_bytes()
                    });
                }
            }
            Self::Utf32(big) => {
                for ch in text.chars() {
                    let unit = ch as u32;
                    bytes.extend_from_slice(&if big {
                        unit.to_be_bytes()
                    } else {
                        unit.to_le_bytes()
                    });
                }
            }
            Self::Ascii => {
                // Reject unrepresentable text instead of silently losing user data.
                if !text.is_ascii() {
                    return Err("Text contains characters outside the ASCII encoding".into());
                }
                bytes.extend_from_slice(text.as_bytes());
            }
        }
        if bytes.len() > MAX_TEXT_BYTES {
            return Err("Encoded text exceeds the 16 MiB file limit".into());
        }
        Ok(bytes)
    }

    fn decode(self, bytes: &[u8]) -> Result<String, String> {
        // StreamReader detects a Unicode BOM even when another encoding was selected.
        let (encoding, bytes) = [
            (Self::Utf32(false), b"\xff\xfe\0\0".as_slice()),
            (Self::Utf32(true), b"\0\0\xfe\xff".as_slice()),
            (Self::Utf8, b"\xef\xbb\xbf".as_slice()),
            (Self::Utf16(false), b"\xff\xfe".as_slice()),
            (Self::Utf16(true), b"\xfe\xff".as_slice()),
        ]
        .into_iter()
        .find_map(|(encoding, bom)| bytes.strip_prefix(bom).map(|rest| (encoding, rest)))
        .unwrap_or((self, bytes));
        let invalid = || "File contains invalid bytes for its text encoding".to_string();
        let text = match encoding {
            Self::Utf8 => String::from_utf8(bytes.to_vec()).map_err(|_| invalid())?,
            Self::Utf16(big) => {
                if !bytes.len().is_multiple_of(2) {
                    return Err(invalid());
                }
                let units: Vec<_> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| {
                        if big {
                            u16::from_be_bytes([b[0], b[1]])
                        } else {
                            u16::from_le_bytes([b[0], b[1]])
                        }
                    })
                    .collect();
                String::from_utf16(&units).map_err(|_| invalid())?
            }
            Self::Utf32(big) => {
                if !bytes.len().is_multiple_of(4) {
                    return Err(invalid());
                }
                bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| {
                        let b = [b[0], b[1], b[2], b[3]];
                        char::from_u32(if big {
                            u32::from_be_bytes(b)
                        } else {
                            u32::from_le_bytes(b)
                        })
                        .ok_or_else(invalid)
                    })
                    .collect::<Result<String, _>>()?
            }
            Self::Ascii => {
                if !bytes.is_ascii() {
                    return Err(invalid());
                }
                String::from_utf8(bytes.to_vec()).map_err(|_| invalid())?
            }
        };
        if text.len() > MAX_TEXT_BYTES {
            return Err("Decoded text exceeds the 16 MiB file limit".into());
        }
        Ok(text)
    }
}

pub(super) fn validate_option(key: &str, value: &str) -> bool {
    match key {
        "encoding" => Encoding::parse(value).is_ok(),
        "newLineChars" => matches!(value, "" | "\r\n" | "\r" | "\n"),
        _ => true,
    }
}

impl QuickerRuntime {
    fn finish_file_step(
        &mut self,
        step: &QuickerPluginStepDocument,
        result: Result<(), String>,
    ) -> Result<StepFlow, String> {
        ensure_not_cancelled(self.control.as_ref())?;
        self.assign_output(
            &step.output_params,
            "isSuccess",
            Value::Bool(result.is_ok()),
        )?;
        let stop = self
            .input_value(&step.input_params, "stopIfFail")?
            .is_none_or(|v| truthy(Some(&v)));
        if stop {
            result?;
        }
        Ok(StepFlow::Continue)
    }

    pub(super) fn run_read_file(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        ensure_not_cancelled(self.control.as_ref())?;
        let result = (|| {
            let path = self.input_string(&step.input_params, "path")?;
            let path = expand_environment(&path);
            let kind = self
                .input_string_opt(&step.input_params, "type")?
                .unwrap_or_else(|| "text".into());
            let (output, text) = match kind.as_str() {
                "text" => {
                    let encoding = self
                        .input_string_opt(&step.input_params, "encoding")?
                        .unwrap_or_default();
                    let encoding = Encoding::parse(&encoding)?;
                    (
                        "txt",
                        encoding.decode(&read_text_bytes(&path, self.control.as_ref())?)?,
                    )
                }
                "image" => ("image", read_file_path_reference(&path)?),
                _ => return Err(format!("Unsupported readFile type: {kind}")),
            };
            ensure_not_cancelled(self.control.as_ref())?;
            self.assign_output(&step.output_params, output, Value::String(text))
        })();
        self.finish_file_step(step, result)
    }

    pub(super) fn run_write_text_file(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        ensure_not_cancelled(self.control.as_ref())?;
        let result = (|| {
            let path = self.input_string(&step.input_params, "filePath")?;
            let mut content = self
                .input_string_opt(&step.input_params, "content")?
                .unwrap_or_default();
            if content.len() > MAX_TEXT_BYTES {
                return Err("Text exceeds the 16 MiB file limit".into());
            }
            let new_line = self
                .input_string_opt(&step.input_params, "newLineChars")?
                .unwrap_or_default();
            if !validate_option("newLineChars", &new_line) {
                return Err("Unsupported WriteTextFile newLineChars".into());
            }
            if !new_line.is_empty() {
                content = content
                    .replace("\r\n", "\n")
                    .replace('\r', "\n")
                    .replace('\n', &new_line);
            }
            if self.input_bool(&step.input_params, "addNewLine")? {
                // Retain the Windows module default for imported workflows.
                content.push_str(if new_line.is_empty() {
                    "\r\n"
                } else {
                    &new_line
                });
            }
            let encoding = Encoding::parse(
                &self
                    .input_string_opt(&step.input_params, "encoding")?
                    .unwrap_or_default(),
            )?;
            let bytes = encoding.encode(&content)?;
            // MSI retains UTF-8 BOMs for PowerShell scripts even when addUtf8Bom is false.
            let bom = self.input_bool(&step.input_params, "addUtf8Bom")?
                || path.to_ascii_lowercase().ends_with(".ps1");
            let append = self.input_bool(&step.input_params, "appendMode")?;
            write_text_bytes(
                &path,
                &bytes,
                encoding.preamble(bom),
                append,
                self.control.as_ref(),
            )
        })();
        self.finish_file_step(step, result)
    }
}

fn expand_environment(text: &str) -> String {
    let mut result = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        result.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest[1..].find('%').map(|n| n + 1) else {
            break;
        };
        result.push_str(&std::env::var(&rest[1..end]).unwrap_or_else(|_| rest[..=end].into()));
        rest = &rest[end + 1..];
    }
    result.push_str(rest);
    result
}

#[cfg(not(target_arch = "wasm32"))]
fn open_regular(path: &str, write: bool, append: bool) -> Result<fs::File, String> {
    let mut options = fs::OpenOptions::new();
    options
        .read(!write)
        .write(write)
        .create(write)
        .append(append);
    // Open without truncation. Reject FIFOs/devices before reading or writing.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY);
    }
    let file = options
        .open(path)
        .map_err(|e| format!("Cannot open file {path:?}: {e}"))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Text file operations require a regular file".into());
    }
    Ok(file)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_text_bytes(
    path: &str,
    control: Option<&ActionExecutionControl>,
) -> Result<Vec<u8>, String> {
    ensure_not_cancelled(control)?;
    validate_path(path)?;
    let mut file = open_regular(path, false, false)?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_TEXT_BYTES as u64 {
        return Err("File exceeds the 16 MiB text limit".into());
    }
    let mut bytes = Vec::new();
    let mut chunk = [0; 64 * 1024];
    loop {
        ensure_not_cancelled(control)?;
        let count = file
            .read(&mut chunk)
            .map_err(|e| format!("Cannot read file: {e}"))?;
        if count == 0 {
            break;
        }
        if bytes.len() + count > MAX_TEXT_BYTES {
            return Err("File exceeds the 16 MiB text limit".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(bytes)
}

#[cfg(not(target_arch = "wasm32"))]
fn write_text_bytes(
    path: &str,
    bytes: &[u8],
    bom: &[u8],
    append: bool,
    control: Option<&ActionExecutionControl>,
) -> Result<(), String> {
    use std::io::Write;
    ensure_not_cancelled(control)?;
    validate_path(path)?;
    if bytes.len() + bom.len() > MAX_TEXT_BYTES {
        return Err("Encoded text and BOM exceed the 16 MiB file limit".into());
    }
    if let Some(parent) = Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create file directory: {e}"))?;
    }
    let mut file = open_regular(path, true, append)?;
    ensure_not_cancelled(control)?;
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    if append && length > MAX_TEXT_BYTES.saturating_sub(bytes.len()) as u64 {
        return Err("Append would exceed the 16 MiB text file limit".into());
    }
    if !append {
        file.set_len(0)
            .map_err(|e| format!("Cannot truncate file: {e}"))?;
    }
    if !append || length == 0 {
        file.write_all(bom)
            .map_err(|e| format!("Cannot write BOM: {e}"))?;
    }
    for chunk in bytes.chunks(64 * 1024) {
        ensure_not_cancelled(control)?;
        file.write_all(chunk)
            .map_err(|e| format!("Cannot write text: {e}"))?;
    }
    file.flush()
        .map_err(|e| format!("Cannot flush text file: {e}"))
}

fn validate_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("Text file path is empty".into());
    }
    if cfg!(not(target_os = "windows")) && windows_path(path) {
        return Err("Text file path requires a native path with forward slashes".into());
    }
    Ok(())
}

pub(super) fn windows_path(path: &str) -> bool {
    path.contains('\\')
        || (path.as_bytes().get(1) == Some(&b':') && path.as_bytes()[0].is_ascii_alphabetic())
}

#[cfg(target_arch = "wasm32")]
fn read_text_bytes(_: &str, _: Option<&ActionExecutionControl>) -> Result<Vec<u8>, String> {
    Err("Text file reads are unavailable in the web preview".into())
}

#[cfg(target_arch = "wasm32")]
fn write_text_bytes(
    _: &str,
    _: &[u8],
    _: &[u8],
    _: bool,
    _: Option<&ActionExecutionControl>,
) -> Result<(), String> {
    Err("Text file writes are unavailable in the web preview".into())
}
