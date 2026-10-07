#!/usr/bin/env python3
"""Exercise native showText windows in an isolated X11 and D-Bus session."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/quicker-rs'


def wait_for(predicate, label, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(.05)
    raise AssertionError('Timed out: ' + label)


def step(kind, inputs=None, outputs=None):
    return {'StepRunnerKey': 'sys:' + kind,
            'InputParams': {k: {'Value': v} for k, v in (inputs or {}).items()},
            'OutputParams': outputs or {}}


def save(key, var):
    result = step('stateStorage', {'type': 'saveActionState', 'key': key})
    result['InputParams']['value'] = {'VarKey': var}
    return result


with tempfile.TemporaryDirectory(prefix='quicker-text-windows-') as tmp:
    base = Path(tmp)
    env = dict(os.environ, XDG_CONFIG_HOME=tmp + '/config', XDG_RUNTIME_DIR=tmp,
               XDG_SESSION_TYPE='x11', WINIT_UNIX_BACKEND='x11', QT_QPA_PLATFORM='xcb',
               XDG_CURRENT_DESKTOP='X-Generic', LC_ALL='C.UTF-8')
    env.pop('WAYLAND_DISPLAY', None)

    def command(*args):
        return subprocess.run(args, env=env, capture_output=True, text=True,
                              check=True, timeout=10).stdout.strip()

    def windows(name):
        return subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', name],
                              env=env, capture_output=True, text=True).stdout.splitlines()

    def original_windows():
        # Xlib legacy title matching does not reliably match UTF-8 titles.
        return [w for w in windows('.*') if command('xdotool', 'getwindowname', w) == '结果内容']

    def focus(title):
        window = wait_for(lambda: windows('^' + title + '$'), title)[0]
        command('xdotool', 'windowactivate', '--sync', window)
        time.sleep(.4)
        return window

    config = base / 'config/quicker-rs/config.toml'
    config.parent.mkdir(parents=True)

    def state():
        path = config.with_name('action_state.json')
        return json.loads(path.read_text()).get('Text Smoke', {}) if path.exists() else {}

    outputs = {'resultText': 'result', 'selectedText': 'selected', 'caretPosition': 'caret',
               'selectedOperation': 'operation', 'isWindowExists': 'exists', 'isSuccess': 'ok'}
    workflow = [step('showText', {'type': 'WAIT', 'title': 'Text Wait', 'text': 'a😀中\r\n',
                                'operations': 'Accept|yes', 'showBuildInToolbar': '0',
                                'caretPosition': '-1', 'topMost': '1'}, outputs),
                save('edited', 'result'), save('selection', 'selected'), save('caret', 'caret'), save('button', 'operation'),
                step('showText', {'title': 'Text Live', 'text': 'start😀\r\n', 'autoCloseKey': 'live'}),
                step('showText', {'type': 'APPEND_TEXT', 'text': 'append\n', 'autoCloseKey': 'live'}),
                step('showText', {'type': 'GET_WIN_INFO', 'autoCloseKey': 'live'}, outputs),
                save('appended', 'result'), save('exists', 'exists'),
                step('showText', {'title': 'Text Live', 'text': 'updated 中文\n', 'autoCloseKey': 'live', 'updateIfExists': '1'}),
                step('showText', {'type': 'GET_WIN_INFO', 'autoCloseKey': 'live'}, outputs), save('updated', 'result'),
                step('showText', {'type': 'ACTIVATE_WINDOW', 'autoCloseKey': 'live'}),
                step('showText', {'type': 'WAIT_CLOSE', 'autoCloseKey': 'live'}),
                step('showText', {'type': 'GET_WIN_INFO', 'autoCloseKey': 'live'}, outputs), save('closed', 'exists'),
                step('showText', {'title': 'Text Program Close', 'text': 'close result', 'autoCloseKey': '='}),
                step('showText', {'type': 'CLOSE_WINDOW', 'autoCloseKey': '='}, outputs), save('programClose', 'result'),
                step('showText', {'type': 'APPEND_TEXT', 'autoCloseKey': 'missing', 'text': 'x', 'stopIfFail': '0'}, outputs),
                save('missingAppend', 'ok')]
    originals = []
    if corpus := os.environ.get('QUICKER_COMPAT_CORPUS'):
        source = (Path(corpus) / 'opencc.json').read_bytes()
        assert hashlib.sha256(source).hexdigest() == 'e511eca189b4db9fa323697c263e4686cabd590b918431caab15e6d4e1bcea5e'
        def collect(value):
            if isinstance(value, dict):
                if value.get('Disabled'):
                    return
                if value.get('StepRunnerKey') == 'sys:showText':
                    originals.append(value)
                for child in value.values():
                    collect(child)
            elif isinstance(value, list):
                for child in value:
                    collect(child)
        collect(json.loads(json.loads(source)['Data']))
        assert len(originals) == 3
        workflow += [step('assign', {'input': '繁體中文\r\nOriginal OpenCC output'}, {'output': 'Output'})]
        workflow += originals
        workflow += [step('stateStorage', {'type': 'saveActionState', 'key': 'originals', 'value': '3'})]
    workflow += [step('showText', {'type': 'WAIT', 'title': 'Text Action Cancel', 'text': 'cancel', 'stopIfFail': '0'}),
                 step('stateStorage', {'type': 'saveActionState', 'key': 'afterCancel', 'value': 'wrong'})]
    document = json.dumps({'ActionType': 24, 'Title': 'Text Smoke', 'Data': json.dumps({'Steps': workflow})})
    config.write_text('[[profiles]]\nname = "Global"\n[[profiles.actions]]\nname = "Text Smoke"\n'
                      '[profiles.actions.kind]\ntype = "PluginPipeline"\nquicker_json = ' + json.dumps(document) + '\n')
    with open(base / 'app.log', 'w+') as log:
        app = manager = None
        try:
            manager = subprocess.Popen(['kwin_x11', '--replace'], env=dict(env, KWIN_COMPOSE='N'), stdout=log, stderr=log)
            wait_for(lambda: 'window id #' in command('xprop', '-root', '_NET_SUPPORTING_WM_CHECK'), 'window manager')
            app = subprocess.Popen([str(BINARY)], env=env, stdout=log, stderr=log)
            panel = focus('Quicker-RS')
            command('xdotool', 'mousemove', '--window', panel, '65', '142', 'click', '1')
            window = focus('Text Wait')
            wait_for(lambda: '_NET_WM_STATE_ABOVE' in command('xprop', '-id', window, '_NET_WM_STATE'), 'topMost')
            command('xdotool', 'key', 'ctrl+End')
            command('xdotool', 'type', '--clearmodifiers', '--', 'edited')
            command('xdotool', 'key', 'shift+Home')
            time.sleep(.3)
            command('import', '-window', 'root', '/tmp/quicker-text-windows.png')
            command('xdotool', 'mousemove', '--window', window, '35', '15', 'click', '1')
            wait_for(lambda: 'button' in state(), 'WAIT outputs')
            assert state()['button'] == 'yes', state()
            assert state()['edited'] == 'a😀中\r\nedited', state()
            assert state()['selection'] == 'edited' and state()['caret'] == '6', state()
            window = focus('Text Live')
            wait_for(lambda: 'updated' in state(), 'NO_WAIT lifecycle')
            assert state()['appended'] == 'start😀\r\nappend\n' and state()['exists'] == '1', state()
            assert state()['updated'] == 'updated 中文\n', state()
            command('xdotool', 'key', 'Escape')
            wait_for(lambda: 'missingAppend' in state(), 'programmatic close and missing target')
            assert state()['closed'] == '0' and state()['programClose'] == 'close result', state()
            assert state()['missingAppend'] == '0', state()
            if originals:
                # The first original WAIT step has an empty title. Keep it unchanged.
                candidates = wait_for(lambda: [w for w in windows('.*') if w != panel
                                               and command('xdotool', 'getwindowname', w) == ''
                                               and command('xprop', '-id', w, '_NET_WM_PID').endswith(str(app.pid))], 'original WAIT')
                original = candidates[-1]
                command('xdotool', 'windowactivate', '--sync', original)
                time.sleep(.3)
                command('xdotool', 'key', 'Escape')
                wait_for(lambda: state().get('originals') == '3', 'three original OpenCC steps')
                wait_for(lambda: len(original_windows()) == 2, 'two original NO_WAIT windows')
            focus('Text Action Cancel')
            command(str(BINARY), '--show')
            focus('Quicker-RS')
            command('xdotool', 'key', 'Escape')
            wait_for(lambda: not windows('^Text Action Cancel$'), 'action cancellation closes waiting viewport')
            time.sleep(.3)
            assert 'afterCancel' not in state(), state()
            if originals:
                assert len(original_windows()) == 2, 'NO_WAIT documents must survive action termination'
            command(str(BINARY), '--quit')
            assert app.wait(timeout=10) == 0
            print('PASS: edited text, UTF-16 caret, selection, button, topMost, update, append, query, WAIT_CLOSE, programmatic close, failure, action cancel')
            if originals:
                print('PASS: 3 unchanged OpenCC showText steps and persistent NO_WAIT documents')
        except Exception:
            print('State:', state())
            print('Windows:', [(w, command('xdotool', 'getwindowname', w)) for w in windows('.*')])
            subprocess.run(['import', '-window', 'root', '/tmp/quicker-text-windows-failure.png'], env=env, timeout=10)
            log.seek(0)
            print(log.read())
            raise
        finally:
            if app is not None and app.poll() is None:
                subprocess.run([str(BINARY), '--quit'], env=env, timeout=10)
                try:
                    app.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    app.kill()
                    app.wait()
            if manager is not None:
                manager.terminate()
                try:
                    manager.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    manager.kill()
                    manager.wait()
