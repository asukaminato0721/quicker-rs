#!/usr/bin/env python3
"""Run under Xvfb and D-Bus. Exercise real workflow dialogs with both backends."""
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

def wait_for(predicate, label, timeout=12):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(.05)
    raise AssertionError('Timed out: ' + label)

def step(runner, inputs=None, outputs=None):
    return {'StepRunnerKey': 'sys:' + runner,
            'InputParams': {k: {'Value': v} for k, v in (inputs or {}).items()},
            'OutputParams': outputs or {}}

def save(key, var):
    value = step('stateStorage', {'type': 'saveActionState', 'key': key})
    value['InputParams']['value'] = {'VarKey': var}
    return value

with tempfile.TemporaryDirectory(prefix='quicker-dialogs-') as tmp:
    env = dict(os.environ, XDG_CONFIG_HOME=tmp + '/config', XDG_RUNTIME_DIR=tmp,
               XDG_SESSION_TYPE='x11', WINIT_UNIX_BACKEND='x11', QT_QPA_PLATFORM='xcb',
               QT_QPA_PLATFORMTHEME='generic', XDG_CURRENT_DESKTOP='X-Generic', GTK_USE_PORTAL='0', GDK_DEBUG='no-portals', LC_ALL='C.UTF-8')
    env.pop('WAYLAND_DISPLAY', None)
    # Select one backend without changing the user's installation or environment.
    tools = Path(tmp) / 'bin'
    tools.mkdir()
    for name in (BACKEND, 'xdotool'):
        (tools / name).symlink_to(shutil.which(name))
    app_env = dict(env, PATH=str(tools), ZENITY_OK='47', ZENITY_CANCEL='0', ZENITY_EXTRA='0')
    def command(*args):
        return subprocess.run(args, env=env, capture_output=True, text=True,
                              check=True, timeout=10).stdout.strip()
    def windows(name):
        return subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', name],
                              env=env, capture_output=True, text=True).stdout.splitlines()
    def dialogs():
        found = subprocess.run(['xdotool', 'search', '--onlyvisible', '--class', '(zenity|xdg-desktop-portal.*)' if BACKEND == 'zenity' else BACKEND],
                               env=env, capture_output=True, text=True).stdout.splitlines()
        result = []
        for window in found:
            geometry = subprocess.run(['xdotool', 'getwindowgeometry', '--shell', window],
                                      env=env, capture_output=True, text=True)
            if geometry.returncode == 0:
                values = dict(line.split('=', 1) for line in geometry.stdout.splitlines())
                if int(values['WIDTH']) > 100 and int(values['HEIGHT']) > 70:
                    result.append(window)
        return result
    def respond(key):
        window = wait_for(dialogs, 'dialog')[0]
        command('xdotool', 'windowfocus', '--sync', window)
        time.sleep(.5)
        if key == 'clickOK':
            geometry = dict(line.split('=', 1) for line in command('xdotool', 'getwindowgeometry', '--shell', window).splitlines())
            command('xdotool', 'mousemove', '--window', window, str(int(geometry['WIDTH']) - 45), str(int(geometry['HEIGHT']) - (50 if BACKEND == 'zenity' else 24)), 'click', '1')
        else:
            command('xdotool', 'key', '--clearmodifiers', key)
    def state():
        path = config.with_name('action_state.json')
        return json.loads(path.read_text()).get('Dialog Smoke', {}) if path.exists() else {}

    folder = Path(tmp) / 'folder 中文  '
    folder.mkdir()
    workflow = []
    choices = [('OK', 'Return', 'OK'), ('OKCancel', 'Escape', 'Cancel'),
               ('YesNo', 'Escape', 'No'), ('YesNo', 'Return', 'Yes'),
               ('YesNoCancel', 'Escape', 'Cancel'), ('YesNoCancel', 'alt+n', 'No'),
               ('YesNoCancel', 'Return', 'Yes')]
    for index, (buttons, _, _) in enumerate(choices):
        workflow += [step('MsgBox', {'title': 'Dialog Smoke', 'message': '<plain> & 中文',
                                    'buttons': buttons}, {'result': 'result', 'okOrYes': 'ok'}),
                     save('choice' + str(index), 'result'), save('confirmed' + str(index), 'ok')]
    workflow += [step('userInput', {'defaultValue': '--literal 中文  '}, {'textValue': 'text'}), save('text', 'text'),
                 step('userInput', {'type': 'multiline', 'defaultValue': 'first\n中文  \n\n'}, {'textValue': 'text'}), save('multiline', 'text'),
                 step('selectFolder', {'prompt': 'Folder Smoke', 'initDir': str(folder)}, {'path': 'path'}), save('folder', 'path'),
                 step('userInput', {'stopIfFail': '0'}, {'textValue': 'text', 'isSuccess': 'ok'}), save('cancelText', 'text'), save('cancelOK', 'ok'),
                 step('selectFolder', {'stopIfFail': '0'}, {'path': 'path', 'isSuccess': 'ok'}), save('cancelFolder', 'path'),
                 step('MsgBox', {'title': 'Cancel Action', 'message': 'Stop this workflow.'}),
                 step('stateStorage', {'type': 'saveActionState', 'key': 'afterCancel', 'value': 'wrong'})]
    document = json.dumps({'ActionType': 24, 'Title': 'Dialog Smoke', 'Data': json.dumps({'Steps': workflow})})
    config = Path(tmp) / 'config/quicker-rs/config.toml'
    config.parent.mkdir(parents=True)
    config.write_text('[[profiles]]\nname = "Global"\n[[profiles.actions]]\nname = "Dialog Smoke"\n'
                      '[profiles.actions.kind]\ntype = "PluginPipeline"\nquicker_json = ' + json.dumps(document) + '\n')
    with open(tmp + '/app.log', 'w+') as log:
        app = subprocess.Popen([str(BINARY)], env=app_env, stdout=log, stderr=log)
        try:
            panel = wait_for(lambda: windows('^Quicker-RS$'), 'panel')[0]
            time.sleep(.5)
            command('xdotool', 'windowfocus', '--sync', panel)
            command('xdotool', 'mousemove', '--window', panel, '65', '142', 'click', '1')
            for index, (_, key, expected) in enumerate(choices):
                respond(key)
                wait_for(lambda: state().get('choice' + str(index)) == expected and 'confirmed' + str(index) in state(), 'choice ' + str(index))
                assert state()['confirmed' + str(index)] == ('1' if expected in ('OK', 'Yes') else '0'), state()
            respond('Return')
            wait_for(lambda: 'text' in state(), 'text output')
            assert state()['text'] == '--literal 中文  ', repr(state())
            respond('clickOK')
            wait_for(lambda: 'multiline' in state(), 'multiline output')
            assert state()['multiline'] == 'first\n中文  \n\n', repr(state())
            respond('Return')
            wait_for(lambda: 'folder' in state(), 'folder output')
            assert state()['folder'] == str(folder), repr(state())
            respond('Escape')
            wait_for(lambda: 'cancelOK' in state(), 'input cancel')
            assert state()['cancelText'] == '' and state()['cancelOK'] == '0', state()
            respond('Escape')
            wait_for(lambda: 'cancelFolder' in state(), 'folder cancel')
            assert state()['cancelFolder'] == '', state()
            wait_for(lambda: windows('^Cancel Action$'), 'action cancel dialog')
            command('xdotool', 'windowfocus', '--sync', panel)
            command('xdotool', 'key', 'Escape')
            wait_for(lambda: not dialogs(), 'dialog child stopped')
            time.sleep(.3)
            assert 'afterCancel' not in state(), state()
            command(str(BINARY), '--quit')
            assert app.wait(timeout=10) == 0
            for kind in ('userInput', 'selectFolder'):
                cancel_steps = [step(kind, {'stopIfFail': '0'}),
                                step('stateStorage', {'type': 'saveActionState', 'key': 'afterCancel', 'value': 'wrong'})]
                cancel_doc = json.dumps({'ActionType': 24, 'Title': 'Dialog Smoke', 'Data': json.dumps({'Steps': cancel_steps})})
                config.write_text('[[profiles]]\nname = "Global"\n[[profiles.actions]]\nname = "Dialog Smoke"\n'
                                  '[profiles.actions.kind]\ntype = "PluginPipeline"\nquicker_json = ' + json.dumps(cancel_doc) + '\n')
                app = subprocess.Popen([str(BINARY)], env=app_env, stdout=log, stderr=log)
                panel = wait_for(lambda: windows('^Quicker-RS$'), 'panel restart')[0]
                time.sleep(.5)
                command('xdotool', 'windowfocus', '--sync', panel)
                command('xdotool', 'mousemove', '--window', panel, '65', '142', 'click', '1')
                wait_for(dialogs, kind + ' shown')
                command('xdotool', 'windowfocus', '--sync', panel)
                command('xdotool', 'key', 'Escape')
                wait_for(lambda: not dialogs(), kind + ' stopped')
                time.sleep(.3)
                assert 'afterCancel' not in state(), state()
                command(str(BINARY), '--quit')
                assert app.wait(timeout=10) == 0
            print('PASS:', BACKEND, 'button results, whitespace, multiline, initial folder, user cancel, all three action cancels')
        except Exception:
            print('State:', state())
            print('Windows:', command('xdotool', 'search', '--onlyvisible', '--name', '.'))
            subprocess.run(['import', '-window', 'root', '/tmp/quicker-dialogs-failure.png'], env=env, timeout=10)
            log.seek(0)
            print(log.read())
            raise
        finally:
            if app.poll() is None:
                app.terminate()
                app.wait(timeout=5)
