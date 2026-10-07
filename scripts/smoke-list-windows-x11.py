#!/usr/bin/env python3
"""Verify list transactions and editing in an isolated X11/KWin session."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/quicker-rs'


def wait_for(predicate, label, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = predicate()
        if result:
            return result
        time.sleep(.05)
    raise AssertionError('Timed out: ' + label)


def step(kind, inputs=None, outputs=None):
    return {'StepRunnerKey': 'sys:' + kind,
            'InputParams': {k: {'Value': v} for k, v in (inputs or {}).items()},
            'OutputParams': outputs or {}}


def assign(name, value):
    return step('assign', {'input': value}, {'output': name})


def save(key, variable):
    result = step('stateStorage', {'type': 'saveActionState', 'key': key})
    result['InputParams']['value'] = {'VarKey': variable}
    return result


def manage(title, **inputs):
    result = step('manageList', dict(winTitle=title, **inputs), {'isSuccess': 'ok'})
    result['InputParams']['list'] = {'VarKey': 'items'}
    return result


with tempfile.TemporaryDirectory(prefix='quicker-list-windows-') as tmp:
    base = Path(tmp)
    env = dict(os.environ, XDG_CONFIG_HOME=tmp + '/config', XDG_RUNTIME_DIR=tmp,
               XDG_SESSION_TYPE='x11', WINIT_UNIX_BACKEND='x11', QT_QPA_PLATFORM='xcb',
               XDG_CURRENT_DESKTOP='X-Generic', LC_ALL='C.UTF-8')
    env.pop('WAYLAND_DISPLAY', None)

    def command(*args):
        return subprocess.run(args, env=env, capture_output=True, text=True,
                              check=True, timeout=10).stdout.strip()

    def windows(name):
        ids = subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', '.*'],
                             env=env, capture_output=True, text=True).stdout.splitlines()
        result = []
        for window in ids:
            title = subprocess.run(['xdotool', 'getwindowname', window], env=env,
                                   capture_output=True, text=True, timeout=5)
            if title.returncode == 0 and title.stdout.strip() == name:
                result.append(window)
        return result

    def focus(title):
        window = wait_for(lambda: windows(title), title)[0]
        command('xdotool', 'windowactivate', '--sync', window)
        time.sleep(.25)
        return window

    def click(window, x, y):
        command('xdotool', 'mousemove', '--window', window, str(x), str(y), 'click', '1')
        time.sleep(.12)

    def row(window, index):
        click(window, 75, 51 + 30 * index)

    def done(window):
        geometry = dict(line.split('=', 1) for line in command('xdotool', 'getwindowgeometry', '--shell', window).splitlines())
        click(window, 26, int(geometry['HEIGHT']) - 12)

    def edit_value(window, value, add=False):
        click(window, 30 if add else 82, 16)
        command('xdotool', 'key', 'ctrl+a')
        command('xdotool', 'type', '--clearmodifiers', '--', value)
        command('xdotool', 'key', 'Return')
        time.sleep(.2)

    config = base / 'config/quicker-rs/config.toml'
    config.parent.mkdir(parents=True)

    def state():
        path = config.with_name('action_state.json')
        return json.loads(path.read_text()).get('List Smoke', {}) if path.exists() else {}

    def configure(steps):
        document = json.dumps({'ActionType': 24, 'Title': 'List Smoke', 'Data': json.dumps({'Steps': steps})})
        config.write_text('[[profiles]]\nname = "Global"\n[[profiles.actions]]\nname = "List Smoke"\n'
                          '[profiles.actions.kind]\ntype = "PluginPipeline"\nquicker_json = ' + json.dumps(document) + '\n')

    originals = []
    if corpus := os.environ.get('QUICKER_COMPAT_CORPUS'):
        source = (Path(corpus) / 'opencc.json').read_bytes()
        assert hashlib.sha256(source).hexdigest() == 'e511eca189b4db9fa323697c263e4686cabd590b918431caab15e6d4e1bcea5e'
        data = json.loads(json.loads(source)['Data'])
        def collect(value):
            if isinstance(value, dict):
                if value.get('Disabled'):
                    return
                if value.get('StepRunnerKey') == 'sys:manageList':
                    originals.append(value)
                for child in value.values():
                    collect(child)
            elif isinstance(value, list):
                for child in value:
                    collect(child)
        collect(data['Steps'])
        assert len(originals) == 2
    initial = ['z', 'dup', 'a', 'dup', '末尾  ']
    workflow = [assign('items', initial), manage('List Edit'), save('edited', 'items'), save('editOK', 'ok'),
                manage('List Cancel'), save('cancelled', 'items'), save('cancelOK', 'ok'),
                manage('List Restricted', allowAdd='0', allowEdit='0', allowDelete='0'), save('restricted', 'items'),
                assign('items', initial), manage('List Drag'), save('dragged', 'items'),
                assign('items', ['first', 'second']), manage('List Multi'), save('multiple', 'items'),
                assign('items', ['b', 'A', 'a']), manage('List Sort'), save('sorted', 'items'),
                manage('List Reverse Sort'), save('reverseSorted', 'items'),
                manage('List Reset'), save('reset', 'items'),
                manage('List Close'), save('closed', 'items'), save('closeOK', 'ok')]
    for index, original in enumerate(originals):
        workflow += [assign('options', ['s2t(繁體中文)|0', 's2hk(香港繁體)|0']), original, save('original' + str(index), 'options')]
    workflow += [manage('List Action Cancel', stopIfFail='0'),
                 step('stateStorage', {'type': 'saveActionState', 'key': 'afterCancel', 'value': 'wrong'})]
    configure(workflow)
    with open(base / 'app.log', 'w+') as log:
        app = manager = None
        try:
            manager = subprocess.Popen(['kwin_x11', '--replace'], env=dict(env, KWIN_COMPOSE='N'), stdout=log, stderr=log)
            wait_for(lambda: 'window id #' in command('xprop', '-root', '_NET_SUPPORTING_WM_CHECK'), 'window manager')
            app = subprocess.Popen([str(BINARY)], env=env, stdout=log, stderr=log)
            panel = focus('Quicker-RS')
            click(panel, 65, 142)
            window = focus('List Edit')
            row(window, 1)
            edit_value(window, 'inserted  ', add=True)
            edit_value(window, 'changed  ')
            # Remove both duplicate rows with Ctrl selection.
            row(window, 1)
            command('xdotool', 'keydown', 'ctrl')
            row(window, 4)
            command('xdotool', 'keyup', 'ctrl')
            command('xdotool', 'key', 'Delete')
            done(window)
            wait_for(lambda: 'editOK' in state(), 'edited list')
            assert state()['edited'] == 'z,changed  ,a,末尾  ' and state()['editOK'] == '1', state()
            window = focus('List Cancel')
            row(window, 0)
            command('xdotool', 'key', 'ctrl+a', 'Delete')
            command('xdotool', 'key', 'Escape')
            wait_for(lambda: 'cancelOK' in state(), 'cancelled list')
            assert state()['cancelled'] == state()['edited'] and state()['cancelOK'] == '0', state()
            window = focus('List Restricted')
            row(window, 0)
            for x in (30, 82, 140):
                click(window, x, 16)
            command('xdotool', 'key', 'Delete')
            done(window)
            wait_for(lambda: 'restricted' in state(), 'restricted list')
            assert state()['restricted'] == state()['edited'], state()
            window = focus('List Drag')
            row(window, 0)
            command('xdotool', 'mousemove', '--window', window, '75', '50', 'mousedown', '1')
            command('xdotool', 'mousemove', '--window', window, '95', '78')
            time.sleep(.2)
            command('xdotool', 'mousemove', '--window', window, '95', '180')
            time.sleep(.2)
            command('xdotool', 'mouseup', '1')
            time.sleep(.2)
            done(window)
            wait_for(lambda: 'dragged' in state(), 'dragged list')
            assert state()['dragged'] == 'dup,a,dup,末尾  ,z', state()
            window = focus('List Multi')
            row(window, 0)
            command('xdotool', 'keydown', 'shift')
            row(window, 1)
            command('xdotool', 'keyup', 'shift')
            command('xdotool', 'key', 'Delete')
            edit_value(window, 'only', add=True)
            done(window)
            wait_for(lambda: 'multiple' in state(), 'shift selection')
            assert state()['multiple'] == 'only', state()
            window = focus('List Sort')
            click(window, 197, 16)
            done(window)
            wait_for(lambda: 'sorted' in state(), 'ascending sort')
            assert state()['sorted'] == 'A,a,b', state()
            window = focus('List Reverse Sort')
            click(window, 243, 16)
            done(window)
            wait_for(lambda: 'reverseSorted' in state(), 'descending sort')
            assert state()['reverseSorted'] == 'b,a,A', state()
            window = focus('List Reset')
            command('xdotool', 'key', 'ctrl+a', 'Delete')
            click(window, 298, 16)
            done(window)
            wait_for(lambda: 'reset' in state(), 'reset')
            assert state()['reset'] == 'b,a,A', state()
            window = focus('List Close')
            click(window, 30, 16)
            command('xdotool', 'type', '--', 'discard this edit')
            geometry = dict(line.split('=', 1) for line in command('xdotool', 'getwindowgeometry', '--shell', window).splitlines())
            click(window, int(geometry['WIDTH']) - 14, -14)
            wait_for(lambda: 'closeOK' in state(), 'closed window')
            assert state()['closed'] == 'b,a,A' and state()['closeOK'] == '0', state()
            for index, _ in enumerate(originals):
                window = focus('預設配置列表')
                subprocess.run(['import', '-window', 'root', '/tmp/quicker-list-windows.png'], env=env, check=True, timeout=10)
                command('xdotool', 'key', 'ctrl+a', 'Delete')
                edit_value(window, 'my_s2hk(custom)|0', add=True)
                if index == 0:
                    done(window)
                else:
                    command('xdotool', 'key', 'Escape')
                wait_for(lambda: 'original' + str(index) in state(), 'original OpenCC ' + str(index))
                expected = 'my_s2hk(custom)|0' if index == 0 else 's2t(繁體中文)|0,s2hk(香港繁體)|0'
                assert state()['original' + str(index)] == expected, state()
            window = focus('List Action Cancel')
            click(window, 20, 16)
            command('xdotool', 'type', '--', 'must not save')
            command(str(BINARY), '--show')
            focus('Quicker-RS')
            command('xdotool', 'key', 'Escape')
            wait_for(lambda: not windows('List Action Cancel'), 'cancelled action')
            time.sleep(.3)
            assert 'afterCancel' not in state(), state()
            command(str(BINARY), '--quit')
            assert app.wait(timeout=10) == 0
            print('PASS: add, edit, duplicates, Ctrl/Shift selection, drag, sort, reset, permissions, transaction cancel, window close, action cancel')
            if originals:
                print('PASS: two unchanged hash-verified OpenCC manageList steps')
        except Exception:
            print('State:', state())
            subprocess.run(['import', '-window', 'root', '/tmp/quicker-list-windows-failure.png'], env=env, timeout=10)
            log.seek(0)
            print(log.read())
            raise
        finally:
            for process in (app, manager):
                if process and process.poll() is None:
                    process.terminate()
                    process.wait(timeout=5)
