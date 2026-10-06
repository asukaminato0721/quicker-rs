#!/usr/bin/env python3
"""Exercise real selectFile workflows in an isolated X11 and D-Bus session."""
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
WITH_WM = '--with-wm' in sys.argv[2:]

def wait_for(predicate, label, timeout=15):
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

with tempfile.TemporaryDirectory(prefix='quicker-file-dialogs-') as tmp:
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
    app_env = dict(env, PATH=str(tools), ZENITY_OK='47', ZENITY_CANCEL='0')

    def command(*args):
        return subprocess.run(args, env=env, capture_output=True, text=True,
                              check=True, timeout=10).stdout.strip()

    def windows(name):
        return subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', name],
                              env=env, capture_output=True, text=True).stdout.splitlines()

    def focus(title):
        window = wait_for(lambda: windows('^' + title + '$'), title)[0]
        command('xdotool', 'windowactivate' if WITH_WM else 'windowfocus', '--sync', window)
        time.sleep(.4)
        return window

    def respond(title, key='Return'):
        focus(title)
        command('xdotool', 'key', '--clearmodifiers', key)

    config = base / 'config/quicker-rs/config.toml'
    config.parent.mkdir(parents=True)

    def state():
        path = config.with_name('action_state.json')
        return json.loads(path.read_text()).get('File Dialog Smoke', {}) if path.exists() else {}

    def configure(steps):
        document = json.dumps({'ActionType': 24, 'Title': 'File Dialog Smoke', 'Data': json.dumps({'Steps': steps})})
        config.write_text('[[profiles]]\nname = "Global"\n[[profiles.actions]]\nname = "File Dialog Smoke"\n'
                          '[profiles.actions.kind]\ntype = "PluginPipeline"\nquicker_json = ' + json.dumps(document) + '\n')

    single_dir = base / 'single'
    single_dir.mkdir()
    single = single_dir / 'alpha 中文 %#|.txt'
    single.write_text('content')
    multi = base / 'multiple'
    multi.mkdir()
    choices = [multi / 'one 中文|%.txt', multi / 'two "quote"\nnewline.txt']
    for file in choices:
        file.write_text('content')
    existing = base / 'existing.txt'
    existing.write_text('keep')
    output = {'path': 'path', 'pathList': 'paths', 'isSuccess': 'ok'}
    workflow = [step('selectFile', {'title': 'File Open', 'initDir': str(single_dir), 'initFileName': single.name}, output),
                save('opened', 'path'), save('openOK', 'ok'), save('openList', 'paths'),
                step('selectFile', {'type': 'openMultiFile', 'title': 'File Multi', 'initDir': str(multi)}, output),
                step('stateStorage', {'type': 'saveActionState', 'key': 'first', 'value': '$= {paths}[0]'}),
                step('stateStorage', {'type': 'saveActionState', 'key': 'second', 'value': '$= {paths}[1]'}),
                step('stateStorage', {'type': 'saveActionState', 'key': 'count', 'value': '$= {paths}.Count'}),
                save('multiPath', 'path'),
                step('selectFile', {'type': 'saveFile', 'title': 'File Save', 'initDir': tmp, 'initFileName': 'result 中文'}, output),
                save('saved', 'path'), save('saveList', 'paths'),
                step('selectFile', {'type': 'saveFile', 'title': 'File Existing', 'initDir': tmp, 'initFileName': 'existing', 'stopIfFail': '0'}, output),
                save('replaceOK', 'ok'), save('replacePath', 'path'),
                step('selectFile', {'title': 'File Cancel', 'stopIfFail': '0'}, output),
                save('cancelOK', 'ok'), save('cancelPath', 'path')]
    real_files = []
    if corpus := os.environ.get('QUICKER_COMPAT_CORPUS'):
        source = (Path(corpus) / 'opencc.json').read_bytes()
        assert hashlib.sha256(source).hexdigest() == 'e511eca189b4db9fa323697c263e4686cabd590b918431caab15e6d4e1bcea5e'
        data = json.loads(json.loads(source)['Data'])
        extracted = []
        def collect(value):
            if isinstance(value, dict):
                if value.get('Disabled'):
                    return
                if value.get('StepRunnerKey') in ('sys:selectFile', 'sys:WriteTextFile'):
                    extracted.append(value)
                for child in value.values():
                    collect(child)
            elif isinstance(value, list):
                for child in value:
                    collect(child)
        collect(data['Steps'])
        selectors = [s for s in extracted if s['StepRunnerKey'] == 'sys:selectFile']
        writer = next(s for s in extracted if s['StepRunnerKey'] == 'sys:WriteTextFile'
                      and s['InputParams']['filePath'].get('VarKey') == '另存为路径')
        assert len(selectors) == 4
        workflow += [step('assign', {'input': '繁體中文\r\n'}, {'output': 'Output'})]
        for index, selector in enumerate(selectors):
            path = base / f'opencc-{index}.txt'
            real_files.append(path)
            workflow += [step('assign', {'input': str(path)}, {'output': '初始文件名'}),
                         selector, writer, save('opencc' + str(index), '另存为路径')]
    workflow += [step('selectFile', {'title': 'File Action Cancel', 'stopIfFail': '0'}),
                 step('stateStorage', {'type': 'saveActionState', 'key': 'afterCancel', 'value': 'wrong'})]
    configure(workflow)
    with open(base / 'app.log', 'w+') as log:
        app = None
        manager = None
        try:
            if WITH_WM:
                manager = subprocess.Popen(['kwin_x11', '--replace'], env=dict(env, KWIN_COMPOSE='N'), stdout=log, stderr=log)
                wait_for(lambda: 'window id #' in command('xprop', '-root', '_NET_SUPPORTING_WM_CHECK'), 'window manager')
            app = subprocess.Popen([str(BINARY)], env=app_env, stdout=log, stderr=log)
            panel = wait_for(lambda: windows('^Quicker-RS$'), 'panel')[0]
            time.sleep(.5)
            command('xdotool', 'windowfocus', '--sync', panel)
            command('xdotool', 'mousemove', '--window', panel, '65', '142', 'click', '1')
            window = focus('File Open')
            wait_for(lambda: '_NET_WM_STATE_ABOVE' in command('xprop', '-id', window, '_NET_WM_STATE'), 'topMost hint')
            command('xdotool', 'key', 'Return')
            wait_for(lambda: 'openOK' in state(), 'single output')
            assert state()['opened'] == str(single) and state()['openOK'] == '1', state()
            assert state()['openList'] == '', state()
            window = focus('File Multi')
            command('xdotool', 'mousemove', '--window', window, '280', '145' if BACKEND == 'zenity' else '115', 'click', '1')
            command('xdotool', 'key', '--clearmodifiers', 'ctrl+a')
            command('xdotool', 'key', 'Return')
            wait_for(lambda: 'multiPath' in state(), 'multiple output')
            assert {state()['first'], state()['second']} == set(map(str, choices)), state()
            assert state()['count'] == '2', state()
            assert state()['multiPath'] == '', state()
            respond('File Save')
            wait_for(lambda: 'saveList' in state(), 'save output')
            assert state()['saved'] == str(base / 'result 中文.txt'), state()
            assert state()['saveList'] == '', state()
            assert not (base / 'result 中文.txt').exists()
            existing_window = focus('File Existing')
            command('xdotool', 'key', 'Return')
            if BACKEND == 'zenity':
                # GTK can append the selected filter's extension before returning.
                wait_for(lambda: windows('^Replace file$') or command('xdotool', 'getwindowfocus') != existing_window,
                         'overwrite confirmation')
                time.sleep(.3)
                if windows('^Replace file$'):
                    respond('Replace file', 'Escape')
                else:
                    command('xdotool', 'key', 'Escape')
                    time.sleep(.3)
                    respond('File Existing', 'Escape')
            else:
                respond('Replace file', 'Escape')
            wait_for(lambda: 'replacePath' in state(), 'replacement declined')
            assert state()['replaceOK'] == '0' and state()['replacePath'] == state()['saved'], state()
            assert existing.read_text() == 'keep'
            respond('File Cancel', 'Escape')
            wait_for(lambda: 'cancelPath' in state(), 'user cancel')
            assert state()['cancelOK'] == '0' and state()['cancelPath'] == state()['saved'], state()
            for index, path in enumerate(real_files):
                respond('Save file')
                wait_for(lambda: 'opencc' + str(index) in state(), 'OpenCC save ' + str(index))
                assert state()['opencc' + str(index)] == str(path), state()
                assert path.read_bytes() == '繁體中文\r\n'.encode(), path
            focus('File Action Cancel')
            command('xdotool', 'windowactivate' if WITH_WM else 'windowfocus', '--sync', panel)
            time.sleep(.2)
            assert command('xdotool', 'getwindowfocus') == panel, 'The panel must receive action cancellation'
            command('xdotool', 'key', 'Escape')
            wait_for(lambda: not windows('^File Action Cancel$'), 'child stopped')
            time.sleep(.3)
            assert 'afterCancel' not in state(), state()
            command(str(BINARY), '--quit')
            assert app.wait(timeout=10) == 0
            print('PASS:', BACKEND, 'single/multiple/save, Unicode filenames, topMost, extension, overwrite refusal, user/action cancel')
            if real_files:
                print('PASS:', BACKEND, '4 original OpenCC selectors and the original writer with native path variables')
        except Exception:
            print('State:', state())
            subprocess.run(['import', '-window', 'root', '/tmp/quicker-file-dialogs-failure.png'], env=env, timeout=10)
            log.seek(0)
            print(log.read())
            raise
        finally:
            if app is not None and app.poll() is None:
                subprocess.run([str(BINARY), '--quit'], env=app_env, timeout=10)
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
