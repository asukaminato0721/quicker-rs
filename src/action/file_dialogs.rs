//! Native file selection for imported Quicker workflows.
use super::*;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Filter {
    name: String,
    patterns: Vec<String>,
}

fn filters(text: &str, extension: &str) -> Result<Vec<Filter>, String> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    if text.len() > 64 * 1024 || text.chars().any(|c| c.is_control() || c == '\\') {
        return Err("Invalid file filter text".into());
    }
    let parts: Vec<_> = text.split('|').collect();
    if !parts.len().is_multiple_of(2) {
        return Err("File filters require name|pattern pairs".into());
    }
    let mut result = Vec::new();
    for pair in parts.as_chunks::<2>().0 {
        let patterns: Vec<_> = pair[1]
            .split(';')
            .map(str::trim)
            .map(|p| if p == "*.*" { "*" } else { p })
            .map(str::to_owned)
            .collect();
        if pair[0].is_empty()
            || patterns.iter().any(|p| {
                !p.contains('*') || p.chars().any(|c| c.is_whitespace() || "/[]()".contains(c))
            })
        {
            return Err("Unsupported file filter pattern".into());
        }
        result.push(Filter {
            name: pair[0].into(),
            patterns,
        });
    }
    // MSI selects the first filter whose pattern ends with defaultExt.
    if !extension.is_empty() {
        if let Some(index) = result.iter().position(|f| {
            f.patterns.last().is_some_and(|p| {
                p.to_ascii_lowercase()
                    .ends_with(&extension.to_ascii_lowercase())
            })
        }) {
            let preferred = result.remove(index);
            result.insert(0, preferred);
        }
    }
    Ok(result)
}

pub(super) fn validate_option(key: &str, text: &str) -> bool {
    match key {
        "type" => matches!(text, "openFile" | "openMultiFile" | "saveFile"),
        "filter" => filters(text, "").is_ok(),
        "defaultExt" => {
            text.len() <= 255
                && !text
                    .chars()
                    .any(|c| c.is_control() || c.is_whitespace() || "/\\:*?|<>".contains(c))
        }
        "initDir" | "initFileName" => !file_steps::windows_path(text) && !text.contains('\0'),
        _ => true,
    }
}

struct FileDialog {
    kind: String,
    title: String,
    initial: String,
    filters: Vec<Filter>,
    extension: String,
    top_most: bool,
}

#[cfg(target_os = "linux")]
pub(super) fn choose_text_tool_file(
    tool: input_tools::Tool,
    current: &str,
    control: &ActionExecutionControl,
) -> Result<String, String> {
    let current = if tool == input_tools::Tool::Files {
        current.rsplit('\n').next().unwrap_or_default()
    } else {
        current
    }
    .trim();
    let initial = if Path::new(current).is_file() {
        Path::new(current)
            .parent()
            .unwrap_or(Path::new("/"))
            .to_string_lossy()
            .into_owned()
            + "/"
    } else {
        String::new()
    };
    choose_files(
        &FileDialog {
            kind: match tool {
                input_tools::Tool::Files => "openMultiFile",
                input_tools::Tool::Save => "saveFile",
                _ => "openFile",
            }
            .into(),
            title: tool.label().into(),
            initial,
            filters: Vec::new(),
            extension: String::new(),
            top_most: false,
        },
        Some(control),
    )
    .map(|(path, paths)| {
        if tool == input_tools::Tool::Files {
            paths.join("\r\n")
        } else {
            path
        }
    })
}

