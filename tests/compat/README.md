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
still needs a Linux replacement for `QuickLook.exe`. All its step runners are now
recognized. The adapted Linux execution test is described below.
Passing preservation checks does not
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

## Key operation evidence

The MSI `KeyOperationStep.Execute` at RVA `0x2dd924` delegates to `0x40774c`.
The body selects state, down, and up operations. `KeyFromValueOrName` at
`0x116214` accepts hexadecimal and decimal values, then enum names. The
`GetKeyStateFromSystem` method at `0x11629c` returns separate down and toggle bits.
The [official module reference](https://docs.getquicker.net/v2/xaction/modules/keyoperation/)
describes paired presses/releases and state outputs.

The X11 backend uses XTEST events and XKB locked modifiers. It reads the active
keyboard mapping for each operation. Generic modifiers query both sides; a
generic press uses the left side. Mouse state supports the three core buttons.
The action owns only keys that were up before its injected press. Subprograms
share ownership. Teardown releases those keys after success, stop, error, or
cancellation. Quicker virtual keys and raw physical state remain unsupported.

```sh
xvfb-run -a env -u WAYLAND_DISPLAY XDG_SESSION_TYPE=x11 cargo test key_operations_query_state -- --ignored
xvfb-run -a python3 scripts/smoke-input-x11.py --key-operation
```

Both checks passed on 2026-10-07. The first verifies live state, toggling,
subprogram ownership, cancellation cleanup, and preservation of externally held
keys. The second verifies actual Shift+A and Space input and closed-target errors.

## Text output evidence

The MSI `OutputTextStep.Execute` at RVA `0x2cf138` delegates to `0x4006e8`.
It uses the common success/failure wrapper and skips empty text.
`ActionHelper.SendTextToWindow` at `0x259368` selects paste or keyboard text,
normalizes CRLF for keyboard input, uses delays, and optionally presses Return.
The [official reference](https://docs.getquicker.net/v2/xaction/modules/outputtext/)
describes these fields.

The Linux implementation sends short complete xdotool batches and checks
cancellation between them. Character delays are cancellable. The initial X11
test lost Chinese and emoji with the old zero-delay backend. With xdotool's
12 ms key timing, the test received the exact UTF-8 bytes. The test also covers
literal leading dashes, CRLF conversion, appended Return, and restoration of an
action-held Shift key. A separate test cancels during a character delay and
verifies that no second character or later state write occurs with `stopIfFail=0`.

```sh
xvfb-run -a python3 scripts/smoke-input-x11.py --output-text
xvfb-run -a python3 scripts/smoke-input-x11.py --text-cancel
```

These checks passed on 2026-10-07. Simulated typing still depends on the target's
input method and its handling of X11 keyboard events.

## Selected file evidence

The MSI `GetSelectedFilesStep.Execute` at RVA `0x2fbb8c` delegates to `0x4184d0`.
The get operation calls `Wo7HearJ3wm` at `0x2fbc88`. It uses the native selection
API and clipboard file-list fallbacks. The output helper at `0x2fc230` sets lists,
names, first-file outputs, and count. The set operation calls the existing
Explorer-window API. The Linux port does not substitute opening another window
for that operation.
The [official reference](https://docs.getquicker.net/v2/xaction/modules/getselectedfiles/)
describes the copy fallback, wait interval, outputs, and sorting options.

The X11 implementation requires a fresh copy event and a stable focused target.
It reads `text/uri-list` or `x-special/gnome-copied-files`, including INCR transfers.
Invalid or remote URIs fail the complete read. Failed reads clear file outputs.
Size and time sorts require regular files. Unavailable timestamps return an
error. Filename sorting uses deterministic Unicode ordering, not Windows locale
rules. Native Wayland and `setSelection` remain unsupported.

```sh
xvfb-run -a env -u WAYLAND_DISPLAY XDG_SESSION_TYPE=x11 cargo test file_selection_transfers -- --ignored
xvfb-run -a dbus-run-session -- python3 scripts/smoke-files-x11.py
xvfb-run -a dbus-run-session -- python3 scripts/smoke-files-x11.py /path/to/quicklook-forum.json
```

All three checks passed on 2026-10-07. The transfer test covers ordinary and
incremental transfers, GNOME fallback, unsupported formats, declared size limits,
owner changes, timeout, and cancellation. Dolphin passed actual file selection,
repeated identical copies, Unicode and space paths, natural sorting, and stale
clipboard rejection from a terminal that does not copy files.

The last command uses the real forum export with SHA-256
`d0cd58a9baedbd2f2a6fffafde0be2c1603d5ba7a5660a674328e7c841780767`.
It replaces only the program path inside the original steps with ImageMagick
`display`. It retains file selection, branching, iteration, formatting, and launch
parameters. It appends state writes for observations and changes the test title.
The test verified completion and both image windows on two executions. This
validates the adapted workflow. It does not run Windows QuickLook.

The original export now has one static blocker: its Windows executable.
Native tests passed 126 cases with eight opt-in tests excluded. Clippy passed
with warnings denied. The Wasm check passed with preview dead-code warnings.
All six downloaded exports passed preservation again. The ID download interface
also fetched the pinned public `Ref->Ob` export again and verified its hash.

## Regex extraction and text processing evidence

The downloaded `xID->CITAVI` export contains `trim` and a two-group regex.
Its SHA-256 is `13b30098483bc737e0ac5afddd166562b1d6dabd8150f00f7a93e92293918a6b`.
The prototype interpreted `getGroup=1` as a group index. It assigned only one
result and never wrote the second group. This lost the Citavi item ID.

The MSI `RegexExtractStep.Execute` at RVA `0x2d7508` calls `0x404624`.
The body selects modes `0`, `1`, and `2`, and accepts legacy `false` and `true`.
Mode 1 excludes the complete match from the group list. The helper at `0x2d758c`
builds a list for each group in mode 2 and inserts empty strings for missing
captures. The [official module reference](https://docs.getquicker.net/v2/xaction/modules/regexextract/)
agrees with these output shapes. Numbered outputs also accept trailing spaces
in their keys, as the downloaded export requires.

The MSI `StringProcessStep.Execute` at RVA `0x2b7b64` calls `0x3ee16c`.
It dispatches using uppercase method names and handles UTF-8 URL encoding
separately from other encodings. The [text processing reference](https://docs.getquicker.net/v2/xaction/modules/stringprocess/)
describes the whitespace and case operations. The Linux runtime now handles
trim variants and uppercase conversion, alongside lowercase and UTF-8 URL
encoding. Other encodings fail explicitly. Unsupported methods remain JSON
cards in the editor instead of appearing as lowercase conversion.

```sh
QUICKER_COMPAT_CORPUS=/path/to/downloads cargo test downloaded_citavi_text -- --ignored
xvfb-run -a python3 scripts/smoke-plugin-editor-x11.py /path/to/citavi-1.json
```

Both checks passed on 2026-10-07. The first extracts the original trim and regex
steps from the downloaded action. It supplies a controlled Citavi reference and
checks that the name and full ID reach separate variables. This tests the text
pipeline only. It does not run Citavi or the unavailable shared subprograms.
The second saves the full imported action through the GUI and checks exact JSON,
tags, and hotkey preservation.

Unit tests cover all three extraction modes, optional and missing groups, named
group order, zero-length matches, flags, malformed patterns, size limits, output
clearing, and cancellation. They also cover Unicode whitespace and editor method
preservation. Native tests passed 133 cases with nine opt-in tests excluded.
Python tests passed 17 cases. Clippy and Wasm checks passed, with preview dead-code
warnings. All six downloaded exports passed preservation again. The `xID->CITAVI`
report now lists only its two unresolved shared-subprogram call sites as blockers.

The regex engine is not .NET. The runtime rejects right-to-left matching and
.NET object outputs. It reports engine differences and applies documented memory,
match-count, and backtracking limits. It does not reproduce .NET's three-second
match timeout or every syntax and Unicode rule.

## Dialog evidence (2026-10-07)

`MessageBoxOutputRunner.Execute` starts at RVA `0x2e9a3c` in the supplied MSI.
It outputs the standard result name and sets `okOrYes` for enum values 1 and 6
(OK and Yes). It restores the foreground window when requested. Empty titles
use the action title. `UserInputStep.Execute` at `0x2b7ee0` delegates to
`0x3eea7c`, which uses `String.IsNullOrEmpty` for `isEmpty`.
The input validation handler at `0x1c6ee8` also uses `IsNullOrEmpty` and
`Regex.IsMatch` without flags.
`SelectFolderStep.Execute` starts at `0x2fda90` and delegates to `0x41905c`.
The [official message-box definition](https://docs.getquicker.net/v2/xaction/modules/msgbox/)
also describes these outputs and the custom mode that remains unsupported here.

The [dialog smoke test](../../scripts/smoke-dialogs-x11.py) runs actual kdialog
and zenity processes through imported workflows. It checks standard button
results, both confirmation values, Unicode and literal argument text, trailing
spaces/newlines, multiline input, initial directories, and user cancellation.
It cancels each of the three dialog types through the launcher and verifies that
the dialog closes without executing the next step, including `stopIfFail=0`.
The test selects each backend through an isolated PATH and uses an isolated X11
server and D-Bus session. GTK portal integration is disabled for this check.
Desktop portal dialogs and native Wayland remain unverified.

Zenity permits exit-code changes through environment variables. The runner sets
fixed values for its child processes. The test supplies conflicting settings to
verify that cancellation cannot become a positive response. The underlying
[Zenity response implementation](https://github.com/GNOME/zenity/blob/master/src/util.c)
describes these overrides.

A new author export came from
[Xebec33/chinese_quick_converter](https://github.com/Xebec33/chinese_quick_converter/blob/d00f7bf185506334075586ed0827659fe5c06c93/quick_converter.json).
The downloaded `OpenCC` file has SHA-256
`e511eca189b4db9fa323697c263e4686cabd590b918431caab15e6d4e1bcea5e`.
Its local action ID is `14c751d1-1c05-4e21-9b82-128b35413d45`; it has no
`SharedActionId`. Therefore it is not added to the shared-ID download registry.
Use `--file` after downloading the pinned export. All four import/preservation
checks passed. The report blocks its `SelectSingleFolder` input tool, script/file
modules, and unsupported expressions. The complete OpenCC workflow was not run.

### OpenCC text file execution

The opt-in `downloaded_opencc_file_steps_read_and_write_native_paths` test loads
the pinned `opencc.json` from `QUICKER_COMPAT_CORPUS`. It executes ten original
file steps with temporary native paths in the existing variables. It checks
UTF-8 content, Unicode variable names, file outputs, and preserved newlines.
Two other file steps contain Windows path templates and must return errors.
The test does not execute the OpenCC program or the complete action.

```sh
QUICKER_COMPAT_CORPUS=/path/to/json cargo test --locked downloaded_ -- --ignored --nocapture
```

The MSI references are `ReadFileStep.Execute` at `0x2fcc7c`, its closure at
`0x4185d8`, `WriteTextFileStep.Execute` at `0x2fdf8c`, its closure at `0x419530`,
the overwrite helper at `0x2fe000`, and `NormalizeNewlines` at `0x2fe11c`.
The official [read-file documentation](https://docs.getquicker.net/v2/xaction/modules/readfile/)
and [write-text documentation](https://docs.getquicker.net/v2/xaction/modules/writetextfile/)
describe the input and output parameters. Tests cover exact UTF-8/16/32 bytes,
BOM handling, append, overwrite, CRLF defaults, newline conversion, the `.ps1`
BOM exception, failure outputs, cancellation, size limits, and special files.

The implementation rejects malformed text instead of using .NET replacement
fallback. Automatic encoding detection, Windows default code pages, and legacy
encodings remain gaps. Reads expand process environment variables. Windows path
normalization and zero-width character removal are not reproduced. Local text
paths retain their whitespace. Writes can be partial after cancellation or an
I/O error. Reports describe these limits and never claim full action execution.

### File selection and real OpenCC save steps

The file dialog runner follows `SelectFileStep.Execute` at `0x2fd370`, its
closures at `0x4189f4` and `0x418db8`, and `AppHelper` methods at `0x107bd0`,
`0x107d18`, and `0x107e58`. The
[official module documentation](https://docs.getquicker.net/v2/xaction/modules/selectfile/)
describes its modes and bindings. The MSI emits both path output slots on
success and emits neither slot after cancellation. Tests preserve this behavior.

Run the GUI check after `cargo build --locked`:

```sh
xvfb-run -a -s '-screen 0 1280x900x24' env -u WAYLAND_DISPLAY \
  QT_QPA_PLATFORM=xcb GDK_DEBUG=no-portals GTK_USE_PORTAL=0 \
  XDG_CURRENT_DESKTOP=X-Generic QUICKER_COMPAT_CORPUS=/path/to/json \
  dbus-run-session -- python3 scripts/smoke-file-dialogs-x11.py kdialog
```

Repeat with `zenity`. Add `--with-wm` after the backend to start an isolated
KWin X11 session. The test validates the EWMH above property. Under a window
manager, it activates the panel before sending action cancellation and checks
the actual input focus. Escape in the file chooser only cancels that selection.

The test covers single/multiple selection, save paths, default extensions,
overwrite refusal, failure outputs, and action cancellation. It includes
Unicode, quotes, percent signs, vertical bars, and a newline in real filenames.
Without `QUICKER_COMPAT_CORPUS`, it runs the authored workflow only. With that
variable, it checks the pinned OpenCC hash and executes all four original file
selectors plus the original writer, using temporary path variables. Both
backends passed these original save steps. The full action remains blocked by
other modules, expressions, and Windows commands.

[KDialog's implementation](https://github.com/KDE/kdialog/blob/master/src/kdialog.cpp)
returns file URLs for URL selection. The runner accepts local URLs only.
[Zenity's file selection implementation](https://github.com/GNOME/zenity/blob/master/src/fileselection.c)
accepts a separator for multiple paths. The runner uses a fresh random delimiter
for each dialog. Native filters and automatic extensions can differ from Windows.
Simple wildcard filters are supported. Exact-name and bracket-pattern filters
are rejected. Local Qt/GTK dialogs keep cancellation within the managed process.
Portal selection and native Wayland remain outside this verification.

### List operations and original OpenCC steps

`ListOperationRunner.Execute` at `0x2f2ba0` invokes the closure at `0x410bf0`.
The closure uses string lists. It changes the input for updates and returns a
new list for sorting, slicing, concatenation, distinct values, and filtering.
The text predicates at `0x412340`, `0x41235c`, and `0x412374` use
`OrdinalIgnoreCase`. Filter exclusions use LINQ `Except`, which removes duplicates.
Slices use `Skip` and `Take`. Equality and index lookup remain case-sensitive.
The closure normalizes negative positions before dispatch. `removeAt` adds
Count again if the normalized position remains negative. Tests retain that behavior.

The MSI passes no stop parameter to `ExecuteCommonAction` at `0x2a7d1c`.
List failures therefore stop the action. The newer
[official list catalog](https://docs.getquicker.net/v2/xaction/modules/listoperations/)
includes `stopIfFail`. The compatibility runner rejects its false setting.
The runtime implements 31 operations. Fuzzy/pinyin filtering remains blocked.
Ordinal sorting replaces Windows culture sorting. Natural sorting, Unicode
case tables, native timestamps, and collection reference identity can differ.
The checker reports these limitations. Lists accept text items only, with limits
of 100,000 items and 16 MiB. Regex syntax and backtracking retain the limits of
the existing regex runner. No arbitrary C# or .NET objects are introduced.

`CommentStep.Execute` at `0x2ee2a0` returns without reading its inputs.
The runtime and checker ignore comment inputs and child steps.
The opt-in `downloaded_opencc_list_and_comment_steps_execute_unchanged` test
executes both original append steps and both original comment steps. It loads
original variable declarations and supplies a native path through the existing
`path` variable. It checks the two resulting list entries. The source JSON
remains unchanged. Run this test with the existing `downloaded_` corpus command.

On 2026-10-07, another download of the pinned author export matched SHA-256
`e511eca189b4db9fa323697c263e4686cabd590b918431caab15e6d4e1bcea5e`.
All four corpus tests passed. The report no longer lists list operations or
comments as blockers. Other OpenCC modules, expressions, and Windows paths
remain blocked. The complete OpenCC workflow was not executed.

### Path expressions and OpenCC output files

The path interpreter implements nine pure methods with Linux separators. Its
rules follow the [.NET path implementation](https://github.com/dotnet/runtime/blob/v8.0.0/src/libraries/System.Private.CoreLib/src/System/IO/Path.cs)
and [Unix path implementation](https://github.com/dotnet/runtime/blob/v8.0.0/src/libraries/System.Private.CoreLib/src/System/IO/Path.Unix.cs).
Directory extraction compresses separator runs. Filename and extension methods
retain text components. Dotfiles have an extension in these .NET methods.
`Combine` preserves dot segments and resets at a later absolute component.
It accepts multiple strings or one text list. No operation reads the filesystem.
The methods reject Windows paths, NUL characters, and paths above 1 MiB.

The string parser follows the [C# literal specification](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/language-specification/lexical-structure#6456-string-literals).
Verbatim literals retain their contents and decode doubled quotes. Regular
literals now use C# escapes instead of JSON escapes. Unicode surrogate pairs
produce UTF-8 text. Unpaired surrogates produce errors. Character literals,
interpolation, raw strings, and C# statement blocks remain unsupported.

A separate C# program generated 234 Linux reference results using SDK 8.0.100
and .NET 8.0.0. The SDK archive matched Microsoft's release-metadata SHA-512.
The Rust interpreter matched all recorded results. The reference fixture and
its generator are committed. Normal builds have no .NET dependency.

The opt-in `downloaded_opencc_path_expressions_and_save_steps_execute_unchanged`
test loads the pinned author export. It executes six original assignments and
evaluates two original dialog/launcher inputs. Two original writes produce the
expected UTF-8 files. The original input file remains unchanged. Two assignments
produce paths with literal backslashes. Their values remain unchanged and the
original write step rejects them. No file manager or dialog runs in this test.

A fresh download on 2026-10-07 matched the pinned OpenCC hash. Preservation and
all five corpus tests passed. The checker now reports two unsupported expression
blocks, eight Linux path warnings, and two Windows literal warnings. Previously
it reported ten unsupported expressions. The two statement blocks, other modules,
and Windows commands still prevent full OpenCC execution. A syntax check does
not establish that a path expression can run with every input value.


## Text window evidence

The MSI ShowTextStep dispatcher is at RVA `0x405144`. Window creation and
configuration use `0x2d85d4` and `0x405be8`. Query uses `0x2d8360` and `0x405844`.
Close uses `0x2d8f34` and `0x406820`. Wait-close uses `0x2d8204`.
The official module catalog and text-window reference define the option names.

The native implementation uses egui viewports. Action workers share bounded
text documents with the UI thread. Each window has separate editor state.
Unit tests cover replacement, update, exact append, limits, and UTF-16 offsets.
Static checks reject unsupported modes, native handles, and advanced options.

```sh
cargo build --locked
xvfb-run -a -s '-screen 0 1280x900x24' env -u WAYLAND_DISPLAY \
  QT_QPA_PLATFORM=xcb QUICKER_COMPAT_CORPUS=/tmp/quicker-real-plugins \
  dbus-run-session -- python3 scripts/smoke-text-windows-x11.py
```

The test requires KWin, Xvfb, xdotool, D-Bus, and ImageMagick. It uses isolated
configuration and runtime directories. It checks edited Unicode text, CRLF,
selection, UTF-16 caret offsets, a return button, topMost, update, append,
query, wait-close, programmatic close, failure continuation, and cancellation.
It verifies the pinned OpenCC hash and executes three unchanged showText steps.
Both NO_WAIT windows remain open when the action terminates. Other OpenCC modules
are not executed by this test. The full action remains blocked.

## Waiting window evidence

The MSI ShowWaitWinStep Execute method is at RVA `0x2b59b4`. The creation,
existing-window update, and explicit-update closures are `0x3edd2c`, `0x3edfd4`,
and `0x3ee088`. They use the root action context, including subprogram calls.
Missing-window checks return closed. The MSI also writes `isClosed` after
`waitClose` and `showAndWaitClose`, although the catalog only lists `check`.

WaitUserWindow.Update is at `0x1c812c`. Progress parsing uses `0x1c845c`.
A negative numerator adds the total before progress calculation. The native
implementation rejects nonfinite values and nonpositive totals. It caps text,
button count, window count, font size, and automatic timeout.

The closed handler at `0x1c7d08` stops the action unless a button or programmatic
close sets its bypass flag. The automatic timer at `0x1c8930` calls Close without
that flag. The Linux implementation retains this MSI behavior. The current
[official documentation](https://docs.getquicker.net/v2/xaction/modules/showwaitwin/)
describes stopping for the title-bar close button but does not explain this timer
case. Delay execution (`0x402468`) checks closure for all positive delays.
Clipboard waits (`0x402098`) enable monitoring only when a window exists at entry.

```sh
cargo build --locked
xvfb-run -a -s '-screen 0 1280x900x24' env -u WAYLAND_DISPLAY \
  QUICKER_COMPAT_CORPUS=/tmp/quicker-real-plugins \
  dbus-run-session -- python3 scripts/smoke-wait-windows-x11.py
QUICKER_COMPAT_CORPUS=/tmp/quicker-real-plugins \
  cargo test --locked downloaded_opencc_wait_window -- --ignored
```

The GUI test needs KWin, Xvfb, xdotool, xprop, D-Bus, and ImageMagick. It uses
isolated settings and an isolated desktop session. It checks all three activation
modes, position restoration after a manual move, closure outputs, return buttons,
long prompts, shared subprogram state,
programmatic closure, timeout behavior, cancellation, and action-end cleanup.
It also checks that monitored 30-second delay and clipboard waits finish within
three seconds of a button click. Clipboard closure returns failure outputs.

The GUI test verifies the pinned OpenCC hash. It executes the original update
step with and without an existing window. The original expression displays
`2/4` with controlled count/list variables. A screenshot confirms that progress.
This does not execute the complete OpenCC action. Its script, form, selection,
list-management, Windows-path, and complex-expression gaps remain.

The compatibility report now recognizes `sys:showWaitWin`. It reports active
unsupported options and defers dynamic option values to runtime validation.
X11 placement uses the desktop workarea. Per-monitor placement, native Wayland,
Markdown help, rich button syntax, and taskbar progress still need implementation.

## Input text-tool evidence

The MSI tool parser at RVA `0xc57c8` reads comma-separated tool names. The
configured file tool executes at `0xc968c`. The folder and save tools execute
at `0xc9930` and `0xc9ca8`. All four path tools send `IsFullContent=true`.
The input handler at `0x1c74f8` therefore replaces the complete text value.
Multiple-file selection joins paths with CRLF. The separate legacy context-menu
handlers use selection insertion. They do not define these configured buttons.
Custom replacement modes through `extraSettings` remain unsupported.

```sh
cargo build --locked
xvfb-run -a -s '-screen 0 1280x900x24' env -u WAYLAND_DISPLAY \
  QUICKER_COMPAT_CORPUS=/tmp/quicker-real-plugins \
  dbus-run-session -- python3 scripts/smoke-input-tools-x11.py kdialog
# Repeat with zenity to verify the GTK backend.
```

The test uses isolated KWin settings and the same dependencies as the waiting
window test. It also needs kdialog or zenity. It checks whole-value replacement,
Unicode paths, multiple-file CRLF output, save paths without file writes,
validation retry, exact multiline text, and picker/input/action cancellation.
Closing an input during a picker stops the picker. With `stopIfFail=false`,
that input failure does not cancel the action.

When a corpus directory is supplied, the test verifies the pinned OpenCC hash.
It executes `/Steps/0/IfSteps/0/IfSteps/0` without changing that JSON step.
The native picker returns a temporary directory through the original
`build目录` output. This verifies one module. The complete OpenCC action still
has script, form, list-management, selection, Windows-path, and expression gaps.

The checker accepts these four built-in tools. It still rejects unknown tools
and static custom settings. Native Wayland input tools remain unverified.
