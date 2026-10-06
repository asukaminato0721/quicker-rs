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

On Linux, closing the window or pressing Escape at the panel root hides it.
Use **Quit** or `--quit` to exit. Escape during execution cancels the action.
The current right-drag radial menu operates inside the panel; global mouse
activation is still pending. Wayland keyboard injection and focus restoration
are also pending, so keyboard macros are not yet reliable for native Wayland
applications.

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
types 7, 11, and 24. Paste Quicker JSON in its import area. This is not universal
compatibility with Quicker's Windows action ecosystem: Windows executables,
COM/PowerShell integrations, remote templates, and unsupported step runners need
Linux equivalents. Review imported actions before running them: actions can
execute commands, access files, and make network requests.

Basic actions support JSON import/export. The Quicker builder also exports
JSON. Raw import/export preserves unknown document metadata; conversion through
the visual builder can still lose fields it cannot represent (tracked in
PORTING.md).

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