impl QuickerRuntime {
    fn file_dialog_options(&self, step: &QuickerPluginStepDocument) -> Result<FileDialog, String> {
        let input = |key, default: &str| {
            self.input_string_opt(&step.input_params, key)
                .map(|v| v.unwrap_or_else(|| default.into()))
        };
        for key in ["type", "filter", "defaultExt", "initDir", "initFileName"] {
            if let Some(value) = self.input_string_opt(&step.input_params, key)? {
                if !validate_option(key, &value) {
                    return Err(format!("Unsupported selectFile option: {key}"));
                }
            }
        }
        let kind = input("type", "openFile")?;
        let directory = input("initDir", "")?;
        let name = input("initFileName", "")?;
        let mut initial = if name.is_empty() {
            directory.clone()
        } else {
            Path::new(&directory)
                .join(&name)
                .to_string_lossy()
                .into_owned()
        };
        if !initial.is_empty() && !Path::new(&initial).is_absolute() {
            initial = std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(initial)
                .to_string_lossy()
                .into_owned();
        }
        if name.is_empty() && !initial.is_empty() && !initial.ends_with('/') {
            initial.push('/');
        }
        let default_ext = input("defaultExt", ".txt")?;
        let extension = default_ext.trim_start_matches('.').to_owned();
        let filters = filters(
            &input("filter", "文本文件|*.txt|所有文件|*.*")?,
            &default_ext,
        )?;
        let title = input("title", "")?;
        Ok(FileDialog {
            title: if title.is_empty() {
                if kind == "saveFile" {
                    "Save file"
                } else {
                    "Select file"
                }
                .into()
            } else {
                title
            },
            kind,
            initial,
            filters,
            extension,
            top_most: self
                .input_value(&step.input_params, "topMost")?
                .is_none_or(|v| truthy(Some(&v))),
        })
    }

    pub(super) fn run_file_dialog(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        ensure_not_cancelled(self.control.as_ref())?;
        let stop = self
            .input_value(&step.input_params, "stopIfFail")?
            .is_none_or(|v| truthy(Some(&v)));
        let _session = dialogs::DialogSession::new(self.control.as_ref());
        let result = self
            .file_dialog_options(step)
            .and_then(|options| choose_files(&options, self.control.as_ref()));
        ensure_not_cancelled(self.control.as_ref())?;
        self.assign_output(
            &step.output_params,
            "isSuccess",
            Value::Bool(result.is_ok()),
        )?;
        // MSI writes both output slots only on success. Failed selection retains previous values.
        match result {
            Ok((path, paths)) => {
                self.assign_output(&step.output_params, "path", Value::String(path))?;
                self.assign_output(
                    &step.output_params,
                    "pathList",
                    Value::Array(paths.into_iter().map(Value::String).collect()),
                )?;
            }
            Err(error) if stop => return Err(error),
            Err(_) => {}
        }
        Ok(StepFlow::Continue)
    }
}

#[cfg(target_os = "linux")]
fn command(options: &FileDialog, backend: &str, separator: &str) -> Command {
    let mut command = Command::new(backend);
    command.args(["--title", &options.title]);
    if backend == "kdialog" {
        // Keep the chooser in the managed child so cancellation also closes its window.
        command.env("QT_QPA_PLATFORMTHEME", "generic");
        command.arg(if options.kind == "saveFile" {
            "--getsaveurl"
        } else {
            "--getopenurl"
        });
        if options.kind == "openMultiFile" {
            command.args(["--multiple", "--separate-output"]);
        }
        let filter = options
            .filters
            .iter()
            .map(|f| format!("{} ({})", f.name, f.patterns.join(" ")))
            .collect::<Vec<_>>()
            .join("\n");
        command.arg("--").arg(&options.initial).arg(filter);
    } else {
        dialogs::configure_zenity(&mut command);
        // GTK portals run in a different process and cannot share child cancellation.
        command
            .env("GTK_USE_PORTAL", "0")
            .env("GDK_DEBUG", "no-portals");
        command.arg("--file-selection");
        if !options.initial.is_empty() {
            command.arg(format!("--filename={}", options.initial));
        }
        if options.kind == "saveFile" {
            command.args(["--save", "--confirm-overwrite"]);
        }
        if options.kind == "openMultiFile" {
            command
                .arg("--multiple")
                .arg(format!("--separator={separator}"));
        }
        for filter in &options.filters {
            command.arg(format!(
                "--file-filter={}|{}",
                filter.name,
                filter.patterns.join(" ")
            ));
        }
    }
    command
}

