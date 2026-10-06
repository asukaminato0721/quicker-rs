#!/usr/bin/env python3
"""Verify input text tools with real Qt/GTK pickers in an isolated X11 session."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/quicker-rs'
BACKEND = sys.argv[1] if len(sys.argv) > 1 else 'kdialog'
assert BACKEND in ('kdialog', 'zenity')


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


def save(key, var):
    result = step('stateStorage', {'type': 'saveActionState', 'key': key})
    result['InputParams']['value'] = {'VarKey': var}
    return result


with tempfile.TemporaryDirectory(prefix='quicker-input-tools-') as tmp:
    base = Path(tmp)
    env = dict(os.environ, XDG_CONFIG_HOME=tmp + '/config', XDG_RUNTIME_DIR=tmp,
               XDG_SESSION_TYPE='x11', WINIT_UNIX_BACKEND='x11', QT_QPA_PLATFORM='xcb',
               QT_QPA_PLATFORMTHEME='generic', XDG_CURRENT_DESKTOP='X-Generic',
               GTK_USE_PORTAL='0', GDK_DEBUG='no-portals', LC_ALL='C.UTF-8')
    env.pop('WAYLAND_DISPLAY', None)
    tools = base / 'bin'
    tools.mkdir()
    for name in (BACKEND, 'xdotool'):
        (tools / name).symlink_to(shutil.which(name))
    app_env = dict(env, PATH=str(tools))
    config = base / 'config/quicker-rs/config.toml'
    config.parent.mkdir(parents=True)

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

    def state():
        path = config.with_name('action_state.json')
        return json.loads(path.read_text()).get('Input Tools', {}) if path.exists() else {}

    def submit():
        window = focus('Quicker input')
        geometry = dict(line.split('=', 1) for line in command('xdotool', 'getwindowgeometry', '--shell', window).splitlines())
        command('xdotool', 'mousemove', '--window', window, '20', str(int(geometry['HEIGHT']) - 12), 'click', '1')

    def choose(title, path=None, multiline=False):
        window = focus('Quicker input')
        command('xdotool', 'mousemove', '--window', window, '45', '166' if multiline else '59', 'click', '1')
        picker = focus(title)
        if path is None:
            command('xdotool', 'key', 'Escape')
        else:
            command('xdotool', 'key', 'ctrl+l')
            # GTK does not accept all Unicode key events from xdotool.
            # The preceding workflow step puts the exact path on the clipboard.
            command('xdotool', 'key', '--clearmodifiers', 'ctrl+v')
            command('xdotool', 'key', 'Return')
            time.sleep(.5)
            if picker in windows(title):
                command('xdotool', 'key', 'Return')
        wait_for(lambda: not windows(title), 'picker closed')
        time.sleep(.25)

    directory = base / 'folder 中文  '
    directory.mkdir()
    file = base / 'file 中文 %|.txt'
    file.write_text('keep')
    destination = base / 'new 中文.txt'
    multi = base / 'multiple'
    multi.mkdir()
    files = [multi / 'one 中文|%.txt', multi / 'two.txt']
    for path in files:
        path.write_text('keep')
    originals = []
    if corpus := os.environ.get('QUICKER_COMPAT_CORPUS'):
        source = (Path(corpus) / 'opencc.json').read_bytes()
        assert hashlib.sha256(source).hexdigest() == 'e511eca189b4db9fa323697c263e4686cabd590b918431caab15e6d4e1bcea5e'
        data = json.loads(json.loads(source)['Data'])
        original = data['Steps'][0]['IfSteps'][0]['IfSteps'][0]
        assert original['StepRunnerKey'] == 'sys:userInput'
        assert original['InputParams']['texttools']['Value'] == 'SelectSingleFolder'
        originals = [original, save('original', 'build目录')]
    output = {'textValue': 'text', 'isSuccess': 'ok', 'isEmpty': 'empty'}
    workflow = [step('writeClipboard', {'text': str(directory)}),
                step('userInput', {'texttools': 'SelectSingleFolder', 'defaultValue': 'replace me'}, output), save('folder', 'text'),
                step('writeClipboard', {'text': str(file)}),
                step('userInput', {'texttools': 'SelectSingleFile', 'defaultValue': 'prefix OLD suffix'}, output), save('file', 'text'),
                step('writeClipboard', {'text': str(destination)}),
                step('userInput', {'texttools': 'SelectSavePath'}, output), save('save', 'text'),
                step('userInput', {'texttools': 'SelectSingleFolder', 'defaultValue': 'keep 中文  '}, output), save('pickerCancel', 'text'),
                step('userInput', {'type': 'multiline', 'texttools': 'SelectSingleFile', 'defaultValue': 'first\nlast  \n\n'}, output), save('multiline', 'text'),
                step('userInput', {'texttools': 'SelectSingleFolder', 'isRequired': '1', 'pattern': '^yes$'}, output), save('validated', 'text'),
                step('userInput', {'texttools': 'SelectSingleFolder', 'stopIfFail': '0'}, output), save('cancelText', 'text'), save('cancelOK', 'ok'), save('cancelEmpty', 'empty')]
    workflow += [step('userInput', {'type': 'multiline', 'texttools': 'SelectMultiFile', 'defaultValue': str(files[0])}, output), save('multiple', 'text')]
    workflow += [step('writeClipboard', {'text': str(directory)})] + originals
    workflow += [step('userInput', {'texttools': 'SelectSingleFolder', 'stopIfFail': '0'}, output),
                 step('stateStorage', {'type': 'saveActionState', 'key': 'afterCancel', 'value': 'wrong'})]

    def configure(steps):
        document = json.dumps({'ActionType': 24, 'Title': 'Input Tools', 'Data': json.dumps({'Steps': steps})})
        config.write_text('[[profiles]]\nname = "Global"\n[[profiles.actions]]\nname = "Input Tools"\n'
                          '[profiles.actions.kind]\ntype = "PluginPipeline"\nquicker_json = ' + json.dumps(document) + '\n')

    configure(workflow)
    with open(base / 'app.log', 'w+') as log:
        app = manager = None
        try:
            manager = subprocess.Popen(['kwin_x11', '--replace'], env=dict(env, KWIN_COMPOSE='N'), stdout=log, stderr=log)
            wait_for(lambda: 'window id #' in command('xprop', '-root', '_NET_SUPPORTING_WM_CHECK'), 'window manager')
            app = subprocess.Popen([str(BINARY)], env=app_env, stdout=log, stderr=log)
            panel = focus('Quicker-RS')
            command('xdotool', 'mousemove', '--window', panel, '65', '142', 'click', '1')
            choose('Select folder', directory)
            submit()
            wait_for(lambda: 'folder' in state(), 'folder result')
            assert state()['folder'] == str(directory), state()
            window = focus('Quicker input')
            command('xdotool', 'key', 'Home', 'Right', 'Right', 'Right', 'Right', 'Right', 'Right', 'Right', 'shift+Right', 'shift+Right', 'shift+Right')
            choose('Select file', file)
            submit()
            wait_for(lambda: 'file' in state(), 'file result')
            assert state()['file'] == str(file), state()
            choose('Save path', destination)
            submit()
            wait_for(lambda: 'save' in state(), 'save result')
            assert state()['save'] == str(destination) and not destination.exists(), state()
            choose('Select folder')
            submit()
            wait_for(lambda: 'pickerCancel' in state(), 'picker cancel result')
            assert state()['pickerCancel'] == 'keep 中文  ', state()
            submit()
            wait_for(lambda: 'multiline' in state(), 'multiline result')
            assert state()['multiline'] == 'first\nlast  \n\n', state()
            submit()
            focus('Input')
            command('xdotool', 'key', 'Return')
            focus('Quicker input')
            command('xdotool', 'type', '--', 'yes')
            submit()
            wait_for(lambda: 'validated' in state(), 'validation retry')
            assert state()['validated'] == 'yes', state()
            focus('Quicker input')
            command('xdotool', 'key', 'Escape')
            wait_for(lambda: 'cancelEmpty' in state(), 'input cancelled')
            assert state()['cancelText'] == '' and state()['cancelOK'] == '0' and state()['cancelEmpty'] == '1', state()
            window = focus('Quicker input')
            command('xdotool', 'mousemove', '--window', window, '45', '172', 'click', '1')
            picker = focus('Select files')
            command('xdotool', 'mousemove', '--window', picker, '280', '145' if BACKEND == 'zenity' else '115', 'click', '1')
            command('xdotool', 'key', '--clearmodifiers', 'ctrl+a')
            if BACKEND == 'kdialog':
                geometry = dict(line.split('=', 1) for line in command('xdotool', 'getwindowgeometry', '--shell', picker).splitlines())
                command('xdotool', 'mousemove', '--window', picker, str(int(geometry['WIDTH']) - 50), str(int(geometry['HEIGHT']) - 55), 'click', '1')
            else:
                command('xdotool', 'key', 'Return')
            wait_for(lambda: not windows('Select files'), 'multiple picker closed')
            time.sleep(.3)
            submit()
            wait_for(lambda: 'multiple' in state(), 'multiple result')
            assert set(state()['multiple'].split('\r\n')) == set(map(str, files)), state()
            if originals:
                focus('Quicker input')
                subprocess.run(['import', '-window', 'root', '/tmp/quicker-input-tools.png'], env=env, check=True, timeout=10)
                choose('Select folder', directory)
                submit()
                wait_for(lambda: 'original' in state(), 'unchanged OpenCC input')
                assert state()['original'] == str(directory), state()
            window = focus('Quicker input')
            command('xdotool', 'mousemove', '--window', window, '45', '59', 'click', '1')
            focus('Select folder')
            command(str(BINARY), '--show')
            focus('Quicker-RS')
            command('xdotool', 'key', 'Escape')
            wait_for(lambda: not windows('Select folder') and not windows('Quicker input'), 'action cancelled')
            time.sleep(.3)
            assert 'afterCancel' not in state(), state()
            command(str(BINARY), '--quit')
            assert app.wait(timeout=10) == 0
            # Closing the input during a picker cancels only that dialog.
            # stopIfFail=false must still let the action continue.
            configure([step('userInput', {'texttools': 'SelectSingleFolder', 'stopIfFail': '0'}, output), save('closedWhilePicking', 'ok')])
            app = subprocess.Popen([str(BINARY)], env=app_env, stdout=log, stderr=log)
            panel = focus('Quicker-RS')
            command('xdotool', 'mousemove', '--window', panel, '65', '142', 'click', '1')
            window = focus('Quicker input')
            command('xdotool', 'mousemove', '--window', window, '45', '59', 'click', '1')
            focus('Select folder')
            focus('Quicker input')
            command('xdotool', 'key', 'Escape')
            wait_for(lambda: not windows('Select folder') and not windows('Quicker input'), 'both dialogs closed')
            wait_for(lambda: 'closedWhilePicking' in state(), 'action continued after input close')
            assert state()['closedWhilePicking'] == '0', state()
            command(str(BINARY), '--quit')
            assert app.wait(timeout=10) == 0
            print('PASS:', BACKEND, 'full text replacement, multiple paths with CRLF, save without write, picker/input/action cancellation, validation, multiline')
            if originals:
                print('PASS: unchanged hash-verified OpenCC userInput step')
        except Exception:
            print('State:', state())
            subprocess.run(['import', '-window', 'root', '/tmp/quicker-input-tools-failure.png'], env=env, timeout=10)
            log.seek(0)
            print(log.read())
            raise
        finally:
            for process in (app, manager):
                if process and process.poll() is None:
                    process.terminate()
                    process.wait(timeout=5)
