#!/usr/bin/env python3
"""Run under xvfb-run. Uses isolated config/runtime directories."""
import os
from pathlib import Path
import subprocess
import tempfile
import time
import tomllib

binary = Path(__file__).resolve().parents[1] / 'target/debug/quicker-rs'

def wait_for(predicate, label, timeout=8):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.05)
    raise AssertionError(f'timed out: {label}')

with tempfile.TemporaryDirectory(prefix='quicker-smoke-') as tmp:
    env = dict(os.environ, XDG_CONFIG_HOME=tmp + '/config', XDG_RUNTIME_DIR=tmp, XDG_SESSION_TYPE='x11', WINIT_UNIX_BACKEND='x11')
    env.pop('WAYLAND_DISPLAY', None)
    def run(*args):
        return subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, check=True, timeout=10)
    run('--check-config')
    config = Path(tmp) / 'config/quicker-rs/config.toml'
    assert config.is_file()
    def visible():
        found = subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', '^Quicker-RS$'], env=env, capture_output=True, text=True)
        return found.stdout.strip().splitlines()
    with open(tmp + '/app.log', 'w+') as log:
        app = subprocess.Popen([str(binary), '--hidden'], env=env, stdout=log, stderr=log)
        try:
            wait_for(lambda: list(Path(tmp).glob('quicker-rs-*/control.sock')), 'control socket')
            time.sleep(0.5)
            assert not visible(), 'hidden startup mapped the window'
            run('--show')
            wait_for(visible, 'show existing panel')
            assert len(visible()) == 1
            run('--show')
            time.sleep(0.2)
            assert len(visible()) == 1, 'second launch created another window'
            run('--toggle')
            wait_for(lambda: not visible(), 'toggle hides panel')
            run('--toggle')
            wait_for(visible, 'toggle shows panel')
            window = visible()[0]
            subprocess.run(['xdotool', 'windowfocus', '--sync', window], env=env, check=True)
            subprocess.run(['import', '-window', window, '/tmp/quicker-panel.png'], env=env, check=True, timeout=10)
            subprocess.run(['xdotool', 'mousemove', '--window', window, '482', '23', 'click', '1'], env=env, check=True)
            time.sleep(0.3)
            subprocess.run(['import', '-window', window, '/tmp/quicker-editor.png'], env=env, check=True, timeout=10)
            def input_at(x, y, text):
                subprocess.run(['xdotool', 'mousemove', '--window', window, str(x), str(y), 'click', '1', 'key', 'ctrl+a', 'type', '--clearmodifiers', text], env=env, check=True)
            def key(chord):
                subprocess.run(['xdotool', 'key', '--clearmodifiers', chord], env=env, check=True)
            def click(x, y):
                subprocess.run(['xdotool', 'mousemove', '--window', window, str(x), str(y), 'click', '1'], env=env, check=True)
                time.sleep(0.2)
            def saved_names():
                return [a['name'] for a in tomllib.loads(config.read_text())['profiles'][0]['actions']]
            # Coordinates correspond to the fixed default 600x500 viewport.
            input_at(100, 140, 'Native Smoke')
            input_at(100, 320, '/usr/bin/true')
            key('ctrl+s')
            wait_for(lambda: 'Native Smoke' in saved_names(), 'create basic action')
            input_at(120, 67, 'Native Smoke')
            time.sleep(0.2)
            key('Return')
            time.sleep(0.2)
            click(28, 189)
            input_at(100, 109, 'Native Smoke Edited')
            key('ctrl+s')
            wait_for(lambda: 'Native Smoke Edited' in saved_names(), 'edit basic action')
            click(91, 189)
            wait_for(lambda: 'Native Smoke Edited' not in saved_names(), 'delete basic action')
            run('--hide')
            wait_for(lambda: not visible(), 'hide panel')
            run('--quit')
            assert app.wait(timeout=8) == 0
            print('PASS: hidden startup, show, single instance, toggle, action create/edit/delete, hide, quit')
        except Exception:
            if visible():
                subprocess.run(['import', '-window', visible()[0], '/tmp/quicker-smoke-failure.png'], env=env, timeout=10)
            log.seek(0)
            print(log.read())
            raise
        finally:
            if app.poll() is None:
                app.terminate()
                app.wait(timeout=5)
