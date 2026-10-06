# Shared action compatibility checks

The tool uses the v1 API found in the supplied Quicker 1.45.5 MSI.
It does not claim support for every Quicker version or action.

## MSI evidence

The inspection extracted managed assemblies without running the installer.
`dnfile` and `dncil` read the assembly metadata and method bodies.

| Artifact | SHA-256 |
| --- | --- |
| Quicker.x64.1.45.5.0.msi | `97418d96c00e25c8811b3c5fe3d7df6bb07a9b7be0849a6a1e41d9a1b8896a95` |
| Quicker.exe | `ac815c9280dee1ff2e476da5f6035a0451d07d3c0c4e12a02f0861f19fd4d8bd` |
| Quicker.Common.dll | `838e949d15b376c087b2bf0d00bf14f3c6c1b0e122a06ffea4213c800f71a594` |

`<DownloadSharedActionAsync>d__32.MoveNext` in Quicker.exe contains this route:

```text
https://api.getquicker.net/api/SharedAction/Download
    ?id={GUID}&revision={optional integer}&softVersion={version}&forPreview={boolean}
```

The method body starts at RVA `0x3d3384` in this assembly.
The API client initializer contains `https://api.getquicker.net` and `/api`.
`CreateClient` at RVA `0x282a9c` sets `DefaultRequestHeaders.Authorization`
with the `Bearer` scheme. The tool reads the token from `QUICKER_API_TOKEN`.

The response type is `ApiResult<SharedActionDto>` from Quicker.Common.dll.
`ApiResult<T>` defines `IsSuccess`, `ErrorCode`, `Message`, `Errors`, and `Data`.
`SharedActionDto` defines `Id`, `Revision`, `ActionType`, `Title`, and workflow
`Data`, plus other action metadata. The tool checks the ID and requested revision.
It preserves the HTTP response separately from the extracted action document.
The decoder accepts PascalCase and camelCase envelope fields.

The anonymous official request returned HTTP 401 on 2026-10-06.
No authenticated success response was available for this session.
Offline tests check response decoding against the MSI-defined structure.
They do not establish a successful authenticated download.

## Real public export

