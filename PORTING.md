# Linux port work log

The target is a usable Linux Quicker: global activation, a contextual action
panel, editable shortcuts and automation, import/export, reliable execution,
persistent settings, and desktop installation. The existing Windows MSI is a
reference artifact, not an executable dependency of the Linux implementation.

## Initial audit (2026-10-06)

- Native egui application and Quicker JSON interpreter exist.
- Baseline `cargo test --all-targets`: 46 pass, 12 fail due to missing fixtures.
- Config loading overwrites invalid files; loading also silently repopulates
  user profiles. Saves are non-atomic and errors are hidden from the UI.
- Subprocess output pipes are drained only after exit, causing deadlock on
  large output; cancellation only kills the immediate shell.
- Clipboard writers are dropped immediately, losing Linux selection ownership.
- Only plugin actions can be edited/deleted in the panel.
- Global hotkey support is X11-only; there is no single-instance activation
  command, desktop integration, or global mouse activation.
- Keyboard automation runs while the launcher has focus.
- Per-action hotkeys are stored but never registered.
- The radial menu is local to the window. Native Wayland focus/input and
  compositor integration need explicit implementation and verification.
- README is still a prototype stub. No native Linux CI or installation guide.

## Completion evidence needed

- Reproducible unit and integration checks with committed fixtures.
- Native runtime tests of activation, focus restoration, clipboard ownership,
  action CRUD, profile routing, and execution/cancellation.
- X11 and supported Wayland activation paths tested and limitations documented.
- Installation assets and accurate user documentation.
- Full review of supported workflow semantics and import/export preservation.

Keep this list aligned with actual evidence; passing unit tests alone does not
establish completion of the port.

## Implemented and checked in the first porting pass

- Config load fails without modifying invalid files. Removed destructive
  automatic demo/profile repopulation. Validated settings and atomic saves;
  UI save errors are surfaced. Workflow-state writes are also atomic.
- Managed subprocesses drain both output streams concurrently, cap captured
  output, and cancel the Unix process group. Launched applications are reaped.
- Persistent clipboard owner: verified by a second process after the writer
  thread exits, under Xvfb.
- Basic-action forms, JSON import/export, all-action edit/delete, Ctrl+S.
- Linux single instance with a private runtime socket, show/toggle/hide/quit
  commands, hidden startup, close-to-hide, explicit Quit. Shortcut settings
  rebind on X11; Wayland users can bind the activation command in their desktop.
- Raw Quicker import/export retains unknown document metadata. Unicode variable
  expansion no longer consumes characters following multibyte variable names.
- Committed authored fixtures replace unavailable prototype samples. They are
  not evidence of compatibility with the original missing exports.
- Xvfb GUI smoke test passed activation plus action create/edit/delete with
  config-file assertions. Screenshot inspection covered panel and basic editor.
- Native Clippy with `-D warnings` passed. Wasm library check passed.
- Final unit run with DISPLAY and WAYLAND_DISPLAY removed: 64 passed, one
  display-dependent clipboard test ignored. That test passed separately under
  Xvfb. Desktop-entry validation and `git diff --check` passed.
- Added desktop/icon assets, Makefile installation, Linux CI, and README.

## Step 2: X11 input target (2026-10-06)

- Capture the target window before first startup and immediately before showing
  the panel. Keep the last external window while the launcher has focus.
- Hide the panel before starting keyboard workflows, then restore the recorded
  recipient through the window manager and verify actual input focus. Closed
  targets stop execution; errors reopen the panel.
- Handle nested input steps and disabled branches. Pass literal text after `--`
  to xdotool so leading dashes are not interpreted as options.
- `scripts/smoke-input-x11.py` verifies real terminal input, focus, panel hiding,
  and closed-target failure. Passed on bare Xvfb and KWin X11, including visible
  first startup (`--visible-start`) and hidden startup.
- Native tests: 65 passed, one display-dependent test ignored. Native Clippy
  passed with warnings denied. Wasm check passed with existing dead-code warnings.

## Step 3: Preserve imported plugins during editing (2026-10-06)

- Preserve document and step metadata, variable definitions, subprograms,
  disabled flags, and unknown fields. Step identity survives reordering.
  Unsupported steps use JSON cards. Keep launcher hotkeys and tags on save.
- Native tests: 70 passed, two opt-in tests ignored. Clippy passed with warnings
  denied. An Xvfb editor save preserved a real imported plugin and launcher fields.
