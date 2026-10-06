use super::*;
use std::cmp::Ordering as SortOrder;

pub(super) const SORT_TYPES: &[&str] = &[
    "Default",
    "Origin",
    "FileName",
    "FileNameNature",
    "FileSizeAsc",
    "FileSizeDesc",
    "CreationTimeAsc",
    "CreationTimeDesc",
    "LastAccessTimeAsc",
    "LastAccessTimeDesc",
    "LastWriteTimeAsc",
    "LastWriteTimeDesc",
];

fn parse_file_uris(bytes: &[u8]) -> Result<Vec<String>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "File clipboard is not UTF-8")?;
    let mut paths = Vec::new();
    for line in text
        .lines()
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
    {
        let location = line
            .strip_prefix("file://")
            .ok_or("Selection contains a non-local URI")?;
        let path = if location.starts_with('/') {
            location
        } else {
            location
                .strip_prefix("localhost")
                .filter(|s| s.starts_with('/'))
                .ok_or("Remote file authorities are not supported")?
        };
        if path.contains(['?', '#']) {
            return Err("File URI contains an unescaped query or fragment".into());
        }
        if path.bytes().any(|c| c.is_ascii_whitespace()) {
            return Err("File URI contains unescaped whitespace".into());
        }
        for (i, byte) in path.bytes().enumerate() {
            if byte == b'%'
                && !path
                    .as_bytes()
                    .get(i + 1..i + 3)
                    .is_some_and(|s| s.iter().all(u8::is_ascii_hexdigit))
            {
                return Err("File URI has invalid percent encoding".into());
            }
        }
        let path = urlencoding::decode(path)
            .map_err(|_| "File path is not UTF-8")?
            .into_owned();
        if path.contains('\0') {
            return Err("File path contains a NUL character".into());
        }
        paths.push(path);
    }
    if paths.is_empty() {
        return Err("No selected files were copied".into());
    }
    Ok(paths)
}

fn name(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
}

// Compare digit runs without parsing fixed-width numbers. Case folding and
// ordinal Unicode ordering are deterministic; Windows locale rules can differ.
fn natural(a: &str, b: &str) -> SortOrder {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut a, mut b) = (a.as_str(), b.as_str());
    while !a.is_empty() && !b.is_empty() {
        let (ac, bc) = (a.chars().next().unwrap(), b.chars().next().unwrap());
        if ac.is_ascii_digit() && bc.is_ascii_digit() {
            let an = a.bytes().take_while(u8::is_ascii_digit).count();
            let bn = b.bytes().take_while(u8::is_ascii_digit).count();
            let (ad, bd) = (
                a[..an].trim_start_matches('0'),
                b[..bn].trim_start_matches('0'),
            );
            let order = ad
                .len()
                .cmp(&bd.len())
                .then_with(|| ad.cmp(bd))
                .then(an.cmp(&bn));
            if order != SortOrder::Equal {
                return order;
            }
            a = &a[an..];
            b = &b[bn..];
        } else {
            if ac != bc {
                return ac.cmp(&bc);
            }
            a = &a[ac.len_utf8()..];
            b = &b[bc.len_utf8()..];
        }
    }
    a.len().cmp(&b.len())
}

fn sort_files(
    files: &mut [String],
    sort: &str,
    control: Option<&ActionExecutionControl>,
) -> Result<(), String> {
    if !SORT_TYPES.contains(&sort) {
        return Err(format!("Unsupported file sort: {sort}"));
    }
    ensure_not_cancelled(control)?;
    match sort {
        "Origin" => return Ok(()),
        "Default" | "FileNameNature" => {
            files.sort_by(|a, b| natural(name(a), name(b)));
            return Ok(());
        }
        "FileName" => {
            files.sort_by_cached_key(|p| name(p).to_lowercase());
            return Ok(());
        }
        _ => {}
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        #[derive(PartialEq, Eq, PartialOrd, Ord)]
        enum Key {
            Size(u64),
            Time(std::time::SystemTime),
        }
        let mut keys = HashMap::new();
        for path in files.iter() {
            ensure_not_cancelled(control)?;
            let metadata =
                fs::metadata(path).map_err(|e| format!("Cannot sort file {path}: {e}"))?;
            if !metadata.is_file() {
                return Err("Metadata sorting requires regular files".into());
            }
            let key = if sort.starts_with("FileSize") {
                Key::Size(metadata.len())
            } else {
                Key::Time(
                    match sort {
                        "CreationTimeAsc" | "CreationTimeDesc" => metadata.created(),
                        "LastAccessTimeAsc" | "LastAccessTimeDesc" => metadata.accessed(),
                        _ => metadata.modified(),
                    }
                    .map_err(|e| format!("Requested file timestamp is unavailable: {e}"))?,
                )
            };
            keys.insert(path.clone(), key);
        }
        files.sort_by(|a, b| {
            let order = keys[a].cmp(&keys[b]);
            if sort.ends_with("Desc") {
                order.reverse()
            } else {
                order
            }
        });
        Ok(())
    }
    #[cfg(target_arch = "wasm32")]
    Err("File metadata sorting requires the native application".into())
}

