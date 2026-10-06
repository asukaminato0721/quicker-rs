#!/usr/bin/env python3
"""Run under xvfb-run with an exported plugin path; edits never execute it."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import tomllib

binary = Path(__file__).resolve().parents[1] / 'target/debug/quicker-rs'
original = json.loads(Path(sys.argv[1]).read_text(encoding='utf-8-sig'))

def wait_for(predicate, label):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        result = predicate()
        if result:
            return result
        time.sleep(0.05)
    raise AssertionError(f'timed out: {label}')

with tempfile.TemporaryDirectory(prefix='quicker-plugin-editor-') as tmp:
    env = dict(os.environ, XDG_CONFIG_HOME=tmp + '/config', XDG_RUNTIME_DIR=tmp,
               XDG_SESSION_TYPE='x11', WINIT_UNIX_BACKEND='x11')
    env.pop('WAYLAND_DISPLAY', None)
    def command(*args):
        return subprocess.run(args, env=env, capture_output=True, text=True,
                              check=True, timeout=10).stdout.strip()
    def windows():
        return subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', '^Quicker-RS$'],
                              env=env, capture_output=True, text=True).stdout.splitlines()
    config = Path(tmp) / 'config/quicker-rs/config.toml'
    config.parent.mkdir(parents=True)
    config.write_text('[[profiles]]\nname = "Global"\n[[profiles.actions]]\n'
                      'name = "Imported Plugin"\ntags = ["keep-tag"]\nhotkey = "Ctrl+Alt+F9"\n'
                      '[profiles.actions.kind]\ntype = "PluginPipeline"\nquicker_json = '
                      + json.dumps(json.dumps(original, ensure_ascii=False)) + '\n')
    before = config.stat().st_mtime_ns
    with open(tmp + '/app.log', 'w+') as log:
        app = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
        try:
            window = wait_for(windows, 'panel')[0]
            time.sleep(0.4)
            command('xdotool', 'windowfocus', '--sync', window)
            command('xdotool', 'mousemove', '--window', window, '28', '189', 'click', '1')
            time.sleep(0.5)
            command('import', '-window', window, '/tmp/quicker-plugin-editor.png')
            command('xdotool', 'key', '--clearmodifiers', 'ctrl+s')
            wait_for(lambda: config.stat().st_mtime_ns != before, 'plugin save')
            saved = tomllib.loads(config.read_text())['profiles'][0]['actions'][0]
            assert json.loads(saved['kind']['quicker_json']) == original
            assert saved['tags'] == ['keep-tag']
            assert saved['hotkey'] == 'Ctrl+Alt+F9'
            command(str(binary), '--quit')
            assert app.wait(timeout=8) == 0
            print('PASS: imported plugin editor save preserves document, tags, hotkey')
        except Exception:
            log.seek(0)
            print(log.read())
            raise
        finally:
            if app.poll() is None:
                app.terminate()
                app.wait(timeout=5)
