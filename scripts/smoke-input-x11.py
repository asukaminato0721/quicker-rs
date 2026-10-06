#!/usr/bin/env python3
"""Run under xvfb-run: verify actual macro recipient and closed-window failure."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

binary = Path(__file__).resolve().parents[1] / 'target/debug/quicker-rs'

def wait_for(predicate, label, timeout=8):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = predicate()
        if result:
            return result
        time.sleep(0.05)
    raise AssertionError(f'timed out: {label}')

with tempfile.TemporaryDirectory(prefix='quicker-input-') as tmp:
    env = dict(os.environ, XDG_CONFIG_HOME=tmp + '/config', XDG_RUNTIME_DIR=tmp,
               XDG_SESSION_TYPE='x11', WINIT_UNIX_BACKEND='x11')
    env.pop('WAYLAND_DISPLAY', None)
    def command(*args):
        return subprocess.run(args, env=env, capture_output=True, text=True,
                              check=True, timeout=10).stdout.strip()
    def windows(name):
        return subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', name],
                              env=env, capture_output=True, text=True).stdout.splitlines()
    def panel():
        return windows('^Quicker-RS$')
    def click_action(window):
        command('xdotool', 'windowfocus', '--sync', window)
        command('xdotool', 'mousemove', '--window', window, '65', '142', 'click', '1')

    text = '--literal-input-123'
    document = json.dumps({'ActionType': 7, 'Title': 'Input Smoke', 'Data': '%' + text})
    if '--output-text' in sys.argv:
        text = 'x' * 70 + '--literal-中🙂\nend\r'
        document = json.dumps({'ActionType': 24, 'Title': 'Input Smoke', 'Data': json.dumps({'Steps': [
            {'StepRunnerKey': 'sys:keyoperation', 'InputParams': {'type': {'Value': 'key_down'}, 'key': {'Value': 'LSHIFT'}}},
            {'StepRunnerKey': 'sys:outputText', 'InputParams': {
                'method': {'Value': 'input'}, 'content': {'Value': 'x' * 70}}},
            {'StepRunnerKey': 'sys:keyoperation', 'InputParams': {'type': {'Value': 'get_key_state'}, 'key': {'Value': 'LSHIFT'}},
             'OutputParams': {'isDown': 'held'}},
            {'StepRunnerKey': 'sys:stateStorage', 'InputParams': {
                'type': {'Value': 'saveActionState'}, 'key': {'Value': 'held'}, 'value': {'VarKey': 'held'}}},
            {'StepRunnerKey': 'sys:outputText', 'InputParams': {
                'method': {'Value': 'input'}, 'content': {'Value': '--literal-中🙂\r\nend'},
                'delayBetweenChar': {'Value': '3'}, 'appendReturn': {'Value': '1'}}}
        ]})})
    if '--text-cancel' in sys.argv:
        text = 'A'
        document = json.dumps({'ActionType': 24, 'Title': 'Input Smoke', 'Data': json.dumps({'Steps': [
            {'StepRunnerKey': 'sys:outputText', 'InputParams': {
                'method': {'Value': 'input'}, 'content': {'Value': 'AB'},
                'delayBetweenChar': {'Value': '1500'}, 'stopIfFail': {'Value': '0'}}},
            {'StepRunnerKey': 'sys:stateStorage', 'InputParams': {
                'type': {'Value': 'saveActionState'}, 'key': {'Value': 'afterCancel'}, 'value': {'Value': 'wrong'}}}
        ]})})
    if '--key-operation' in sys.argv:
        def key_step(operation, key):
            return {'StepRunnerKey': 'sys:keyoperation', 'InputParams': {
                'type': {'Value': operation}, 'key': {'Value': key}}}
        text = 'A '
        document = json.dumps({'ActionType': 24, 'Title': 'Input Smoke', 'Data': json.dumps({
            'Steps': [
                {'StepRunnerKey': 'sys:subprogram', 'InputParams': {'subProgram': {'Value': 'Hold Shift'}}},
                key_step('key_down', '65'), key_step('key_up', 'A'),
                key_step('key_up', 'SHIFT'), key_step('key_down', 'Space'), key_step('key_up', '0x20')],
            'SubPrograms': [{'Name': 'Hold Shift', 'Steps': [key_step('key_down', 'LSHIFT')]}]
        })})
    config = Path(tmp) / 'config/quicker-rs/config.toml'
    config.parent.mkdir(parents=True)
    config.write_text('[[profiles]]\nname = "Global"\n[[profiles.actions]]\n'
                      'name = "Input Smoke"\n[profiles.actions.kind]\n'
                      'type = "PluginPipeline"\nquicker_json = ' + json.dumps(document) + '\n')
    # Raw terminal input is persisted byte by byte; no shell consumes the macro.
    receiver = Path(tmp) / 'receiver.py'
    receiver.write_text('import os, sys, tty\n'
                        'tty.setraw(0)\n'
                        'open(sys.argv[1] + ".ready", "w").close()\n'
                        'with open(sys.argv[1], "ab", buffering=0) as output:\n'
                        '    while True: output.write(os.read(0, 1))\n')
    output = Path(tmp) / 'received'
    terminal = subprocess.Popen(['xterm', '-u8', '-title', 'Quicker Input Target', '-e',
                                 'python3', str(receiver), str(output)], env=env)
    with open(tmp + '/app.log', 'w+') as log:
        app = None
        try:
            target = wait_for(lambda: windows('^Quicker Input Target$'), 'terminal')[0]
            wait_for(lambda: Path(str(output) + '.ready').exists(), 'recipient readiness')
            command('xdotool', 'windowfocus', '--sync', target)
            args = [] if '--visible-start' in sys.argv else ['--hidden']
            app = subprocess.Popen([str(binary), *args], env=env, stdout=log, stderr=log)
            wait_for(lambda: list(Path(tmp).glob('quicker-rs-*/control.sock')), 'control socket')
            time.sleep(0.5)
            if args:
                command(str(binary), '--show')
            window = wait_for(panel, 'panel')[0]
            time.sleep(0.3)
            click_action(window)
            wait_for(lambda: output.exists() and output.read_bytes() == text.encode(), 'exact recipient text')
            wait_for(lambda: not panel(), 'input action hides panel')
            assert command('xdotool', 'getwindowfocus') == target
            if '--output-text' in sys.argv:
                state = json.loads(config.with_name('action_state.json').read_text())
                assert state['Input Smoke']['held'] == '1', state

            if '--text-cancel' in sys.argv:
                command(str(binary), '--show')
                window = wait_for(panel, 'panel during text delay')[0]
                command('xdotool', 'windowfocus', '--sync', window)
                command('xdotool', 'key', 'Escape')
                # Wait past the second character's due time. Cancellation must
                # also stop the next step when stopIfFail is false.
                time.sleep(1.8)
                assert output.read_bytes() == b'A', output.read_bytes()
                state = config.with_name('action_state.json')
                assert not state.exists() or 'afterCancel' not in state.read_text()

            # Keep the remembered target, close it, and run the same action again.
            command(str(binary), '--show')
            window = wait_for(panel, 'panel reopened')[0]
            command('xdotool', 'windowfocus', '--sync', window)
            terminal.terminate()
            terminal.wait(timeout=5)
            time.sleep(0.3)
            click_action(window)
            time.sleep(1.2)
            assert panel(), 'closed target error must reopen the panel'
            assert output.read_bytes() == text.encode(), 'closed target caused extra input'
            command(str(binary), '--quit')
            assert app.wait(timeout=8) == 0
            print('PASS: exact macro recipient, focus restoration, hidden panel, closed-target failure')
        except Exception:
            if output.exists():
                print('Received:', repr(output.read_bytes()))
                print('Expected:', repr(text.encode()))
            if panel():
                subprocess.run(['import', '-window', panel()[0], '/tmp/quicker-input-failure.png'], env=env, timeout=10)
            log.seek(0)
            print(log.read())
            raise
        finally:
            for process in (app, terminal):
                if process is not None and process.poll() is None:
                    process.terminate()
                    process.wait(timeout=5)
