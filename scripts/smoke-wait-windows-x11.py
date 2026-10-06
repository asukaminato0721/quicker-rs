#!/usr/bin/env python3
"""Verify waiting-window lifecycle and focus in an isolated X11 session."""
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


def marker(key):
    return step('stateStorage', {'type': 'saveActionState', 'key': key, 'value': 'yes'})


with tempfile.TemporaryDirectory(prefix='quicker-wait-windows-') as tmp:
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
            # A short-lived viewport can close after the window list is read.
            title = subprocess.run(['xdotool', 'getwindowname', window], env=env,
                                   capture_output=True, text=True, timeout=5)
            if title.returncode == 0 and title.stdout.strip() == name:
                result.append(window)
        return result

    def focus(title):
        window = wait_for(lambda: windows(title), title)[0]
        command('xdotool', 'windowactivate', '--sync', window)
        time.sleep(.3)
        return window

    def click_button(window, x=30):
        geometry = dict(line.split('=', 1) for line in command('xdotool', 'getwindowgeometry', '--shell', window).splitlines())
        command('xdotool', 'mousemove', '--window', window, str(x), str(int(geometry['HEIGHT']) - 18), 'click', '1')

    def close_cross(window):
        geometry = dict(line.split('=', 1) for line in command('xdotool', 'getwindowgeometry', '--shell', window).splitlines())
        command('xdotool', 'mousemove', '--window', window, str(int(geometry['WIDTH']) - 14), '-14', 'click', '1')

    config = base / 'config/quicker-rs/config.toml'
    config.parent.mkdir(parents=True)

    def state():
        path = config.with_name('action_state.json')
        return json.loads(path.read_text()).get('Wait Smoke', {}) if path.exists() else {}

    outputs = {'isClosed': 'closed', 'selectedOperation': 'operation'}
    originals = []
    variables = []
    if corpus := os.environ.get('QUICKER_COMPAT_CORPUS'):
        source = (Path(corpus) / 'opencc.json').read_bytes()
        assert hashlib.sha256(source).hexdigest() == 'e511eca189b4db9fa323697c263e4686cabd590b918431caab15e6d4e1bcea5e'
        data = json.loads(json.loads(source)['Data'])
        variables = [v for v in data['Variables'] if v['Key'] in ['count', '文件处理列表']]
        def collect(value):
            if isinstance(value, dict):
                if value.get('Disabled'):
                    return
                if value.get('StepRunnerKey') == 'sys:showWaitWin':
                    originals.append(value)
                for child in value.values():
                    collect(child)
            elif isinstance(value, list):
                for child in value:
                    collect(child)
        collect(data)
        assert len(originals) == 1
    workflow = [step('showWaitWin', {'mode': 'check'}, outputs), save('missing', 'closed'),
                step('assign', {'input': '1'}, {'output': 'count'}),
                step('assign', {'input': 'one\ntwo\nthree\nfour'}, {'output': '文件处理列表'})]
    workflow += originals  # The original update must also succeed without an open window.
    workflow += [step('showWaitWin', {'title': 'Wait Live', 'winLocation': 'CenterScreen',
                                    'stopActionIfClose': '0', 'btnText': 'Continue'}),
                 step('showWaitWin', {'mode': 'check'}, outputs), save('open', 'closed')]
    workflow += originals
    if not originals:
        workflow += [step('showWaitWin', {'mode': 'update', 'title': '处理进度', 'prompt': '',
                                        'btnText': '', 'progress': '2/4'})]
    workflow += [marker('originalUpdated'), step('showWaitWin', {'mode': 'waitClose'}, outputs), save('cross', 'closed'),
                 step('showWaitWin', {'mode': 'showAndWaitClose', 'title': 'Wait Choice',
                                     'winLocation': 'LastPosition', 'activateMode': 'AutoActivate',
                                     'operations': 'Accept|chosen', 'btnText': 'Done', 'progress': '-10/100', 'prompt': 'Long prompt\n' * 100}, outputs),
                 save('choice', 'operation'), save('waitClosed', 'closed'),
                 step('showWaitWin', {'mode': 'showAndWaitClose', 'title': 'Wait Not Activated',
                                     'activateMode': 'NotActivated', 'btnText': 'Continue'}),
                 step('showWaitWin', {'title': 'Wait Delay', 'btnText': 'End delay'}),
                 step('delay', {'delayMs': '30000', 'monitorWaitWin': '1'}), marker('delayEnded'),
                 step('showWaitWin', {'title': 'Wait Clipboard', 'btnText': 'End clipboard wait'}),
                 step('waitClipboardChange', {'maxWaitSeconds': '30', 'recentChangeMs': '0', 'monitorWaitWin': '1', 'stopIfFail': '0'},
                      {'isSuccess': 'clipboardOk', 'errMessage': 'clipboardError'}),
                 save('clipboardOk', 'clipboardOk'), save('clipboardError', 'clipboardError'),
                 step('showWaitWin', {'title': 'Wait Program Close', 'activateMode': 'NotActivated'}),
                 step('showWaitWin', {'mode': 'close'}), step('showWaitWin', {'mode': 'check'}, outputs), save('programClosed', 'closed'),
                 step('showWaitWin', {'mode': 'showAndWaitClose', 'title': 'Wait Timer', 'stopActionIfClose': '0', 'autoCloseSeconds': '.4'}, outputs),
                 save('timerClosed', 'closed'),
                 step('showWaitWin', {'title': 'Wait Child Shared', 'activateMode': 'NotActivated'}),
                 step('subprogram', {'subProgram': 'close shared'}),
                 step('showWaitWin', {'mode': 'check'}, outputs), save('childClosed', 'closed'),
                 step('showWaitWin', {'mode': 'showAndWaitClose', 'title': 'Wait Action Cancel', 'activateMode': 'AutoActivate'}),
                 marker('afterCancel')]
    data = {'Variables': variables, 'Steps': workflow, 'SubPrograms': [
        {'Name': 'close shared', 'Steps': [step('showWaitWin', {'mode': 'close'})]}]}
    document = json.dumps({'ActionType': 24, 'Title': 'Wait Smoke', 'Data': json.dumps(data)})
    config.write_text('[[profiles]]\nname = "Global"\n[[profiles.actions]]\nname = "Wait Smoke"\n'
                      '[profiles.actions.kind]\ntype = "PluginPipeline"\nquicker_json = ' + json.dumps(document) + '\n')
    with open(base / 'app.log', 'w+') as log:
        app = manager = None
        try:
            manager = subprocess.Popen(['kwin_x11', '--replace'], env=dict(env, KWIN_COMPOSE='N'), stdout=log, stderr=log)
            wait_for(lambda: 'window id #' in command('xprop', '-root', '_NET_SUPPORTING_WM_CHECK'), 'window manager')
            app = subprocess.Popen([str(BINARY)], env=env, stdout=log, stderr=log)
            panel = focus('Quicker-RS')
            command('xdotool', 'mousemove', '--window', panel, '65', '142', 'click', '1')
            wait_for(lambda: state().get('originalUpdated') == 'yes', 'original update')
            window = wait_for(lambda: windows('处理进度'), 'original wait-window title')[0]
            assert state()['missing'] == '1' and state()['open'] == '0', state()
            assert 'input or input focus: False' in command('xprop', '-id', window, 'WM_HINTS')
            assert 'WM_TAKE_FOCUS' not in command('xprop', '-id', window, 'WM_PROTOCOLS')
            before = command('xdotool', 'getwindowfocus')
            command('xdotool', 'mousemove', '--window', window, '40', '40', 'click', '1')
            time.sleep(.3)
            assert command('xdotool', 'getwindowfocus') == before, 'A mouse-only window stole keyboard focus'
            command('import', '-window', 'root', '/tmp/quicker-wait-windows.png')
            command('xdotool', 'windowmove', window, '170', '260')
            time.sleep(.3)
            moved = dict(line.split('=', 1) for line in command('xdotool', 'getwindowgeometry', '--shell', window).splitlines())
            close_cross(window)
            window = wait_for(lambda: windows('Wait Choice'), 'blocking choice')[0]
            wait_for(lambda: command('xdotool', 'getwindowfocus') == window, 'AutoActivate focus')
            restored = dict(line.split('=', 1) for line in command('xdotool', 'getwindowgeometry', '--shell', window).splitlines())
            assert all(abs(int(restored[key]) - int(moved[key])) <= 2 for key in ['X', 'Y']), (moved, restored)
            click_button(window)
            wait_for(lambda: state().get('choice') == 'chosen', 'return button')
            window = wait_for(lambda: windows('Wait Not Activated'), 'nonactivated window')[0]
            assert command('xdotool', 'getwindowfocus') != window, 'NotActivated stole focus on creation'
            focus('Wait Not Activated')
            assert command('xdotool', 'getwindowfocus') == window
            click_button(window)
            window = wait_for(lambda: windows('Wait Delay'), 'monitored delay')[0]
            started = time.monotonic()
            click_button(window)
            wait_for(lambda: state().get('delayEnded') == 'yes', 'delay ends on window closure', timeout=3)
            assert time.monotonic() - started < 3
            window = wait_for(lambda: windows('Wait Clipboard'), 'monitored clipboard')[0]
            time.sleep(.3)  # The clipboard monitor must enter its wait before the click.
            started = time.monotonic()
            click_button(window)
            wait_for(lambda: 'clipboardError' in state(), 'clipboard failure outputs', timeout=3)
            assert time.monotonic() - started < 3
            assert state()['clipboardOk'] == '0' and 'Wait window closed' in state()['clipboardError'], state()
            window = wait_for(lambda: windows('Wait Action Cancel'), 'cancel target')[0]
            assert state()['cross'] == '1' and state()['waitClosed'] == '1', state()
            assert state()['timerClosed'] == '1' and state()['programClosed'] == '1', state()
            assert state()['childClosed'] == '1', state()
            command(str(BINARY), '--show')
            focus('Quicker-RS')
            command('xdotool', 'key', 'Escape')
            wait_for(lambda: not windows('Wait Action Cancel'), 'action cancellation')
            time.sleep(.2)
            assert 'afterCancel' not in state(), state()
            command(str(BINARY), '--quit')
            assert app.wait(timeout=10) == 0
            # A title-bar close must stop the action when stopActionIfClose is true.
            # A window still open at normal completion must close with its root action.
            for title, settings, should_stop in [
                ('Wait Cross Stop', {'mode': 'showAndWaitClose', 'activateMode': 'AutoActivate'}, True),
                ('Wait Timeout Stop', {'mode': 'showAndWaitClose', 'autoCloseSeconds': '.4'}, True),
                ('Wait Finish', {'mode': 'show'}, False),
            ]:
                settings = dict(settings, title=title)
                final_workflow = [step('showWaitWin', settings), marker(title)]
                final_document = json.dumps({'ActionType': 24, 'Title': 'Wait Smoke',
                                             'Data': json.dumps({'Steps': final_workflow})})
                config.write_text('[[profiles]]\nname = "Global"\n[[profiles.actions]]\nname = "Wait Smoke"\n'
                                  '[profiles.actions.kind]\ntype = "PluginPipeline"\nquicker_json = ' + json.dumps(final_document) + '\n')
                app = subprocess.Popen([str(BINARY)], env=env, stdout=log, stderr=log)
                panel = focus('Quicker-RS')
                command('xdotool', 'mousemove', '--window', panel, '65', '142', 'click', '1')
                if title == 'Wait Cross Stop':
                    window = wait_for(lambda: windows(title), title)[0]
                    close_cross(window)
                elif title == 'Wait Timeout Stop':
                    wait_for(lambda: windows(title), title)
                else:
                    wait_for(lambda: state().get(title) == 'yes', 'normal completion')
                wait_for(lambda: not windows(title), 'owned window cleanup')
                time.sleep(.3)
                assert (title not in state()) == should_stop, state()
                command(str(BINARY), '--quit')
                assert app.wait(timeout=10) == 0
            print('PASS: monitored delay and clipboard failure, title-bar cancellation, timeout cancellation, normal-completion cleanup')
            print('PASS: missing window, update, nonactivation, check, cross, button, auto-close, programmatic close, shared subprogram, cancellation')
            if originals:
                print('PASS: unchanged OpenCC wait-window update, both with and without an open window')
        except Exception:
            print('State:', state())
            log.seek(0)
            print(log.read())
            subprocess.run(['import', '-window', 'root', '/tmp/quicker-wait-windows-failure.png'], env=env, timeout=10)
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
