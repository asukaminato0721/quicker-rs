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

## Next implementation priorities (goal remains active)

The user emphasized plugin support. Prioritize compatibility and executable
plugin workflows over additional launcher conveniences.

1. Lossless visual editing of imported workflows: currently the builder can
   discard variables, metadata, flags, or step properties it cannot represent.
   Raw import/export is preserved, but that alone does not finish the editor.
2. Plugin runtime completeness, explicit unsupported-step diagnostics,
   cancellation of dialogs and every subprocess, reliable launch argument
   parsing, and focused end-to-end tests of representative plugins.
3. Native Wayland activation/input/focus support with explicit capability
   reporting; global mouse activation and pointer placement on X11.
4. Per-action hotkeys, complete profile management/navigation and action
   organization (reorder, duplicate, move, undo deletion).
5. Verify release installation/uninstallation in a staging directory and native
   Wayland behavior. CI has been authored locally but has not run remotely.

The current pass does not establish completion of the Linux port.