- Downloaded five Citavi exports from
  [myCitaviMacros](https://github.com/AlexShyXie/myCitaviMacros/tree/50fd22f27e4c752459ef9fe0dc1c96d490cef81d/Quicker%E5%8A%A8%E4%BD%9C%E9%A2%84%E8%AE%BE)
  and a [QuickLook export](https://www.getquicker.net/Common/Topics/ViewTopic/24595).
  All six passed raw round-trip, builder round-trip, and title-only editing
  assertions. This verifies preservation, not execution of their Windows tools.
- Repeat against downloaded JSON files with `QUICKER_COMPAT_CORPUS=/path/to/json
  cargo test downloaded_actions_round_trip_without_data_loss -- --ignored`.

## Step 4: Download and inspect actions by ID (2026-10-06)

- Added `scripts/check-shared-action.py` for shared IDs, official URLs, and local
  exports. It stores the response, action document, source hash, and JSON report.
- Added `--check-plugin FILE` and Rust interfaces. The report checks preservation
  and lists missing runners with their paths. Execution and inspection share the
  runner mapping. The checker does not execute actions.
- Verified API route, Bearer authentication, and `ApiResult<SharedActionDto>`
  against MSI assemblies. Anonymous official download returned HTTP 401.
- Downloaded the pinned public `Ref->Ob` export by its shared ID. Its hash and ID
  matched. Preservation passed. Runtime inspection identified missing modules.
  Repeated the checks on all six downloaded exports with the same distinction.
- Native tests: 74 passed, two opt-in tests ignored. Python tests: 11 passed.
  Native Clippy passed with warnings denied. Wasm check passed with existing warnings.
- See `tests/compat/README.md` for reproduction steps and limits.

## Step 5: Evaluate plugin expressions and typed variables (2026-10-06)

- Replaced expression-as-text behavior with a pure `$=` evaluator. It supports
  arithmetic, comparisons, Boolean short circuit, conditional expressions, list
  and dictionary indexes, and selected string methods. Unsupported syntax fails.
- Added typed defaults and output conversion for text, numbers, integers,
  Booleans, lists, and dictionaries. Default expressions use declaration order.
- Added expression syntax diagnostics to downloaded-action reports. No action
  runs during inspection. C# host APIs and full language semantics remain unsupported.
- Tests cover false branches, failed expressions, assignment failure outputs,
  the downloaded Citavi `IndexOf` condition, UTF-16 positions, and integer precision.

## Step 6: Execute plugin branches and loops (2026-10-06)

- Added `sys:if`, `sys:repeat`, sequential `sys:each`, `sys:break`, and
  `sys:continue`. Checked execution order against managed bodies in the MSI.
- Corrected `simpleIf` to execute only its true branch. New editor branches
  emit `sys:if`. Imported documents retain their runner keys and metadata.
- Added nested loop control, typed items, index outputs, stop conditions,
  failure outputs, counter validation, and cancellation of unlimited loops.
- Reports reject parallel each execution and loop control outside a loop.
  They warn about loop progress displays, which remain unsupported.
- Native tests: 91 passed, two opt-in tests ignored. Python tests: 11 passed.
  Clippy passed with warnings denied. The six-file preservation test also passed.
  Wasm check passed with dead-code warnings in the preview build.
- Repeated the public ID download. Its hash matched. `Ref->Ob` still needs
  subprograms, clipboard-change waits, and process window activation.
  Preservation passes do not establish execution compatibility.

## Step 7: Execute clipboard waits and selected-text reads (2026-10-06)

- Added XFixes clipboard monitoring with X server timestamps. It detects repeated
  ownership events and distinguishes CLIPBOARD from PRIMARY without storing contents.
- Added `sys:waitClipboardChange` with recent changes, pre-Ctrl+C sequence tracking,
  fractional timeouts, failure outputs, and cancellation.
- Added `sys:getSelectedText` with Unicode text, HTML, retries, trimming, and URL
  encoding. A plain-text fallback only reads PRIMARY owned by the focused window.
  This handles X11 applications that omit events for repeated copies.
- The checker reports unsupported formats, UI Automation, action parameters,
  wait-window monitoring, source URL output, and the X11 backend requirement.
- Native tests: 96 passed, five opt-in tests ignored. Three X11 opt-in tests passed
  separately. They cover clipboard events, rejection of another window's selection,
  and cancellation during a wait. The real launcher workflow passed twice with
  identical selected text. Clippy and Wasm checks passed, with preview dead-code
  warnings. Python tests: 11 passed. Added the new X11 checks to CI.
- The six real exports still pass preservation. The five Citavi workflows now
  lack only subprogram and process-window activation runners. Other runtime
  dependencies and option semantics still need validation.

## Step 8: Activate application windows on X11 (2026-10-07)

- Added `sys:activateProcessMainWindow`. It matches a process name, executable
  path, or PID and applies optional window class and title regular expressions.
  It verifies focus and returns the process ID, window handle, and title.
- Added program launch when the process is absent, a single SendKeys hotkey
  fallback, bounded waits, failure outputs, and cancellation. Native Wayland
  reports an unsupported backend. Application tray hotkeys still need testing.
- Checked input fields and execution behavior against the MSI and official
  module documentation. Reports flag invalid literal options and runtime
  requirements. Inspection does not execute actions.
- Native tests: 98 passed, six opt-in tests ignored. The window query opt-in test
  passed separately. The launcher test used two real windows and verified that
  input reached only the selected window. Program launch with spaces in its path
  also passed. The same test passed with KWin and a minimized target window.
- Python tests: 11 passed. Clippy passed with warnings denied. Wasm check passed
  with preview dead-code warnings. Added the X11 window checks to CI.
- Downloaded the public `Ref->Ob` action again by ID. Its hash matched. All six
  downloaded exports passed preservation checks. The five Citavi actions now
  lack only the subprogram runner. QuickLook still lacks selected-file, key-operation,
  and run runners. These checks do not establish execution of Windows applications.

## Step 9: Execute and resolve subprogram calls (2026-10-07)

- Added local subprogram lookup by name through current and parent scopes.
  Calls initialize independent typed variables and transfer declared `var:` inputs
  and outputs. Normal returns stay within the call. Forced stops leave the action.
- Added bounded recursion, cancellation propagation, failure outputs, and action
  state sharing. Input-target detection follows calls, including nested definitions.
- Added external dependency loading for global exports and shared revision files.
  The loader checks IDs, revisions, type 25, size limits, and template requirements.
  Static reports inspect resolved bodies and block unresolved dependencies.
- Checked the call body, variable mapping, lookup order, action type, and download
  route against MSI managed methods. The real shared dependency returned HTTP 401
  without authentication. Shared collection mutation and server templates remain gaps.
- Native tests: 106 passed, six opt-in tests ignored. Python tests: 11 passed.
  Clippy and Wasm checks passed, with existing preview dead-code warnings.
  Tests cover local and pinned shared calls, nested lookup, return and failure
  behavior, cancellation during execution, and mismatched IDs and revisions.
- Repeated the public action download. Preservation still passes. The report now
  identifies the missing shared subprogram and revision instead of a missing runner.

## Step 10: Download and inspect subprogram dependencies (2026-10-07)

- Added recursive `--with-dependencies` downloads and `--dependency-dir` selection.
  Calls inside shared and local global subprograms participate in the dependency
  graph. The tool deduplicates revisions, bounds the graph, validates identity and
  type, and checks recorded cache hashes. It does not execute downloaded steps.
- Failed dependency downloads retain the parent report and report a blocker with
  a specific error. The checker reads the selected dependency directory and
  inspects child bodies. Added import and preservation support for type 25 exports.
- Repeated the real public action download with dependencies. Both calls resolved
  to one ID/revision request. HTTP 401 produced an authentication error in the
  report and no dependency file. Successful authenticated download remains unverified.
- Native tests: 106 passed, six opt-in tests ignored. Python tests: 17 passed.
  Clippy and Wasm checks passed, with preview dead-code warnings. Tests use the
  production Rust checker to detect missing runners inside downloaded dependencies.

## Step 11: Run programs from imported workflows (2026-10-07)

- Added `sys:run` with direct argument passing, Windows CRT argument parsing,
  alternate paths, working directories, and child environment overrides.
  Type 11 launch arguments now use the same parser.
- Added detached execution, exit waits, PID and exit-code outputs, bounded
  stdout/stderr capture, failure outputs, and cancellation of waited process
  groups. Nonzero exit codes preserve successful launch status, as in the MSI.
- Added desktop handlers for files and URIs, plus existing-window reuse through
  the X11 backend. Unsupported Windows options fail before launch. Reports flag
  Windows targets and dynamic options. Native output decoding requires UTF-8.
- Native tests: 115 passed, six opt-in tests ignored. Python tests: 17 passed.
  Clippy passed with warnings denied. Wasm check passed with preview dead-code
  warnings. The X11 workflow passed with real windows and verified `sys:run`
  window reuse, PID, handle, and subsequent keyboard delivery.
- All six downloaded exports passed preservation. QuickLook now has two missing
  runners: selected files and key operations. Its text-input mode and Windows
  executable still require implementation or replacement. This is not proof
  that the complete QuickLook workflow executes on Linux.

## Step 12: Operate individual keys on X11 (2026-10-07)

- Added `sys:keyoperation` for state reads, key presses, and key releases.
  Verified the operation body and key parser against the MSI. Names, decimal
  codes, and hexadecimal codes map to keys in the installed X11 layout.
- Added XTEST injection, XKB lock-state reads, generic and side-specific
  modifiers, and left/middle/right mouse button state reads. Native Wayland,
  physical state, side mouse buttons, and Quicker virtual keys remain gaps.
- Shared key ownership through subprogram calls. Action teardown releases keys
  pressed by that action after completion, failure, or cancellation. It does not
  claim keys already held by another source. Input steps restore the target;
  state-only workflows do not require a target window.
- The isolated X11 test passed state reads, CapsLock toggling, nested ownership,
  stop/error/cancel cleanup, and preservation of an externally held key. The
  launcher workflow passed actual Shift+A and Space input to the target window,
  plus failure after the target closed. Both checks are included in CI.
- Native tests: 117 passed, seven opt-in tests ignored. Clippy passed with
  warnings denied. The new X11 opt-in test passed separately. Wasm check passed
  with preview dead-code warnings. All six real exports passed preservation.
  The QuickLook report now has one missing runner: selected files.

## Step 13: Send text through imported workflows (2026-10-07)

- Added `sys:outputText` input mode with Unicode, CRLF normalization, character
  delays, appended Return, success output, and failure control. Empty content
  leaves input and clipboard unchanged. Paste now uses documented delay defaults
  and rejects unsupported clipboard-history exclusion before writing.
- Bounded typing batches let complete key releases and modifier restoration
  finish before cancellation. Requested delays remain cancellable. Set xdotool
  key timing to 12 ms after the real test showed Unicode loss at zero delay.
- Native tests: 121 passed, seven opt-in tests ignored. Python tests: 17 passed.
  Clippy and Wasm checks passed, with preview dead-code warnings. Full native
  tests passed outside the sandbox after socket tests hit sandbox restrictions.
- X11 tests received exact Chinese, emoji, literal dashes, and newline bytes,
  preserved a held Shift key, and verified cancellation before the next character
  and state write. Both new workflows are included in CI.
- The real QuickLook report now has two blockers: the selected-files runner and
  the Windows QuickLook executable. A complete Linux workflow still needs both.

## Step 14: Read selected files and execute a downloaded workflow (2026-10-07)

- Added `sys:getSelectedFiles` get mode after inspecting the MSI methods and
  official module definition. It reports typed lists, names, count, success,
  and errors. Failed reads clear outputs. Cancellation always stops execution.
- Added X11 file clipboard transfers with a fresh-event requirement, owner and
  target checks, INCR support, a 16 MiB limit, and cancellation. URI validation
  rejects remote paths and invalid encodings. Sorting supports names, sizes,
  and available timestamps. Windows locale ordering can differ.
- Dolphin passed real selection, repeated identical copies, Unicode and space
  paths, natural sorting, and stale clipboard rejection. Transfer tests passed
  GNOME fallback, INCR, size checks, owner changes, timeout, and cancellation.
- Ran the downloaded QuickLook forum workflow twice. Replaced its Windows
  executable path with Linux ImageMagick display. Both selected images opened,
  and the workflow completed. The original branches, loop, formatting, and
  launch parameters remained intact. Windows QuickLook itself was not tested.
- Native tests: 126 passed, eight opt-in tests ignored. Python tests: 17 passed.
  Clippy and Wasm checks passed, with preview dead-code warnings. Six real
  exports passed preservation. The public ID download and hash check passed.
  Both new X11 checks are included in CI, which has not run remotely.
- The original QuickLook report now has one blocker: its Windows executable.
  Native Wayland, remote file selections, and setting selections remain gaps.

## Step 15: Correct real plugin text extraction (2026-10-07)

- Found a semantic error in the downloaded `xID->CITAVI` workflow. The prototype
  treated the extraction mode as a capture index and never wrote its second
  group. Its required `trim` operation was also unsupported.
- Verified modes, legacy aliases, missing-group values, and outputs against the
  MSI and official docs. Implemented all-match values, first-match groups, and
  per-group lists, including trailing-space output keys and .NET group ordering.
- Added output clearing, failure policy, cancellation checks, size limits, and
  static diagnostics. Right-to-left matching and native .NET objects remain
  unsupported. Regex engine syntax and timeout semantics can differ from .NET.
- Added trim variants and uppercase conversion to runtime and editor. Unsupported
  text methods retain JSON. Group extraction uses JSON cards to keep all outputs.
- The downloaded action's original text steps returned the expected name and ID.
  GUI saving preserved its complete JSON, tags, and hotkey. Six downloaded exports
  passed preservation. The full Citavi workflow still needs shared subprograms.
- Native tests: 133 passed, nine opt-in tests ignored. Both corpus tests passed
  separately. Python tests: 17 passed. Clippy and Wasm checks passed, with preview
  dead-code warnings. The real action report now has two dependency blockers.

## Step 16: Return dialog choices and cancel dialog processes (2026-10-07)

- Checked message-box outputs and input emptiness against the MSI. Standard
  message boxes now return OK, Cancel, Yes, or No and the corresponding Boolean.
  Empty titles use the action title, including calls through subprograms.
- Added managed cancellation to Linux message, input, and folder dialogs.
  Cancellation stops the workflow even when `stopIfFail` is false. Dialog focus
  no longer replaces the launcher's remembered external input target.
- Text input preserves whitespace, supports multiline entry on both backends,
  and retries required-value and regex validation. Failed input and folder
  selection clear outputs. Missing `stopIfFail` now defaults to true.
- Added static diagnostics for unsupported dialog options. Custom message boxes,
  number/date input, text tools, and advanced window options remain gaps.
  Focus restoration requires X11. Desktop portal behavior remains unverified.
- Real kdialog and zenity tests passed all standard button results, Unicode text,
  trailing newlines, initial folders, user cancellation, and action cancellation
  for all three dialog types. Tests also cover overridden Zenity exit settings.
- Downloaded a pinned OpenCC export. All preservation checks passed. Its report
  identifies text tools, script/file modules, and expressions that still need work.
  This is static evidence, not a successful OpenCC execution.
- Native tests: 138 passed, nine opt-in tests ignored. Seven downloaded exports
  passed preservation. Both corpus checks and 17 Python tests passed. Clippy
  and Wasm checks passed. CI includes both GUI checks but has not run remotely.

## Step 17: Execute text file modules from the OpenCC export (2026-10-07)

- Checked `ReadFileStep` and `WriteTextFileStep` in the MSI, including the
  execution closures, overwrite helper, and newline normalization method.
- Added text reads and `sys:WriteTextFile`. Implemented Unicode encodings,
  ASCII, BOM detection, parent directory creation, overwrite, append, newline
  conversion, and the UTF-8 PowerShell BOM exception. Failure stops execution
  by default. A suppressed failure returns `isSuccess=false`.
- Limited text operations to regular files and 16 MiB. Checked cancellation
  between transfers. Kept native path whitespace. Added explicit errors for
  Windows paths, unsupported encodings, invalid bytes, and ASCII data loss.
  This strict encoding behavior differs from .NET replacement fallback.
  Cancellation or I/O failure can leave partial writes.
- Executed ten unchanged file steps from the downloaded OpenCC JSON with native
  path variables. Verified Unicode content and file outputs. Two steps with
  Windows path templates correctly failed. This test does not execute the full
  OpenCC workflow or its external Windows commands.
- Seven downloaded exports passed preservation. All three corpus tests passed.
  Native tests: 145 passed, ten opt-in tests ignored. Python tests: 17 passed.
  Clippy passed with warnings denied. Wasm compiled with preview dead-code
  warnings. Repeated the ID download with dependency checks: the public
  `Ref->Ob` export downloaded and passed preservation. Its official dependency
  returned HTTP 401 and remains a reported runtime blocker.

## Step 18: Native file selection and OpenCC save workflows (2026-10-07)

- Checked `SelectFileStep`, its closures, and the three `AppHelper` file-dialog
  methods in the MSI. Added `openFile`, `openMultiFile`, and `saveFile` runners.
  Single/save selection returns a path. Multiple selection returns a list.
  Success clears the inactive output slot. Failure retains previous path outputs.
- Added filters, default filter selection, initial folders/names, default
  extensions, and overwrite confirmation after extension insertion. The chooser
  does not write a file. Unsupported paths and filter patterns produce errors.
- Kept spaces, quotes, percent signs, separators, Unicode, and embedded newlines
  in selected names. File URLs and a per-dialog random separator prevent ordinary
  filename characters from becoming list delimiters. Captured output is bounded.
- Managed child processes close on action cancellation. Local Qt/GTK choosers
  avoid portal processes that cannot share that cancellation. Added an X11
  above hint for `topMost`, which defaults to true. Wayland requires
  `topMost=false` and remains unverified. Native filter/extension rules can differ
  from Windows. File selection retains JSON editor cards.
- Real kdialog and zenity tests passed single/multiple/save paths, Unicode and
  newline filenames, extension insertion, overwrite refusal, user cancellation,
  and action cancellation. KWin X11 also passed the above hint and action
  cancellation after activating the panel through the window manager.
- Executed all four unchanged `selectFile` steps from the pinned OpenCC export
  on both backends. Each used its original `WriteTextFile` step to write the
  expected UTF-8 content. Only input variables received temporary native paths.
  This is evidence for the save portion, not the full Windows OpenCC workflow.
- Final checks: 151 native tests and 17 Python tests passed. All three corpus
  tests passed, including preservation of seven downloaded actions. Clippy
  passed with warnings denied. Wasm compiled with preview dead-code warnings.
  Added both file-dialog smoke commands to CI. Remote CI remains unverified.

## Step 19: List operations and original OpenCC list steps (2026-10-07)

- Inspected the MSI list runner, execution closure, text predicates, and common
  error helper. Added 31 operations on text lists and the comment no-op runner.
  Mutations update their input variable. Queries and sorting return new values.
  Preserved negative indexes, Skip/Take slices, case-sensitive equality,
  case-insensitive filters, and distinct filter exclusions.
- Added limits of 100,000 items and 16 MiB, regex cancellation checks, and errors
  for unsupported inputs. Preserved the MSI's second negative-index adjustment
  for `removeAt`. List failures stop execution. Fuzzy/pinyin filtering and the
  newer `stopIfFail=false` option remain blocked. Reports identify differences
  in culture sorting, Unicode rules, native metadata, and reference identity.
- Executed both unchanged OpenCC append steps and both unchanged comment steps
  with original variable declarations and a native path variable. Another
  author-source download matched the pinned hash. All four corpus tests passed,
  including preservation of seven real actions. The full OpenCC action remains
  blocked by other modules, expressions, and Windows paths.
- Repeated the ID download interface with dependency checks. The public Ref->Ob
  export downloaded and passed preservation. Its official dependency returned
  HTTP 401. The report retained that failure and returned a blocked result.
- Validation: 158 native tests passed, 11 opt-in tests ignored. All 17 Python
  tests passed. Clippy passed with warnings denied. Wasm compiled with 115
  preview dead-code warnings. No desktop code changed in this step.

## Step 20: Linux path expressions and C# strings (2026-10-07)

- Added nine pure Path methods with Linux separators and a 1 MiB path limit.
  Preserved null results, .NET dotfile extensions, dot segments, and absolute
  component replacement in Combine. Path methods reject Windows paths and NUL.
  Filesystem and environment access remain outside the pure evaluator.
- Added C# verbatim literals and corrected regular literals to use C# escapes.
  Preserved backslashes, doubled quotes, line breaks, and Unicode pairs.
  Unpaired surrogates, character literals, interpolation, and statement blocks
  remain unsupported. Literal concatenation never rewrites backslashes.
- Added expression diagnostics for Linux path rules and Windows literals.
  Evaluated all eight original OpenCC path expressions, including six unchanged
  assignment steps. Two original writes produced expected UTF-8 files. Two
  Windows path constructions retained their values and failed native writes.
- Generated 234 independent reference results with .NET 8.0.0 on Linux.
  Verified SDK 8.0.100 against its published SHA-512. Committed the reference
  dataset and C# generator. The Rust interpreter matched all results.
- A fresh author-source OpenCC download matched its pinned hash. All five
  corpus tests passed. Unsupported expression blocks fell from ten to two.
  Other OpenCC blockers remain. The complete action was not executed.
- Validation: 166 native tests passed, 12 opt-in tests ignored. All 17 Python
  tests passed. Clippy passed with warnings denied. Wasm compiled with 116
  preview dead-code warnings. The application has no new runtime dependency.

## Step 21: Native text windows (2026-10-07)

- Deferred complex C# execution at the user's request. Preserved its unfinished
  helper outside the worktree. The native application does not require .NET.
- Inspected the MSI showText dispatcher, UI closure, query, close, and wait-close
  methods. Added seven operations through native egui viewports. Action workers
  can wait without blocking the UI. Non-waiting windows outlive their actions.
- Added editing, selection, UTF-16 caret results, plain return buttons, line
  numbers, wrapping, colors, font size, centered dimensions, and topMost.
  Preserved keyed replacement, document update, exact append, and missing-window
  results. Cancellation closes a newly opened waiting window and stops execution.
- Bounded open windows and text sizes. Added explicit compatibility blockers for
  unsupported options and native handles. The editor toolbar differs from the
  Windows toolbar. Window enumeration, highlighting, custom fonts, advanced
  handlers, autosave, and other placement modes remain incomplete.
- The isolated X11/KWin test checks actual editing and window lifecycle. It runs
  three unchanged, hash-verified OpenCC steps. Both NO_WAIT windows remain open
  after action cancellation. Six downloaded-corpus tests pass. Full OpenCC still
  has other blockers. Native Wayland text windows are not yet verified.
- Validation: 172 native tests passed, 13 opt-in tests ignored. All 17 Python
  tests passed. Clippy passed with warnings denied. Wasm compiled with 117
  preview dead-code warnings. One earlier single-instance restart test failed
  transiently. Its isolated rerun and two subsequent full runs passed.

## Step 22: Waiting windows and monitored waits (2026-10-07)

- Deferred selection-window implementation at the user's request. Saved its
  unfinished files and tracked patch outside the worktree. It remains recoverable
  at `/tmp/quicker-deferred-selection-step22`. Complex C# execution stays deferred.
- Inspected the MSI showWaitWin dispatcher, creation/update closures, root context,
  progress parser, closed handler, automatic timer, delay, and clipboard waits.
  Added all six waiting-window modes. Root actions and subprograms share one window.
  Show updates an existing window. Update on a missing window does nothing.
- Added closure and button outputs, progress/countdown values, automatic closure,
  font size, and three activation modes. Set X11 input hints before mapping a
  mouse-only window. Buttons stay accessible with long scrolling prompts.
- Added programmatic closure, action cancellation, and root-action cleanup.
  Retained the MSI behavior where automatic timeout can stop the action through
  stopActionIfClose. Button returns bypass that stop flag. Added bounded resources
  and explicit errors for unsupported help and rich buttons.
- Delay and clipboard waits can monitor window closure. Delay exits successfully.
  Clipboard closure returns failure outputs and obeys stopIfFail. Delay monitoring
  also applies below one second, as the inspected MSI implements it. Invalid delay
  integers produce errors. Missing delayMs uses the MSI default of 100 ms.
- The isolated X11/KWin test passed all activation modes, outputs, long prompts,
  shared subprogram state, close/cancel/timeout behavior, and action-end cleanup.
  Monitored 30-second waits returned within three seconds after a button click.
  A position test exposed ignored initial placement and stale cached coordinates.
  Fixed both. The test now passes position restoration after a manual move.
- Executed the unchanged, hash-verified OpenCC update step with and without an open
  window. Controlled variables produced the expected 2/4 display. All seven corpus
  tests passed. The full OpenCC workflow remains blocked by other modules and paths.
- Validation: 180 native tests passed, 14 opt-in tests ignored. All 17 Python tests
  passed. Clippy passed with warnings denied. Wasm compiled with preview dead-code
  warnings. Added the wait-window smoke test and KWin package to CI. Remote CI
  remains unverified. Native Wayland, per-monitor placement, rich button syntax,
  Markdown help, and Windows taskbar progress remain gaps for this module.

## Step 23: Input text tools (2026-10-07)

- Deferred further automatic downloader work at the user's request. Continued
  compatibility checks with the existing local corpus. Selection windows and
  complex C# execution remain deferred.
- Traced the MSI configured tool provider, result callback, and input handler.
  Added single-file, multiple-file, folder, and save-path buttons to native input
  windows. These tools replace the complete value. Multiple files use CRLF.
  The legacy context-menu insertion behavior does not apply to these buttons.
- Kept picker work outside the UI thread. Picker cancellation retains the text.
  Input cancellation closes an active picker and obeys stopIfFail. Action
  cancellation stops both windows and cannot continue through stopIfFail=false.
  Existing required-value and regex validation also apply to these windows.
- Used local Qt/GTK folder dialogs so managed cancellation owns their windows.
  Added explicit limits on text size and open inputs. Unknown tools and custom
  settings remain errors. Fixed the checker so static extraSettings JSON retains
  its unsupported-option blocker instead of becoming a dynamic-value warning.
- Both isolated X11/KWin backend tests pass. They cover whole-value replacement,
  multiple paths, Unicode, whitespace, validation retry, and cancellation.
  Executed the unchanged, hash-verified OpenCC userInput step through both native
  folder pickers. Its original build-directory output matches the selected path.
  The full OpenCC report retains 10 unsupported runners, two Windows paths,
  and two complex expressions. Its text-tool option blocker is removed.
- Validation: 182 native tests and seven existing corpus tests passed. Clippy
  passed with warnings denied. Wasm compiled with 119 preview dead-code warnings.
  Existing Qt and GTK dialog smoke tests also passed after the folder changes.
  Added both input-tool GUI tests to CI. Remote CI and native Wayland remain
  unverified. Other text tools, custom replacement modes, forms, and advanced
  input window options still need implementation.

## Step 24: Native list editing (2026-10-07)

- Kept automatic downloader work, selection windows, and complex C# execution
  deferred. Used the existing OpenCC download for compatibility checks.
- Inspected the MSI list worker, result conversion, reset, delete, add/edit
  prompts, insertion, and sorting. Added `sys:manageList` for bound text lists.
  The native window edits a copy. Done writes the variable; cancellation and
  window closure preserve the original. Action cancellation always stops.
- Added add/edit/delete, Ctrl/Shift multiple selection, drag ordering, sorting,
  reset, and operation permissions. Preserved duplicates, Unicode, and spaces.
  Added limits and explicit errors for unsupported advanced options. The checker
  reports those options and differences in sorting and shared object identity.
- The X11 test exposed a missed fast Ctrl+A shortcut and incorrect test button
  coordinates. Fixed shortcut event handling and corrected the coordinates.
  Tests cover editing, ordering, reset, permissions, window closure during an
  edit, and action cancellation. Both original OpenCC list steps pass without
  changing their JSON. The full workflow retains eight unsupported runners,
  two Windows paths, and two complex expressions.
- Validation: 188 native tests and seven corpus tests passed. Clippy passed
  with warnings denied. Wasm compiled with 123 preview dead-code warnings.
  Added the list-window smoke test to CI. Remote CI and native Wayland remain
  unverified. Menu-data parsing, display expressions, Markdown help, custom
  add/edit subprograms, and shared list reference semantics remain incomplete.

## Step 25: Native multi-field forms (2026-10-07)

- Inspected the MSI form worker, window creation, submission, reset, dropdown
  parser, default selection, text conversion, and text-tool result handler.
  Added `sys:form` for variables, dictionaries, and dynamic dictionary definitions.
  Parse static JSON before evaluating field expressions. Validate all submitted
  values before writing fields. Cancellation preserves the original values.
- Added text and multiline fields, dropdowns, checkboxes, numeric text entry,
  passwords, read-only text, section separators, validation, reset, and native
  path tools. Added dimensions, topMost, focus restoration, and keyboard commands.
  Picker work and regex validation run outside the UI thread. Action cancellation
  closes the form and its active picker, even with `stopIfFail=false`.
- Real OpenCC forms exposed the required LF conversion for list-based choices.
  Fixed that conversion and tested quoted option text. GUI tests also exposed
  Alt shortcut text entering fields. Consumed the corresponding text events.
- Both original OpenCC forms execute without changing their JSON. The settings
  form retains controlled defaults. The conversion form validates required fields
  and accepts a Unicode file path. The complete action retains six unsupported
  runners, two Windows paths, and two complex expressions.
- Validation: 194 native tests and seven corpus tests passed. Clippy passed with
  warnings denied. Wasm compiled with 132 preview dead-code warnings. Added Qt
  and GTK form tests to CI. Remote CI and native Wayland remain unverified.
  Groups, dynamic field updates, expression validation, date and other advanced
  controls, custom buttons, Markdown help, and shared dictionary references
  remain incomplete. Automatic downloader work, selection windows, and complex
  C# execution remain deferred.

## Step 26: Native plugin scripts (2026-10-07)

- Inspected the MSI script worker, interpreter selection, process execution,
  and working-directory resolution. Added `sys:runScript` for `CUSTOM` with
  an explicit Linux interpreter and `PS` with PowerShell `pwsh` on PATH.
- Added temporary script files, file encodings, argument templates, script
  parameters, working directories, output capture, waiting, and detached
  execution. A detached process keeps its script until the direct child exits.
  Cancellation terminates the process group for a waiting or captured script.
- Output bindings force a wait. `stdout` falls back to stderr when stdout is
  empty. A nonzero exit code alone does not fail the step, as in the MSI.
  The checker reports Windows script types and unsupported options explicitly.
- Validation: 202 native tests and eight corpus tests passed. Clippy passed
  with warnings denied. Wasm compiled with 133 preview dead-code warnings.
  A separate test with official PowerShell 7.6.6 passed. It covers a Unicode
  argument, UTF-8 BOM, stderr, output capture, and a nonzero exit code.
- The original OpenCC action still has five Windows-script blockers, one
  unsupported selection runner, two Windows paths, and two complex expressions.
  This step does not make that full action executable. Console windows, console
  input, foreground file-manager directories, Windows encodings, file
  associations, and administrator execution remain incomplete. See the script
  section in `tests/compat/README.md` for exact platform differences.
- Pause after this step, as requested. The Linux port remains incomplete.

## Next implementation priorities (resume after the requested pause)

The user emphasized plugin support. Prioritize compatibility and executable
plugin workflows over additional launcher conveniences.

1. Implement remaining modules and options found in real downloads.
   Validate shared subprogram execution when an export or
   authenticated download is available. Use the reports for
   regression checks and add controlled execution tests.
2. Plugin runtime completeness, explicit unsupported-step diagnostics,
   advanced dialog options, desktop portal cancellation, and focused end-to-end tests
   of representative plugins.
3. Native Wayland activation/input/focus support with explicit capability
   reporting; global mouse activation and pointer placement on X11.
4. Per-action hotkeys, complete profile management/navigation and action
   organization (reorder, duplicate, move, undo deletion).
5. Verify release installation/uninstallation in a staging directory and native
   Wayland behavior. CI has been authored locally but has not run remotely.

The current pass does not establish completion of the Linux port.