#[cfg(target_os = "linux")]
fn choose_files(
    options: &FileDialog,
    control: Option<&ActionExecutionControl>,
) -> Result<(String, Vec<String>), String> {
    if options.top_most && crate::x11::is_wayland() {
        return Err("File dialog topMost requires X11. Set topMost=false on Wayland".into());
    }
    let backend = if which::which("kdialog").is_ok() {
        "kdialog"
    } else if which::which("zenity").is_ok() {
        "zenity"
    } else {
        return Err("Install kdialog or zenity to select files".into());
    };
    // A fresh random delimiter avoids splitting Unix filenames on spaces or newlines.
    let token = tempfile::Builder::new()
        .prefix("quicker-selection-")
        .rand_bytes(24)
        .tempfile()
        .map_err(|e| e.to_string())?;
    let separator = token
        .path()
        .file_name()
        .and_then(|p| p.to_str())
        .ok_or("Invalid dialog separator")?;
    let mut seen = BTreeSet::new();
    let mut poll = std::time::Instant::now();
    let (_, output) = crate::process::output_with_monitor(
        command(options, backend, separator),
        control,
        "file dialog",
        |pid| {
            if options.top_most && poll.elapsed() >= Duration::from_millis(50) {
                poll = std::time::Instant::now();
                if let Some(window) =
                    crate::x11::WindowQuery::new(&pid.to_string(), "", "")?.find()?
                {
                    if seen.insert(window.process.window_id.clone()) {
                        crate::x11::dialog_above(
                            window
                                .process
                                .window_id
                                .parse()
                                .map_err(|_| "Invalid dialog window")?,
                            pid,
                        )?;
                    }
                }
            }
            Ok(())
        },
    )?;
    ensure_not_cancelled(control)?;
    if !output.status.success() {
        return Err(format!(
            "File selection cancelled or failed: {}",
            output.status
        ));
    }
    let paths = decode_paths(&output.stdout, backend, &options.kind, separator)?;
    if options.kind == "openMultiFile" {
        for path in &paths {
            check_selected_path(path, false)?;
        }
        return Ok((String::new(), paths));
    }
    let original = paths.into_iter().next().ok_or("No file was selected")?;
    let mut path = original.clone();
    if options.kind == "saveFile"
        && Path::new(&path).extension().is_none_or(|s| s.is_empty())
        && !options.extension.is_empty()
    {
        path = format!("{}.{}", path.trim_end_matches('.'), options.extension);
    }
    check_selected_path(&path, options.kind == "saveFile")?;
    if path != original && Path::new(&path).exists() {
        let result = dialogs::message_box(
            "Replace file",
            &format!("Replace this file?\n{path}"),
            "YesNo",
            "Warning",
            false,
            control,
        )?;
        if result != "Yes" {
            return Err("File replacement cancelled".into());
        }
    }
    Ok((path, Vec::new()))
}

#[cfg(not(target_os = "linux"))]
fn choose_files(
    _: &FileDialog,
    _: Option<&ActionExecutionControl>,
) -> Result<(String, Vec<String>), String> {
    Err("File selection requires the native Linux backend".into())
}

#[cfg(any(target_os = "linux", test))]
fn decode_paths(
    bytes: &[u8],
    backend: &str,
    kind: &str,
    separator: &str,
) -> Result<Vec<String>, String> {
    let text = dialogs::decode_text_output(bytes)?;
    let parts = if kind == "openMultiFile" {
        text.split(if backend == "kdialog" {
            "\n"
        } else {
            separator
        })
        .collect::<Vec<_>>()
    } else {
        vec![text.as_str()]
    };
    if parts.len() > 10_000 {
        return Err("File selection exceeds 10,000 paths".into());
    }
    parts
        .into_iter()
        .map(|part| {
            let path = if backend == "kdialog" {
                let encoded = part
                    .strip_prefix("file://")
                    .ok_or("File selection requires local files")?;
                if !encoded.starts_with('/') || encoded.contains(['?', '#']) {
                    return Err("File selection returned an unsupported URL".into());
                }
                urlencoding::decode(encoded)
                    .map_err(|_| "File selection returned invalid UTF-8")?
                    .into_owned()
            } else {
                part.to_owned()
            };
            if !Path::new(&path).is_absolute() || path.contains('\0') {
                return Err("File selection returned an invalid native path".into());
            }
            Ok(path)
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn check_selected_path(path: &str, save: bool) -> Result<(), String> {
    let path = Path::new(path);
    if save {
        if path.is_dir() || !path.parent().is_some_and(Path::is_dir) {
            return Err("Save selection requires a file in an existing directory".into());
        }
    } else if !path.is_file() {
        return Err("Open selection requires an existing file".into());
    }
    Ok(())
}