The [public export registry](public-exports.json) maps one shared ID to an
[author-published export](https://github.com/AlexShyXie/myCitaviMacros/tree/50fd22f27e4c752459ef9fe0dc1c96d490cef81d/Quicker%E5%8A%A8%E4%BD%9C%E9%A2%84%E8%AE%BE).
The registry records the exact repository commit and response hash.
The export's `SharedActionId` matches the requested ID.
The repository commit is not an official action revision.

Run the complete download and check:

```sh
cargo build --locked
python3 scripts/check-shared-action.py 6803b583-78f7-400d-a4c1-08de12ec7091 --public-export
```

On 2026-10-06, the request downloaded `Ref->Ob` and matched the recorded SHA-256.
All preservation checks passed. The runtime report returned `blocked` and exit code 1.
It identified unresolved shared subprogram dependencies. The runtime now has a
subprogram runner, but these network dependencies are not installed. The tool did
not execute the action.

Five other downloaded exports passed the same preservation checks.
These files were `obPDF->Cit`, `xID->CITAVI`, `KnowPDF++`, `AnnoPDF++`, and
[用QuickLook预览文件](https://www.getquicker.net/Common/Topics/ViewTopic/24595).
The other Citavi exports also have unresolved subprogram dependencies. QuickLook
still needs `sys:getSelectedFiles`, `sys:keyoperation`, the `outputText` input
mode, and a Linux replacement for `QuickLook.exe`. Its `sys:run` step is now
recognized. Passing preservation checks does not
establish execution compatibility with Citavi, QuickLook, or Windows APIs.

## Automated checks

```sh
cargo test --all-targets --locked
cargo build --locked
python3 scripts/test-check-shared-action.py
```

The Python tests do not need network access. They check authentication failures,
ID and revision mismatches, response shapes, redirects, size limits, and token handling.
They also pass downloaded response fixtures and local files to the actual Rust checker.
The Rust tests check preservation and runtime diagnostics.

The checker reports static evidence only. It does not validate all expressions,
application dependencies, permissions, platform behavior, or unknown option semantics.
Reports always set `runtime.executed` to `false`.

## Control flow evidence

The MSI subprogram body starts at RVA `0x3f7d80`. Its variable initializer at
`0x2c2a30` uses `IsInput`. Its output helper at `0x2c3020` prefixes keys with `var:`.
Local lookup compares `Name` and searches the current context before parent contexts.
`GetSubProgramFromSharedAction` at `0x2ab22c` converts a downloaded workflow body
to a subprogram. `Quicker.Common.ActionType.XSubProgram` has value 25.
`DownloadSubProgramAsync` at `0x3d3730` uses `/SharedAction/Download` with ID,
revision, and client version. An anonymous request for the real Citavi dependency
`3748cecd-84b6-47f7-191e-08ddfab0d924`, revision 4, returned HTTP 401 on 2026-10-07.
Authenticated dependency download and the original keyboard-layout subprogram's
execution remain unverified. Controlled tests cover local and cached shared calls,
typed parameters, variable isolation, nested lookup, returns, failure outputs,
recursion limits, cancellation during execution, and ID/revision mismatches.

The download tool supports `--with-dependencies` and `--dependency-dir PATH`.
On 2026-10-07, the public `Ref->Ob` download succeeded again. The recursive tool
found two calls to the same shared subprogram revision and sent one dependency
request. The server returned HTTP 401. The tool retained the root action and its
preservation report, marked runtime status as blocked, and recorded
`authentication_required` under `dependencies.items`. It created no dependency
file from the failed response. Seventeen offline Python tests pass, including
recursive downloads through global exports, cycles, deduplication, cache hash
checks, ID/revision/type rejection, authenticated-error handling, and type 25
round-trip preservation. These tests use controlled API responses for success.

Quicker.exe delegates its loop runners to managed closure methods. The repeat
body at RVA `0x3f1094` writes `count` before it evaluates `stopCondition`.
It reads the iteration limit and start index once. It checks the stop condition
on each iteration. A count of -1 selects an unlimited loop.
The sequential each body at RVA `0x3efbb8` writes the item and zero-based index
before child steps. Both runners consume the nearest loop's break and continue.
The if body at RVA `0x3f0ebc` selects either branch. The simpleIf body at
RVA `0x3f18b4` only executes its true branch.

Runtime tests check these orders with controlled workflows. They also check
nested loops, typed items, stop propagation, invalid counters, and cancellation.
These tests do not execute the downloaded actions' Windows applications.

## Clipboard evidence

The MSI `GetSelectedTextStep.Execute` delegates to the body at RVA `0x3ea6a4`.
It reads text format, wait time, retry count, and trim settings, and returns text
and encoded text. Its clipboard helper starts at RVA `0x1090a8`.
The `WaitClipboardChangeStep` body at RVA `0x402098` checks sequence numbers,
the sequence captured before Ctrl+C, the last clipboard change time, and a timeout.
The original uses Windows clipboard events. The Linux backend uses XFixes and
X server timestamps. It does not compare clipboard text to detect changes.

Run the X11 event test and the real selected-text workflow:

```sh
xvfb-run -a cargo test x11_clipboard_events_include_identical_copies_and_exclude_primary -- --ignored
xvfb-run -a env -u WAYLAND_DISPLAY XDG_SESSION_TYPE=x11 cargo test focused_selection_rejects_primary_owned_by_another_window -- --ignored
xvfb-run -a env -u WAYLAND_DISPLAY XDG_SESSION_TYPE=x11 cargo test clipboard_wait_cancels_while_waiting -- --ignored
cargo build --locked
xvfb-run -a python3 scripts/smoke-clipboard-x11.py
```

The workflow test requires Xvfb, xterm, xdotool, and ImageMagick for failure
screenshots. Xterm has a test-specific Ctrl+C copy binding. The test uses actual
keyboard input, clipboard ownership, clipboard data, and the launcher UI.

## Window activation evidence

The MSI `ActivateProcessMainWindowStep.Execute` delegates to RVA `0x3fa1a0`.
It reads process name/PID, window class, title, path, and hotkey inputs.
The implementation searches for a process window, starts a missing program when
a path is supplied, and can send an activation hotkey. Output helpers return
PID, window handle, and title.

The Linux implementation searches EWMH client windows. A bare X server uses
root children. Matching windows must satisfy both class and title filters.
It uses the existing focus verifier after each activation request.

```sh
xvfb-run -a env -u WAYLAND_DISPLAY XDG_SESSION_TYPE=x11 cargo test window_query_filters_before_focus_and_returns_window_metadata -- --ignored
xvfb-run -a python3 scripts/smoke-window-x11.py
```

The workflow test checks two existing xterm windows, exact keyboard recipients,
failure outputs, and a program launched through a path with spaces. Its
`--minimized` mode requires a window manager. That mode passed with KWin in an
isolated Xvfb/D-Bus session. Application-specific tray hotkeys still need testing.

## Run module evidence

The MSI `RunOrOpenStep.Execute` at RVA `0x2d04d8` delegates to the body at
`0x400dc8`. It reads run options, captures output when an output variable is
bound, and treats a completed process launch as successful even with a nonzero
exit code. `ActionHelper.StartProcess` at `0x257a6c` handles alternate paths,
environment values, working directories, and existing-window activation.
The official [run module reference](https://www.getquicker.net/KC/Help/Doc/run)
documents the input and output fields.

The Linux implementation uses the
[Microsoft CRT argument rules](https://learn.microsoft.com/en-us/cpp/c-language/parsing-c-command-line-arguments?view=msvc-170).
Eight tests check quoted and empty arguments, Unicode, literal shell operators,
environment overrides, alternate paths, working directories, nonzero exit
codes, output capture, detached launches, document handlers, and cancellation
of process descendants. Unsupported Windows options fail before process launch.
The X11 workflow test also calls `sys:run` and verifies reuse of the focused
application, its PID and window handle, and subsequent keyboard delivery.

On 2026-10-07, all six downloaded exports passed preservation again. The QuickLook
report recognizes `sys:run` and flags `QuickLook.exe` as requiring replacement.
These tests do not verify execution of the Windows QuickLook application.
