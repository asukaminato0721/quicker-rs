#!/usr/bin/env python3
"""Verify that a plugin activates the requested X11 window before sending keys."""
import json
import os
from pathlib import Path
import subprocess
import shlex
import signal
import sys
import tempfile
import time

BINARY = Path(__file__).resolve().parents[1] / 'target/debug/quicker-rs'


def wait_for(predicate, label):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.05)
    raise AssertionError('timed out: ' + label)


def bind(value):
    return {'Value': value}


def save(key):
    return {'StepRunnerKey': 'sys:stateStorage', 'InputParams': {
        'type': bind('saveActionState'), 'key': bind(key), 'value': {'VarKey': key}}}


with tempfile.TemporaryDirectory(prefix='quicker-window-') as tmp:
    env = dict(os.environ, XDG_CONFIG_HOME=tmp + '/config', XDG_RUNTIME_DIR=tmp,
               XDG_SESSION_TYPE='x11', WINIT_UNIX_BACKEND='x11')
    env.pop('WAYLAND_DISPLAY', None)

    def command(*args):
        return subprocess.run(args, env=env, capture_output=True, text=True,
                              check=True, timeout=10).stdout.strip()

    def windows(title):
        return subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', title],
                              env=env, capture_output=True, text=True).stdout.splitlines()

    receiver = Path(tmp) / 'receiver.py'
    receiver.write_text('import os,sys,tty\ntty.setraw(0)\n'
                        'open(sys.argv[1]+".ready", "w").close()\n'
                        'with open(sys.argv[1], "ab", buffering=0) as output:\n'
                        '    while True: output.write(os.read(0, 1))\n')
    config = Path(tmp) / 'config/quicker-rs/config.toml'
    config.parent.mkdir(parents=True)
    terminals = []
    app = None
    with open(tmp + '/app.log', 'w+') as log:
        try:
            for name in ['Original Window', 'Activation Target']:
                terminals.append(subprocess.Popen(['xterm', '-title', name, '-e', 'python3', str(receiver), tmp + '/' + name], env=env))
                wait_for(lambda: Path(tmp + '/' + name + '.ready').exists(), name)
            original = wait_for(lambda: windows('^Original Window$'), 'original')[0]
            target = wait_for(lambda: windows('^Activation Target$'), 'target')[0]
            target_pid = command('xdotool', 'getwindowpid', target)
            if '--minimized' in sys.argv:
                command('xdotool', 'windowminimize', target)
                time.sleep(0.2)
            command('xdotool', 'windowfocus', '--sync', original)
            spawn_script = Path(tmp) / 'start target with spaces'
            spawn_script.write_text('#!/bin/sh\nexec ' + shlex.join([
                'xterm', '-class', 'QuickerSpawn', '-title', 'Spawned Activation Target',
                '-e', 'python3', str(receiver), tmp + '/spawned']) + '\n')
            spawn_script.chmod(0o700)
            steps = [
                {'StepRunnerKey': 'sys:activateProcessMainWindow', 'InputParams': {
                    'process': bind('xterm'), 'className': bind('^XTerm$'), 'windowTitle': bind('^Activation Target$')},
                 'OutputParams': {'isSuccess': 'ok', 'pid': 'pid', 'mainWinHandle': 'handle', 'mainWinTitle': 'title'}},
                *[save(k) for k in ['ok', 'pid', 'handle', 'title']],
                {'StepRunnerKey': 'sys:keyInput', 'InputParams': {'keys': bind(json.dumps({'CtrlKeys': [], 'Keys': [65]}))}},
                {'StepRunnerKey': 'sys:activateProcessMainWindow', 'InputParams': {
                    'process': bind(target_pid), 'windowTitle': bind('^Missing Window$'), 'stopIfFail': bind('false')},
                 'OutputParams': {'isSuccess': 'missing', 'pid': 'missingPid', 'errMessage': 'error'}},
                *[save(k) for k in ['missing', 'missingPid', 'error']],
                {'StepRunnerKey': 'sys:activateProcessMainWindow', 'InputParams': {
                    'process': bind('QuickerSpawn'), 'path': bind(str(spawn_script)), 'windowTitle': bind('^Spawned Activation Target$')},
                 'OutputParams': {'isSuccess': 'spawnOk', 'pid': 'spawnPid', 'mainWinHandle': 'spawnHandle'}},
                *[save(k) for k in ['spawnOk', 'spawnPid', 'spawnHandle']],
                {'StepRunnerKey': 'sys:keyInput', 'InputParams': {'keys': bind(json.dumps({'CtrlKeys': [], 'Keys': [66]}))}},
            ]
            document = {'ActionType': 24, 'Title': 'Activation Smoke', 'Data': json.dumps({'Steps': steps})}
            config.write_text('[[profiles]]\nname = "Global"\n[[profiles.actions]]\nname = "Activation Smoke"\n'
                              '[profiles.actions.kind]\ntype = "PluginPipeline"\nquicker_json = ' + json.dumps(json.dumps(document)) + '\n')
            app = subprocess.Popen([str(BINARY), '--hidden'], env=env, stdout=log, stderr=log)
            wait_for(lambda: list(Path(tmp).glob('quicker-rs-*/control.sock')), 'control socket')
            time.sleep(0.5)
            command(str(BINARY), '--show')
            panel = wait_for(lambda: windows('^Quicker-RS$'), 'panel')[0]
            time.sleep(0.3)
            command('xdotool', 'windowfocus', '--sync', panel)
            command('xdotool', 'mousemove', '--window', panel, '65', '142', 'click', '1')
            state = config.with_name('action_state.json')
            def complete():
                if state.exists():
                    data = json.loads(state.read_text()).get('Activation Smoke', {})
                    return data if 'spawnHandle' in data else False
                return False
            data = wait_for(complete, 'activation workflow')
            assert data['ok'] == '1', data
            assert data['pid'] == target_pid, data
            assert data['handle'] == target, data
            assert data['title'] == 'Activation Target', data
            assert data['missing'] == '0' and data['missingPid'] == '0', data
            assert 'No matching application window' in data['error'], data
            wait_for(lambda: Path(tmp + '/Activation Target').read_bytes() == b'a', 'target key input')
            assert Path(tmp + '/Original Window').read_bytes() == b''
            assert data['spawnOk'] == '1', data
            wait_for(lambda: Path(tmp + '/spawned').exists() and Path(tmp + '/spawned').read_bytes() == b'b', 'spawned target key input')
            assert command('xdotool', 'getwindowfocus') == data['spawnHandle']
            command(str(BINARY), '--quit')
            assert app.wait(timeout=8) == 0
            print('PASS: title/class/PID filters, activation metadata, key recipients, failure outputs, and path launch')
        except Exception:
            log.seek(0)
            print(log.read())
            raise
        finally:
            for window in windows('^Spawned Activation Target$'):
                try:
                    os.kill(int(command('xdotool', 'getwindowpid', window)), signal.SIGTERM)
                except (ProcessLookupError, subprocess.SubprocessError):
                    pass
            for process in [app, *terminals]:
                if process is not None and process.poll() is None:
                    process.terminate()
                    process.wait(timeout=5)
