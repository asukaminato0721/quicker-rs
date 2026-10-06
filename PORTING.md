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

## Next implementation priorities (goal remains active)

The user emphasized plugin support. Prioritize compatibility and executable
plugin workflows over additional launcher conveniences.

1. Implement missing modules found in real downloads, including selected files.
   Validate shared subprogram execution when an export or
   authenticated download is available. Use the reports for
   regression checks and add controlled execution tests.
2. Plugin runtime completeness, explicit unsupported-step diagnostics,
   cancellation of dialogs and waited subprocesses, and focused end-to-end tests
   of representative plugins.
3. Native Wayland activation/input/focus support with explicit capability
   reporting; global mouse activation and pointer placement on X11.
4. Per-action hotkeys, complete profile management/navigation and action
   organization (reorder, duplicate, move, undo deletion).
5. Verify release installation/uninstallation in a staging directory and native
   Wayland behavior. CI has been authored locally but has not run remotely.

The current pass does not establish completion of the Linux port.