impl QuickerRuntime {
    pub(super) fn run_selected_files(
        &mut self,
        step: &QuickerPluginStepDocument,
    ) -> Result<StepFlow, String> {
        let stop = self
            .input_value(&step.input_params, "stopIfFail")?
            .is_none_or(|v| truthy(Some(&v)));
        let result = (|| {
            let operation = self
                .input_string_opt(&step.input_params, "operation")?
                .unwrap_or_else(|| "getSelection".into());
            if operation != "getSelection" {
                return Err(format!("Unsupported selected-files operation: {operation}"));
            }
            let sort = self
                .input_string_opt(&step.input_params, "sortType")?
                .unwrap_or_else(|| "Default".into());
            if !SORT_TYPES.contains(&sort.as_str()) {
                return Err(format!("Unsupported file sort: {sort}"));
            }
            let wait = self
                .input_string_opt(&step.input_params, "waitMs")?
                .unwrap_or_else(|| "200".into())
                .parse::<u32>()
                .map_err(|_| "Invalid selected-files waitMs")?;
            let mut files = self.copy_selected_files(wait)?;
            if files.is_empty() {
                return Err("No selected files were copied".into());
            }
            sort_files(&mut files, &sort, self.control.as_ref())?;
            Ok(files)
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
        let error = result.as_ref().err().cloned();
        let files = result.unwrap_or_default();
        self.assign_output(&step.output_params, "fileCount", Value::from(files.len()))?;
        self.assign_output(
            &step.output_params,
            "firstFile",
            Value::String(files.first().cloned().unwrap_or_default()),
        )?;
        self.assign_output(
            &step.output_params,
            "firstFileName",
            Value::String(
                files
                    .first()
                    .map(|p| name(p).to_owned())
                    .unwrap_or_default(),
            ),
        )?;
        self.assign_output(
            &step.output_params,
            "fileNames",
            Value::Array(
                files
                    .iter()
                    .map(|p| Value::String(name(p).into()))
                    .collect(),
            ),
        )?;
        self.assign_output(
            &step.output_params,
            "files",
            Value::Array(files.into_iter().map(Value::String).collect()),
        )?;
        if stop {
            if let Some(error) = error {
                return Err(error);
            }
        }
        Ok(StepFlow::Continue)
    }

    fn copy_selected_files(&mut self, wait: u32) -> Result<Vec<String>, String> {
        ensure_not_cancelled(self.control.as_ref())?;
        #[cfg(test)]
        if let Some(result) = with_action_test_runtime(|r| r.file_selection_results.pop_front()) {
            return result;
        }
        #[cfg(target_os = "linux")]
        {
            use std::time::{Duration, Instant};
            if crate::x11::is_wayland() {
                return Err("Selected files require an X11 session".into());
            }
            let target =
                crate::x11::focused_process().ok_or("No file-selection target is focused")?;
            let before = clipboard_steps::clipboard_snapshot()?.0;
            self.clipboard_before_copy = Some(before);
            send_key_combo(&["ctrl".into()], "c")?;
            let start = Instant::now();
            loop {
                ensure_not_cancelled(self.control.as_ref())?;
                if clipboard_steps::clipboard_snapshot()?.0 != before {
                    break;
                }
                if start.elapsed() >= Duration::from_millis(u64::from(wait)) {
                    return Err(
                        "The active application did not copy selected files before the timeout"
                            .into(),
                    );
                }
                sleep_millis(2, self.control.as_ref())?;
            }
            let current = clipboard_steps::clipboard_snapshot()?.0;
            let bytes = crate::x11::read_file_selection(self.control.as_ref())?;
            if clipboard_steps::clipboard_snapshot()?.0 != current {
                return Err("The file clipboard changed during the read".into());
            }
            let after = crate::x11::focused_process().ok_or("File-selection target lost focus")?;
            if after.window_id != target.window_id || after.process_id != target.process_id {
                return Err("The focused window changed during file selection".into());
            }
            parse_file_uris(&bytes)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = wait;
            Err("Selected files require the Linux application".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn file_uri_parser_preserves_paths_and_rejects_partial_lists() {
        assert_eq!(
            parse_file_uris(
                b"# comment\r\nfile:///tmp/a%20b%23%25\r\nfile://localhost/tmp/%E4%B8%AD%0A\r\n"
            )
            .unwrap(),
            ["/tmp/a b#%", "/tmp/中\n"]
        );
        for invalid in [
            "",
            "# comment",
            "file://server/tmp/a",
            "https://example.com/a",
            "file:///tmp/%",
            "file:///tmp/%g0",
            "file:///tmp/%ff",
            "file:///tmp/%00",
            "file:///tmp/a?b",
            "file:///tmp/a#b",
            "file:///tmp/good\nfile://server/bad",
        ] {
            assert!(parse_file_uris(invalid.as_bytes()).is_err(), "{invalid}");
        }
        assert!(parse_file_uris(&[0xff]).is_err());
    }

    #[test]
    fn file_sort_handles_numeric_names_and_real_metadata() {
        let mut files: Vec<String> = [
            "/a/File10",
            "/b/file02",
            "/a/file2",
            "/a/file99999999999999999999999999999999",
        ]
        .map(String::from)
        .into();
        let original = files.clone();
        sort_files(&mut files, "Origin", None).unwrap();
        assert_eq!(files, original);
        sort_files(&mut files, "Default", None).unwrap();
        assert_eq!(
            files,
            [
                "/a/file2",
                "/b/file02",
                "/a/File10",
                "/a/file99999999999999999999999999999999"
            ]
        );
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("small");
        let b = dir.path().join("large");
        fs::write(&a, b"a").unwrap();
        fs::write(&b, b"abc").unwrap();
        let mut files = vec![b.to_str().unwrap().into(), a.to_str().unwrap().into()];
        sort_files(&mut files, "FileSizeAsc", None).unwrap();
        assert_eq!(files[0], a.to_str().unwrap());
        sort_files(&mut files, "FileSizeDesc", None).unwrap();
        assert_eq!(files[0], b.to_str().unwrap());
        sort_files(&mut files, "LastWriteTimeAsc", None).unwrap();
        files.push(dir.path().to_str().unwrap().into());
        assert!(sort_files(&mut files, "FileSizeAsc", None)
            .unwrap_err()
            .contains("regular files"));
    }

    #[test]
    fn selected_files_outputs_are_typed_and_cleared_after_failure() {
        reset_action_test_runtime();
        with_action_test_runtime(|r| {
            r.file_selection_results.extend([
                Ok(vec!["/tmp/file10".into(), "/tmp/file2".into()]),
                Err("No selection".into()),
            ])
        });
        let data: QuickerPluginData = serde_json::from_value(json!({"Variables": [
            {"Key":"files", "Type":4}, {"Key":"count", "Type":12}, {"Key":"ok", "Type":2}
        ]}))
        .unwrap();
        let mut runtime = QuickerRuntime::new(&data, "files-test".into(), None).unwrap();
        let step = serde_json::from_value(json!({"StepRunnerKey":"sys:getSelectedFiles",
            "InputParams":{"stopIfFail":{"Value":"0"}}, "OutputParams":{
                "isSuccess":"ok", "errMessage":"error", "files":"files", "fileCount":"count",
                "fileNames":"names", "firstFile":"first", "firstFileName":"name"}}))
        .unwrap();
        runtime.run_selected_files(&step).unwrap();
        assert_eq!(runtime.vars["files"], json!(["/tmp/file2", "/tmp/file10"]));
        assert_eq!(runtime.vars["count"], 2);
        assert_eq!(runtime.vars["ok"], true);
        assert_eq!(runtime.vars["name"], "file2");
        assert_eq!(runtime.vars["first"], "/tmp/file2");
        assert_eq!(runtime.vars["names"], json!(["file2", "file10"]));
        runtime.run_selected_files(&step).unwrap();
        assert_eq!(runtime.vars["files"], json!([]));
        assert_eq!(runtime.vars["names"], json!([]));
        assert_eq!(runtime.vars["count"], 0);
        assert_eq!(runtime.vars["ok"], false);
        assert_eq!(runtime.vars["first"], "");
        assert_eq!(runtime.vars["name"], "");
        assert_eq!(runtime.vars["error"], "No selection");
    }

    #[test]
    fn selected_files_validates_before_copy_and_never_swallows_cancellation() {
        reset_action_test_runtime();
        with_action_test_runtime(|r| {
            r.file_selection_results
                .push_back(Ok(vec!["/tmp/a".into()]))
        });
        let data: QuickerPluginData = serde_json::from_value(json!({})).unwrap();
        let control = ActionExecutionControl::default();
        let mut runtime =
            QuickerRuntime::new(&data, "files-test".into(), Some(control.clone())).unwrap();
        for params in [
            json!({"operation":{"Value":"setSelection"}}),
            json!({"sortType":{"Value":"invalid"}}),
            json!({"waitMs":{"Value":"-1"}}),
        ] {
            let step = serde_json::from_value(
                json!({"StepRunnerKey":"sys:getSelectedFiles", "InputParams":params}),
            )
            .unwrap();
            assert!(runtime.run_selected_files(&step).is_err());
        }
        with_action_test_runtime(|r| assert_eq!(r.file_selection_results.len(), 1));
        control.cancel();
        let step = serde_json::from_value(json!({"StepRunnerKey":"sys:getSelectedFiles", "InputParams":{"stopIfFail":{"Value":"0"}}})).unwrap();
        assert!(runtime
            .run_selected_files(&step)
            .unwrap_err()
            .contains("cancelled"));
    }
}
