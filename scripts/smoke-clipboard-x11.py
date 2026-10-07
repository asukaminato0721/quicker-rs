#!/usr/bin/env python3
"""Use an isolated X11 display to verify selected text and clipboard waits."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

BINARY = Path(__file__).resolve().parents[1] / 'target/debug/quicker-rs'


def wait_for(predicate, label, timeout=10):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = predicate()
        if result:
            return result
        time.sleep(0.05)
    raise AssertionError('timed out: ' + label)


def binding(value):
    return {'Value': value}


def save(key, variable):
    return {'StepRunnerKey': 'sys:stateStorage', 'InputParams': {
        'type': binding('saveActionState'), 'key': binding(key), 'value': {'VarKey': variable}}}


with tempfile.TemporaryDirectory(prefix='quicker-clipboard-') as tmp:
    env = dict(os.environ, XDG_CONFIG_HOME=tmp + '/config', XDG_RUNTIME_DIR=tmp,
               XDG_SESSION_TYPE='x11', WINIT_UNIX_BACKEND='x11')
    env.pop('WAYLAND_DISPLAY', None)

    def command(*args):
        return subprocess.run(args, env=env, capture_output=True, text=True,
                              check=True, timeout=10).stdout.strip()

    def windows(title):
        return subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', title],
                              env=env, capture_output=True, text=True).stdout.splitlines()

    steps = [
        {'StepRunnerKey': 'sys:getSelectedText', 'InputParams': {'trim': binding('true'), 'waitMs': binding('1000')},
         'OutputParams': {'output': 'text', 'outputEncoded': 'encoded'}},
        save('text', 'text'), save('encoded', 'encoded'),
        {'StepRunnerKey': 'sys:waitClipboardChange', 'InputParams': {'recentChangeMs': binding('10000')},
         'OutputParams': {'isSuccess': 'recent'}},
        save('recent', 'recent'),
        {'StepRunnerKey': 'sys:waitClipboardChange', 'InputParams': {
            'recentChangeMs': binding('0'), 'maxWaitSeconds': binding('0'), 'stopIfFail': binding('false')},
         'OutputParams': {'isSuccess': 'unchanged'}},
        save('unchanged', 'unchanged'),
    ]
    document = {'ActionType': 24, 'Title': 'Clipboard Smoke', 'Data': json.dumps({'Steps': steps})}
    config = Path(tmp) / 'config/quicker-rs/config.toml'
    config.parent.mkdir(parents=True)
    config.write_text('[[profiles]]\nname = "Global"\n[[profiles.actions]]\n'
                      'name = "Clipboard Smoke"\n[profiles.actions.kind]\n'
                      'type = "PluginPipeline"\nquicker_json = ' + json.dumps(json.dumps(document)) + '\n')
    expected = 'selected from Linux'
    # Xterm uses a custom copy binding so the test app supports Quicker's Ctrl+C contract.
    receiver = subprocess.Popen(['xterm', '-title', 'Clipboard Target', '-fa', 'Monospace', '-fs', '12',
                                 '-xrm', '*VT100.translations: #override Ctrl <Key>c: copy-selection(CLIPBOARD)',
                                 '-e', 'python3', '-c', 'import time; print("selected from Linux", flush=True); time.sleep(120)'], env=env)
    app = None
    with open(tmp + '/app.log', 'w+') as log:
        try:
            target = wait_for(lambda: windows('^Clipboard Target$'), 'target')[0]
            time.sleep(0.4)
            command('xdotool', 'windowfocus', '--sync', target)
            command('xdotool', 'mousemove', '--window', target, '12', '12', 'click', '--repeat', '3', '--delay', '80', '1')
            app = subprocess.Popen([str(BINARY), '--hidden'], env=env, stdout=log, stderr=log)
            wait_for(lambda: list(Path(tmp).glob('quicker-rs-*/control.sock')), 'control socket')
            time.sleep(0.5)
            state = config.with_name('action_state.json')
            for attempt in range(2):
                print('Clipboard workflow attempt:', attempt + 1, flush=True)
                if state.exists():
                    state.unlink()
                command(str(BINARY), '--show')
                panel = wait_for(lambda: windows('^Quicker-RS$'), 'panel')[0]
                time.sleep(0.3)
                command('xdotool', 'windowfocus', '--sync', panel)
                command('xdotool', 'mousemove', '--window', panel, '65', '142', 'click', '1')
                def complete():
                    if not state.exists():
                        return False
                    data = json.loads(state.read_text()).get('Clipboard Smoke', {})
                    return data if 'unchanged' in data else False
                data = wait_for(complete, 'clipboard workflow')
                assert data == {'text': expected, 'encoded': 'selected%20from%20Linux',
                                'recent': '1', 'unchanged': '0'}, data
                wait_for(lambda: not windows('^Quicker-RS$'), 'panel hidden')
                assert command('xdotool', 'getwindowfocus') == target
            command(str(BINARY), '--quit')
            assert app.wait(timeout=8) == 0
            print('PASS: selected text, repeated identical copy, recent change, timeout, and target focus')
        except Exception:
            panels = windows('^Quicker-RS$')
            if panels:
                command('import', '-window', panels[0], '/tmp/quicker-clipboard-error.png')
            log.seek(0)
            print(log.read())
            raise
        finally:
            for process in (app, receiver):
                if process is not None and process.poll() is None:
                    process.terminate()
                    process.wait(timeout=5)
