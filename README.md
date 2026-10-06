# Quicker-RS

A Linux quick-action panel inspired by [Quicker](https://getquicker.net/), built
with Rust and egui. The Linux port is in active development. See
[PORTING.md](PORTING.md) for the remaining integration work and verification log.

## Build and run

Install a recent stable Rust toolchain, a C compiler, pkg-config, and the D-Bus,
X11/XCB, OpenGL, and xkbcommon development libraries. On Debian/Ubuntu:

```sh
sudo apt install build-essential pkg-config libdbus-1-dev libxcb1-dev libx11-dev libxkbcommon-dev libgl1-mesa-dev
cargo build --release --locked
./target/release/quicker-rs
```

X11 and Wayland windows are supported. The optional action helpers are
`xdg-open` (opening files/URLs), `xdotool` (X11 keyboard automation), `curl`
(download steps), and either `zenity` or `kdialog` (workflow dialogs). Install a
CJK font such as Noto Sans CJK to display Chinese action names.

## Desktop activation

On X11, **Alt+Space** toggles the panel. Choose another shortcut in Settings if
your window manager already uses it; Apply Settings rebinds it immediately.
On Wayland, add a custom keyboard shortcut in your desktop settings that runs:

```sh
quicker-rs --toggle
```

Launching a second instance shows the existing panel. Supported commands:

| Command | Behavior |
| --- | --- |
| `quicker-rs` or `--show` | Show the existing panel, or start it |
| `--toggle` | Show/hide the panel, or start it |
| `--hidden` or `--hide` | Start hidden, or hide an existing panel |
| `--quit` | Exit the existing instance |
| `--check-config` | Validate configuration without opening a window |
| `--check-plugin FILE` | Print a JSON compatibility report without executing the action |

On Linux, closing the window or pressing Escape at the panel root hides it.
Use **Quit** or `--quit` to exit. Escape during execution cancels the action.
The current right-drag radial menu operates inside the panel; global mouse
activation is still pending. On X11, keyboard actions hide the panel and
restore the captured target window before execution; a closed or unfocusable
target stops the action. Native Wayland keyboard input is not yet implemented
and these actions report an error.

## Actions and profiles

Use **+** to create a program, script, URL, file/folder shortcut, text snippet,
clipboard action, or group. Program arguments are separate values, so spaces
inside an argument do not need shell quoting. Use a shell action for shell syntax.
Every action has Edit and Delete controls. Save with **Ctrl+S**. Open a group to
add or edit its children.

The upper section contains global actions. The lower section uses the matching
application profile; configure process names under Settings → Profiles. Focus
detection currently uses the existing X11/KDE integration and has not yet been
verified on every compositor.

The automation editor supports key sequences and a subset of Quicker action
types 7, 11, 24, and 25. Paste Quicker JSON in its import area. This is not universal
compatibility with Quicker's Windows action ecosystem: Windows executables,
COM/PowerShell integrations, remote templates, and unsupported step runners need
Linux equivalents. Review imported actions before running them: actions can
execute commands, access files, and make network requests.

Basic actions support JSON import/export. The Quicker builder also exports
JSON. Imported documents retain unknown metadata, variables, subprograms, and
step options during visual editing. Unsupported steps remain editable JSON
cards. Preserving a step does not mean its runner is implemented on Linux.

## Check a shared action

Build the checker:

```sh
cargo build --locked
```

Download an action by shared ID or official share URL:

```sh
python3 scripts/check-shared-action.py 5e25bcf4-5a00-4272-eba7-08dd74a9f021 --revision 1
```

The official API requires authentication. Set `QUICKER_API_TOKEN` in your local
environment before this command. The tool sends it as a Bearer token to the
official API. It excludes the token from reports and the checker environment.
An anonymous request returned HTTP 401 during verification.

To check the recorded public author export without authentication:

```sh
python3 scripts/check-shared-action.py 6803b583-78f7-400d-a4c1-08de12ec7091 --public-export
```

This sample comes from the author's GitHub repository. The tool checks its
commit, SHA-256, and `SharedActionId`. It reports this source separately from the
official API. The sample currently returns code 1 because it needs missing modules.
The public registry contains one shared ID. It is not a mirror of the action store.

To check a local export:

```sh
python3 scripts/check-shared-action.py --file /path/to/action.json
# Direct Rust interface, without artifact storage:
./target/debug/quicker-rs --check-plugin /path/to/action.json
```

The report checks import, export, editor save, and a title-only edit. It lists
step runners and selected unsupported options, including nested steps and subprograms.
Disabled branches do not create runtime blockers. The checker never executes
the imported action. Runner availability does not prove compatible behavior.

The runtime evaluates a subset of `$=` expressions. This includes arithmetic,
comparisons, Boolean operators, conditional expressions, indexes, and selected
string methods. It preserves short-circuit behavior. Unknown syntax returns an
error. The evaluator does not provide arbitrary C# code or .NET host APIs.
It uses ordinal string matching. Case-insensitive ordinal matching currently
requires ASCII text. Reports check expression syntax, but runtime types still need validation.

Expressions support C# regular strings and verbatim strings (`@"text"`). Verbatim
strings retain backslashes, doubled quotes, and line breaks. Regular strings
support C# escapes, including `\x`, `\u`, and `\U`. Unpaired UTF-16 surrogates,
character literals, interpolation, and raw string syntax remain unsupported.

The pure `Path` methods are `GetDirectoryName`, `GetFileName`,
`GetFileNameWithoutExtension`, `GetExtension`, `HasExtension`, `GetPathRoot`,
`IsPathRooted`, `ChangeExtension`, and `Combine`. They use Linux path rules.
They do not inspect the filesystem or resolve symlinks. Dot segments remain
unchanged. `Combine` accepts text arguments or a text list. A later absolute
component replaces earlier components. Paths have a 1 MiB limit. Windows drive
paths, backslashes, and NUL characters produce errors in path methods.
Literal concatenation retains backslashes and receives a compatibility warning
when it constructs a path. Such output still requires replacement before file
operations. Path constants and methods that access the environment remain unsupported.

`sys:regexExtract` supports all-match values, the first match's groups, and
per-group lists. It returns `matches` and `match1` through `match5`, including
the trailing-space keys used by Quicker exports. Optional groups produce empty
strings. Failed extraction clears the outputs. Right-to-left matching and native .NET
match objects remain unsupported. Regex syntax, Unicode classes, and culture
rules can differ from .NET. Limits are 64 KiB per pattern, 16 MiB of input and
extracted text, and 100,000 matches. The engine also limits backtracking.
Cancellation is checked between match searches. The .NET three-second timeout
is not reproduced. Group extraction uses the JSON editor to retain all outputs.

`sys:stringProcess` supports `trim`, `trimStart`, `trimEnd`, `toLower`, `toUpper`,
and UTF-8 `urlEncode`. These methods are available in the action editor.
Unsupported methods retain their JSON instead of becoming lowercase operations.
Both text modules report success and errors and honor `stopIfFail`.

`sys:listOperations` supports 31 operations on text lists. These include updates,
negative indexes, slices, concatenation, distinct values, regex filters, text
filters, and sorting. Updates change the bound list variable. Queries and sorting
return separate values. Filter exclusions use distinct values, as in the MSI.
`sys:comment` does not evaluate inputs or execute child steps.

List limits are 100,000 items and 16 MiB of text. Cancellation is checked between
regex searches. Fuzzy/pinyin filtering and `stopIfFail=false` remain unsupported.
The supplied MSI stops on list errors. Its `removeAt` applies the list length
twice to negative indexes that remain negative after the first adjustment.
Other indexed updates reject positions outside the list. Slices use Skip/Take
behavior and can return an empty list. Sort order and Unicode case rules can
differ from Windows. File metadata sorting requires native regular files and
available timestamps. Lists do not share .NET reference identity. These modules
retain JSON editor cards to preserve all bindings.

`sys:readFile` reads text as UTF-8 by default. `sys:WriteTextFile` creates parent
directories and supports overwrite, append, BOMs, and newline conversion.
Both modules support UTF-8, UTF-16, UTF-32, and ASCII. Unicode BOMs override the
selected read encoding. Writes use CRLF when `addNewLine` is true and no newline
format is selected. UTF-8 PowerShell files retain a BOM, as in the MSI.
Failed operations set `isSuccess=false`. Missing `stopIfFail` defaults to true.

Text files require native paths and regular files. Paths retain whitespace.
Backslashes and Windows drive paths require replacement on Linux. Reads expand
`%NAME%` from the process environment. Windows default code pages, automatic
encoding detection, UTF-7, and legacy encodings remain unsupported.
Invalid input bytes and non-ASCII text in ASCII writes produce errors instead
of replacement characters. Text and encoded files have a 16 MiB limit.
Cancellation is checked between 64 KiB transfers. Interrupted writes can leave
partial content. The web preview cannot read or write local text files.

`sys:MsgBox` supports standard OK, OK/Cancel, Yes/No, and Yes/No/Cancel buttons.
It returns the selected `result` and `okOrYes`. Custom message boxes remain
unsupported. Dialog appearance and icons depend on kdialog or zenity.
Closing a Yes/No dialog returns No. Button workflows retain JSON editor cards.

`sys:userInput` supports text and multiline input, required values, and regex
validation. It preserves trailing spaces and newlines. Failed input clears outputs.
Number/date input, text tools, Markdown help, custom placement, and advanced
window behavior remain unsupported. The checker reports these options.
`sys:selectFolder` uses `initDir` and clears its path after a failed selection.
It does not list open file manager windows. Both modules default `stopIfFail` to true.

`sys:selectFile` supports `openFile`, `openMultiFile`, and `saveFile`. It returns
`path` for single-file/save selection and `pathList` for multiple files.
Filters, initial paths, initial filenames, and default extensions are supported.
The module selects the first filter that matches `defaultExt`. Native filter
matching and extension insertion can differ from Windows. If the backend returns
a save path without an extension, the module adds `defaultExt`. It asks before
replacing a file at that adjusted path. Selection itself never writes the file.
Failed selection retains previous path outputs and sets `isSuccess=false`.
Missing `stopIfFail` and `topMost` both default to true. Set `topMost=false` on
Wayland. On X11, the window manager handles the EWMH above hint.
The module uses local Qt/GTK windows so action cancellation can close them.
Remote URLs and Windows initial paths are unsupported. Filter patterns must
contain `*` and cannot contain spaces, brackets, or parentheses.

All four Linux dialog modules respond to action cancellation. They stop the
managed dialog process and do not continue the workflow after cancellation.
Focus restoration requires X11. Disable `restoreFocus` on native Wayland.
The X11 tests cover local kdialog and zenity windows. Desktop portal dialogs
and native Wayland behavior require separate validation.

`sys:showText` uses native application windows. It supports `WAIT`, `NO_WAIT`,
`CLOSE_WINDOW`, `GET_WIN_INFO`, `APPEND_TEXT`, `ACTIVATE_WINDOW`, and `WAIT_CLOSE`.
Windows can remain open after an action ends. A nonempty `autoCloseKey` identifies
one window. `=` uses the action state scope. Empty keys create separate windows.
`updateIfExists` updates the document in an existing window. WAIT returns edited
text, selected text, a UTF-16 caret offset, and a plain toolbar button value.
Closing a window normally succeeds. Cancelling an action closes its waiting
window and stops execution, even when `stopIfFail=false`.

Text windows support line numbers, wrapping, font size, colors, topMost,
Escape, close-on-blur, centered placement, and pixel dimensions. The native
editor toolbar provides copy, select all, reset, wrap, and close. Its commands
and shortcuts differ from the Windows editor. Limits are 32 open windows and
1 MiB per document. Native window handles, window enumeration, syntax highlighting,
custom fonts, percentage dimensions, other placement modes, saved placement,
whole-line copy, state autosave, and advanced handlers remain unsupported.
The checker reports those requests as blockers. The X11 test executes all three
active OpenCC text-window steps without changes. Native Wayland window behavior
still needs validation. Complex C# expression execution is deferred.

`sys:showWaitWin` supports show, update, check, close, waitClose, and
showAndWaitClose in the native X11 application. A root action and its subprograms
share one window. Showing an existing window updates its content. Updating a
missing window does nothing. Windows close when their root action ends.
The module returns closure status and plain button values. Buttons and
programmatic closure continue the action. With `stopActionIfClose=true`, the
close button and automatic timeout stop the action, matching the inspected MSI.
Set this option to false when a timeout must continue execution.

Wait windows support progress, negative countdown values, font size, automatic
closure, all three activation modes, and position reuse within an action.
Placement uses the X11 desktop workarea, not individual monitor boundaries.
Plain buttons remain accessible when the prompt needs scrolling. Limits are
32 windows, 64 KiB per text field, 32 buttons, and a one-day automatic timeout.
Markdown help, icons, rich button syntax, Windows taskbar progress, and native
Wayland placement/focus remain unsupported. The checker identifies unsupported
options. Selection windows and complex C# execution remain deferred.

With `monitorWaitWin=true`, a delay ends when the window closes, including when
no window exists. Clipboard waits monitor only a window that exists at entry.
Closure before a clipboard change returns failure and obeys `stopIfFail`.
Action cancellation always stops execution. Delay values require 32-bit integers.
A missing value defaults to 100 ms. Empty text, zero, and negative values continue
immediately.

Control flow supports `sys:if`, `sys:simpleIf`, `sys:repeat`, sequential `sys:each`,
`sys:break`, and `sys:continue`. Nested loops handle break, continue, stop, and
cancellation. `simpleIf` has one branch. New editor branches use `sys:if`.
Parallel list execution is unsupported. Loop progress bars are not displayed.

On X11, `sys:waitClipboardChange` uses XFixes events and supports recent changes,
timeouts, and cancellation. `sys:getSelectedText` sends Ctrl+C and reads fresh
plain text or HTML. Some X11 applications emit no event for a repeated copy.
For plain text, the fallback reads PRIMARY only if its owner belongs to the
focused window. It then copies that text to CLIPBOARD. Selection reads support retry,
trimming, and URL encoding. They do not return a source URL.
UI Automation, action-parameter text, and native Wayland clipboard events remain
unsupported and are reported by the checker.

`sys:getSelectedFiles` supports `getSelection` on X11. It sends Ctrl+C and requires
a new clipboard event. It reads local file URIs, preserves Unicode paths, and
returns file lists, names, first-file outputs, and a count. Failed reads clear
these outputs. Repeated copies depend on the file manager sending a new event.
Transfers support INCR, a 16 MiB limit, cancellation, and a five-second deadline.
Filename and natural sorting can differ from Windows locale sorting. Size and
timestamp sorting require local regular files and available metadata.
`setSelection`, remote file URIs, and native Wayland remain unsupported.
The [file selection test](scripts/smoke-files-x11.py) exercises Dolphin and can
run the downloaded QuickLook workflow with a Linux preview program.

`sys:activateProcessMainWindow` supports X11 window activation by PID, executable
name/path, or application class. Class and title filters use regular expressions.
It verifies focus before returning PID, window ID, and title outputs. It can
start a missing application from an executable path, or send a tray activation
hotkey. The hotkey accepts one .NET SendKeys chord, such as `^%q` or `+{F12}`.
Program paths are executable paths without shell arguments. Startup waits up to
five seconds. Hotkey activation waits up to one second. Native Wayland window
activation remains unsupported.

`sys:run` starts Linux programs with Quicker argument quoting, alternate paths,
working directories, and environment overrides. Arguments are passed directly;
shell operators are literal. It supports detached launches, exit waits, PID and
exit-code outputs, and bounded stdout/stderr capture. Captured output requires
UTF-8; `oem` selects native UTF-8 on Linux. Nonzero exit codes do not indicate a
launch failure. Cancellation stops waited processes and their descendants.
Detached applications continue running after the action ends.

The run module can reuse an existing X11 application window. Window outputs for
a new detached process are a single immediate query, so they can be empty before
the window appears. Documents and URIs use desktop handlers and do not provide
application PID, exit status, or captured output. Windows accounts, elevation,
non-normal window styles, and `waitInputIdle` are unsupported. Windows executable
paths need installed Linux equivalents. The checker reports these requirements.

`sys:keyoperation` can read X11 key state, press a key, or release it.
It accepts Windows key names, decimal codes, and hexadecimal codes. Generic
modifiers read both sides. CapsLock and NumLock state uses XKB locked modifiers.
Left, middle, and right mouse buttons support state reads only. Physical device
state, side mouse buttons, and Quicker virtual keys remain unsupported.
The installed keyboard layout must contain the requested key. Keys pressed by
an action are shared with its subprograms and released when the action ends,
fails, or is cancelled. Keys held before the action are not owned by this cleanup.

`sys:outputText` supports clipboard paste and simulated text input. Input accepts
Unicode, converts CRLF to LF, and supports a per-character delay and an optional
final Return. It does not change the clipboard. The X11 text backend uses
xdotool's 12 ms key timing so temporary Unicode mappings remain available to
the target. A zero character delay adds no further wait. Cancellation is checked
between batches of at most 32 characters and during requested delays. Each batch
finishes its key releases and modifier restoration before cancellation.
Paste defaults to 50 ms before Ctrl+V and 10 ms after it. Both modes report
`isSuccess` and honor `stopIfFail`; cancellation always stops the action. Empty
content is skipped. Clipboard history exclusion remains unsupported. Input
methods and target applications can affect simulated typing.

`sys:subprogram` runs action-local subprograms by name. Each call has fresh typed
variables. Inputs and outputs use `var:KEY` bindings and the variable's `IsInput`
and `IsOutput` flags. Calls can use definitions in the current or parent scope.
Normal `sys:stop` returns to the caller. `method=forcestop` stops the whole action.
Cancellation propagates through calls. Calls have a depth limit of 32.

External subprograms load from `$XDG_CONFIG_HOME/quicker-rs/subprograms`, or
`QUICKER_SUBPROGRAM_DIR`. A shared `@@GUID@REVISION@TITLE` reference reads
`shared/GUID/REVISION.json`. This file must be a SharedActionDto with the matching
`Id`, `Revision`, `ActionType: 25`, and workflow `Data`. A global `%%GUID` reference
reads `global/GUID.json`, containing the exported subprogram and matching `Id`.
The runtime does not download missing files. The checker reports missing dependencies
and inspects resolved bodies without executing them. Server templates remain unsupported.
List and dictionary inputs currently use value copies. Shared mutable object behavior
still needs implementation before modules that mutate these objects are supported.

Download and inspect shared dependencies with the action:

```sh
python3 scripts/check-shared-action.py 6803b583-78f7-400d-a4c1-08de12ec7091 --public-export --with-dependencies
```

The tool writes dependencies under `.compat/subprograms` by default. Use
`--dependency-dir PATH` to select a directory. The checker reads that directory
for this invocation. To use these files in the application, set
`QUICKER_SUBPROGRAM_DIR` to its absolute path when starting Quicker RS.
The tool follows shared calls inside downloaded and local global subprograms.
It deduplicates ID/revision pairs and limits the graph to 128 files and 32 levels.
Cached downloads must match their recorded hash. A failed dependency download
keeps the root report and records an error under `dependencies.items`.
Official downloads require `QUICKER_API_TOKEN` when the API rejects anonymous access.
Type 25 subprogram documents also support import, editing, and static inspection.

| Exit code | Meaning |
| --- | --- |
| 0 | Preservation checks passed. No known static blockers. Runtime validation remains required. |
| 1 | The report found a preservation failure or a runtime blocker. |
| 2 | Input, download, authentication, or checker failure. |

The Python tool stores `response.json`, `action.json`, `source.json`, and
`report.json` under `.compat/<id>/<response-sha256>/`. Use `--output-dir` to change
this location. Use `--binary` to select a release build. Rust callers can use
`quicker_rs::check_plugin_json` or `quicker_rs::check_plugin_file`.
See [the compatibility evidence](tests/compat/README.md) for API details and test results.

## Configuration

Settings live in `$XDG_CONFIG_HOME/quicker-rs/config.toml`, normally
`~/.config/quicker-rs/config.toml`. Workflow state is in `action_state.json` next
to it. Existing configs are never populated with demo actions on startup.
Invalid configs produce an error without overwriting the file. Saves use atomic
replacement, and errors are shown in the UI.

Scripts run on worker threads with bounded captured output (1 MiB per stream).
Cancellation terminates the Unix process group for managed subprocesses.
Programs launched as independent applications continue running.

## Install

```sh
make install                   # builds release and installs under ~/.local
make uninstall                 # keeps configuration and workflow state
```

Ensure `~/.local/bin` is on your desktop session's PATH. To start at login, add
`quicker-rs --hidden` to your desktop's autostart settings. Packaging can use
`make install PREFIX=/usr DESTDIR=/path/to/staging`.

## Verify

```sh
cargo test --all-targets --locked
cargo check --target wasm32-unknown-unknown --lib --locked
cargo build --locked
xvfb-run -a -s '-screen 0 1280x900x24' python3 scripts/smoke-x11.py
xvfb-run -a cargo test clipboard_persists_after_action_worker_exits -- --ignored
```

The GUI smoke test requires Xvfb, xdotool, ImageMagick, and Python 3.11+. It uses
temporary configuration and runtime directories, leaving your desktop config
unchanged. The clipboard test uses an isolated display and checks ownership
from a second process. The wasm build remains a browser UI preview; desktop
automation is unavailable there.
