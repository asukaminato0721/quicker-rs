#!/usr/bin/env python3
"""Use an isolated X11/D-Bus session to test Dolphin selection and stale copies.

Optional argument: a downloaded QuickLook forum export. Replace only its program
path with ImageMagick display and add observation steps after the original flow.
"""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time

binary = Path(__file__).resolve().parents[1] / 'target/debug/quicker-rs'


def wait_for(predicate, label, timeout=12):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.05)
    raise AssertionError('Timed out: ' + label)


with tempfile.TemporaryDirectory(prefix='quicker-files-') as tmp:
    env = dict(os.environ, XDG_CONFIG_HOME=tmp + '/config', XDG_RUNTIME_DIR=tmp,
               XDG_SESSION_TYPE='x11', WINIT_UNIX_BACKEND='x11', QT_QPA_PLATFORM='xcb',
               QT_QPA_PLATFORMTHEME='generic', NO_AT_BRIDGE='1')
    env.pop('WAYLAND_DISPLAY', None)
    env.pop('SESSION_MANAGER', None)

    def command(*args):
        return subprocess.run(args, env=env, capture_output=True, text=True,
                              check=True, timeout=10).stdout.strip()

    def windows(name, selector='--name'):
        return subprocess.run(['xdotool', 'search', '--onlyvisible', selector, name],
                              env=env, capture_output=True, text=True).stdout.splitlines()

    directory = Path(tmp) / 'Selection Target'
    directory.mkdir()
    paths = [directory / name for name in ['image2 中.ppm', 'image10 space.ppm']]
    for path in paths:
        path.write_text('P3\n2 2\n255\n255 0 0  0 255 0\n0 0 255  255 255 255\n')
    step = {'StepRunnerKey': 'sys:getSelectedFiles', 'InputParams': {
        'stopIfFail': {'Value': '0'}}, 'OutputParams': {
            'files': 'files', 'isSuccess': 'ok', 'fileCount': 'count', 'errMessage': 'error'}}
    data = {'Steps': [step]}
    document = {'ActionType': 24, 'Title': 'Files Smoke'}
    real = len(sys.argv) == 2
    if real:
        document = json.loads(Path(sys.argv[1]).read_text())
        data = json.loads(document['Data'])
        run = data['Steps'][2]['IfSteps'][1]
        assert run['StepRunnerKey'] == 'sys:run'
        assert run['InputParams']['path']['Value'] == 'QuickLook.exe'
        run['InputParams']['path']['Value'] = command('which', 'display')
        document['Title'] = 'Files Smoke'
    for key, variable in [('files', 'files'), ('ok', 'isSuccess' if real else 'ok')]:
        data['Steps'].append({'StepRunnerKey': 'sys:stateStorage', 'InputParams': {
            'type': {'Value': 'saveActionState'}, 'key': {'Value': key},
            'value': {'VarKey': variable}}})
    document['Data'] = json.dumps(data)
    state_scope = document.get('Id') or document['Title']
    config = Path(tmp) / 'config/quicker-rs/config.toml'
    config.parent.mkdir(parents=True)
    config.write_text('[[profiles]]\nname = "Global"\n[[profiles.actions]]\n'
                      'name = "Files Smoke"\n[profiles.actions.kind]\n'
                      'type = "PluginPipeline"\nquicker_json = ' + json.dumps(json.dumps(document)) + '\n')
    state_path = config.with_name('action_state.json')
    processes = []
    with open(tmp + '/session.log', 'w+') as log:
        try:
            dolphin = subprocess.Popen(['dolphin', '--new-window', str(directory)], env=env,
                                       stdout=log, stderr=log, start_new_session=True)
            processes.append(dolphin)
            target = wait_for(lambda: windows('dolphin', '--class'), 'Dolphin window', timeout=45)[0]
            time.sleep(1)
            command('xdotool', 'windowfocus', '--sync', target)
            command('xdotool', 'key', 'ctrl+a')
            app = subprocess.Popen([str(binary), '--hidden'], env=env,
                                   stdout=log, stderr=log, start_new_session=True)
            processes.append(app)
            wait_for(lambda: list(Path(tmp).glob('quicker-rs-*/control.sock')), 'control socket')
            time.sleep(0.5)

            def execute(target):
                if state_path.exists():
                    state_path.unlink()
                command('xdotool', 'windowfocus', '--sync', target)
                command(str(binary), '--show')
                panel = wait_for(lambda: windows('^Quicker-RS$'), 'panel')[0]
                time.sleep(0.3)
                command('xdotool', 'windowfocus', '--sync', panel)
                command('xdotool', 'mousemove', '--window', panel, '65', '142', 'click', '1')

                def completed():
                    try:
                        state = json.loads(state_path.read_text()).get(state_scope, {})
                        return state if 'ok' in state else None
                    except (OSError, ValueError):
                        return None
                return wait_for(completed, 'selection result')

            for _ in range(2):
                state = execute(target)
                assert state['ok'] == '1', state
                assert state['files'] == ','.join(str(p) for p in paths), state
            if real:
                for path in paths:
                    wait_for(lambda: any(path.name in command('xdotool', 'getwindowname', window)
                                         for window in windows('display', '--class')),
                             'preview for ' + path.name)
                print('PASS: downloaded QuickLook flow opened both selected images through Linux display')
            else:
                # This terminal does not copy files. The previous Dolphin list
                # stays on the clipboard. The action must not reuse that list.
                terminal = subprocess.Popen(['xterm', '-title', 'No File Selection', '-e', 'sleep', '30'],
                                            env=env, stdout=log, stderr=log, start_new_session=True)
                processes.append(terminal)
                other = wait_for(lambda: windows('^No File Selection$'), 'non-file target')[0]
                state = execute(other)
                assert state == {'files': '', 'ok': '0'}, state
                print('PASS: Dolphin files, repeated identical copy, Unicode paths, numeric sort, stale-copy rejection')
            command(str(binary), '--quit')
            assert app.wait(timeout=8) == 0
        except Exception:
            for window in windows('.'):
                print('Window:', window, command('xdotool', 'getwindowname', window))
            log.seek(0)
            print(log.read())
            raise
        finally:
            for process in reversed(processes):
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                process.wait(timeout=5)
