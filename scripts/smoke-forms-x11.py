#!/usr/bin/env python3
"""Verify native forms and unchanged OpenCC form steps in an isolated X11 session."""
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
        value = predicate()
        if value:
            return value
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


def form(title, fields, **options):
    return step('form', dict(title=title, formDef=json.dumps({'Fields': fields}), **options),
                {'isSuccess': 'ok', 'button': 'button'})


with tempfile.TemporaryDirectory(prefix='quicker-forms-') as tmp:
    base = Path(tmp)
    env = dict(os.environ, XDG_CONFIG_HOME=tmp + '/config', XDG_RUNTIME_DIR=tmp,
               XDG_SESSION_TYPE='x11', WINIT_UNIX_BACKEND='x11', QT_QPA_PLATFORM='xcb',
               XDG_CURRENT_DESKTOP='X-Generic', GDK_DEBUG='no-portals', GTK_USE_PORTAL='0', LC_ALL='C.UTF-8')
    env.pop('WAYLAND_DISPLAY', None)
    tools = base / 'bin'
    tools.mkdir()
    for name in (BACKEND, 'xdotool'):
        (tools / name).symlink_to(shutil.which(name))
    app_env = dict(env, PATH=str(tools), QT_QPA_PLATFORMTHEME='generic')

    def command(*args):
        return subprocess.run(args, env=env, capture_output=True, text=True,
                              check=True, timeout=10).stdout.strip()

    def windows(title):
        ids = subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', '.*'],
                             env=env, capture_output=True, text=True).stdout.splitlines()
        result = []
        for window in ids:
            name = subprocess.run(['xdotool', 'getwindowname', window], env=env,
                                  capture_output=True, text=True, timeout=5)
            if name.returncode == 0 and name.stdout.strip() == title:
                result.append(window)
        return result

    def focus(title):
        window = wait_for(lambda: windows(title), title)[0]
        command('xdotool', 'windowactivate', '--sync', window)
        time.sleep(.25)
        return window

    def click(window, x, y):
        command('xdotool', 'mousemove', '--window', window, str(x), str(y), 'click', '1')
        time.sleep(.15)

    def type_value(text):
        command('xdotool', 'key', 'ctrl+a')
        command('xdotool', 'type', '--clearmodifiers', '--', text)

    def submit():
        command('xdotool', 'key', 'alt+s')
        time.sleep(.2)

    def screenshot(path):
        subprocess.run(['import', '-window', 'root', path], env=env, check=True, timeout=10)

    def choose(window, y, cancel=False):
        click(window, 160, y)
        picker = focus('Select file')
        if cancel:
            command('xdotool', 'key', 'Escape')
        else:
            command('xdotool', 'key', 'ctrl+l')
            command('xdotool', 'key', '--clearmodifiers', 'ctrl+v')
            command('xdotool', 'key', 'Return')
            time.sleep(.5)
            if picker in windows('Select file'):
                command('xdotool', 'key', 'Return')
        wait_for(lambda: not windows('Select file'), 'picker closed')
        time.sleep(.25)

    config = base / 'config/quicker-rs/config.toml'
    config.parent.mkdir(parents=True)

    def state():
        path = config.with_name('action_state.json')
        return json.loads(path.read_text()).get('Form Smoke', {}) if path.exists() else {}

    originals = []
    variables = []
    if corpus := os.environ.get('QUICKER_COMPAT_CORPUS'):
        source = (Path(corpus) / 'opencc.json').read_bytes()
        assert hashlib.sha256(source).hexdigest() == 'e511eca189b4db9fa323697c263e4686cabd590b918431caab15e6d4e1bcea5e'
        data = json.loads(json.loads(source)['Data'])
        variables = data['Variables']

        def collect(value):
            if isinstance(value, dict):
                if value.get('Disabled'):
                    return
                if value.get('StepRunnerKey') == 'sys:form':
                    originals.append(value)
                for child in value.values():
                    collect(child)
            elif isinstance(value, list):
                for child in value:
                    collect(child)
        collect(data['Steps'])
        assert len(originals) == 2

    field = {'InputMethod': 1, 'FieldKey': 'name', 'Label': 'Name', 'IsRequired': True, 'Pattern': '^[A-Z]{3}$'}
    variables += [{'Key': 'name', 'Type': 0, 'DefaultValue': 'OLD'}]
    source_file = base / '字典 input.txt'
    source_file.write_text('keep this file')
    path_field = dict(field, Pattern='', TextTools='SelectSingleFile')
    workflow = [form('Form Validate', [field]), save('name', 'name'), save('ok', 'ok'),
                form('Form Cancel', [field], stopIfFail=False), save('cancelName', 'name'), save('cancelButton', 'button'),
                form('Form Close', [field], stopIfFail=False), save('closeName', 'name'), save('closeButton', 'button'),
                step('writeClipboard', {'text': str(source_file)}), form('Form Picker', [path_field]), save('picked', 'name'),
                form('Form Picker Cancel', [path_field]), save('pickerCancelled', 'name'),
                assign('name', 'a'), form('Form Choice', [dict(field, InputMethod=3, Pattern='', SelectionItems='Alpha|a\r\nBeta|b')]), save('choice', 'name')]
    if originals:
        workflow += [assign('转换模式', '智能简繁互转'), assign('输出方式', '文本窗口'),
                     assign('configList', 'Simplified|s2t\r\nTraditional|t2s'), assign('简转繁配置', 's2t'),
                     assign('繁转简配置', 't2s'), assign('build目录', '/tmp/original-build'), originals[0],
                     save('originalMode', '转换模式'), save('originalOutput', '输出方式'),
                     save('originalS2T', '简转繁配置'), save('originalT2S', '繁转简配置'), save('originalBuild', 'build目录'),
                     assign('from_what', 'text'), assign('to_what', 'ocd2'), assign('dict_input', ''), assign('dict_ouput', ''),
                     step('writeClipboard', {'text': str(source_file)}), originals[1], save('originalFrom', 'from_what'), save('originalTo', 'to_what'),
                     save('originalInput', 'dict_input'), save('originalOutputFile', 'dict_ouput')]
    workflow += [form('Form Action Cancel', [dict(field, TextTools='SelectSingleFile')], stopIfFail=False),
                 step('stateStorage', {'type': 'saveActionState', 'key': 'afterCancel', 'value': 'wrong'})]
    document = json.dumps({'ActionType': 24, 'Title': 'Form Smoke', 'Data': json.dumps({'Variables': variables, 'Steps': workflow})})
    config.write_text('[[profiles]]\nname = "Global"\n[[profiles.actions]]\nname = "Form Smoke"\n'
                      '[profiles.actions.kind]\ntype = "PluginPipeline"\nquicker_json = ' + json.dumps(document) + '\n')

    with open(base / 'app.log', 'w+') as log:
        app = manager = None
        try:
            manager = subprocess.Popen(['kwin_x11', '--replace'], env=dict(env, KWIN_COMPOSE='N'), stdout=log, stderr=log)
            wait_for(lambda: 'window id #' in command('xprop', '-root', '_NET_SUPPORTING_WM_CHECK'), 'window manager')
            app = subprocess.Popen([str(BINARY)], env=app_env, stdout=log, stderr=log)
            panel = focus('Quicker-RS')
            click(panel, 65, 142)
            window = focus('Form Validate')
            type_value('bad')
            submit()
            assert windows('Form Validate') and 'name' not in state(), state()
            command('xdotool', 'key', 'alt+r')
            submit()
            wait_for(lambda: 'ok' in state(), 'reset and submit')
            assert state()['name'] == 'OLD' and state()['ok'] == '1', state()
            window = focus('Form Cancel')
            type_value('NEW')
            command('xdotool', 'key', 'Escape')
            wait_for(lambda: 'cancelButton' in state(), 'cancelled form')
            assert state()['cancelName'] == 'OLD' and state()['cancelButton'] == 'Cancel', state()
            window = focus('Form Close')
            type_value('NEW')
            geometry = dict(line.split('=', 1) for line in command('xdotool', 'getwindowgeometry', '--shell', window).splitlines())
            click(window, int(geometry['WIDTH']) - 14, -14)
            wait_for(lambda: 'closeButton' in state(), 'closed form')
            assert state()['closeName'] == 'OLD' and state()['closeButton'] == 'Cancel', state()
            window = focus('Form Picker')
            choose(window, 46)
            focus('Form Picker')
            submit()
            wait_for(lambda: 'picked' in state(), 'file path')
            assert state()['picked'] == str(source_file), state()
            window = focus('Form Picker Cancel')
            choose(window, 46, cancel=True)
            focus('Form Picker Cancel')
            submit()
            wait_for(lambda: 'pickerCancelled' in state(), 'cancelled picker')
            assert state()['pickerCancelled'] == str(source_file), state()
            window = focus('Form Choice')
            click(window, 250, 20)
            screenshot('/tmp/quicker-form-choice.png')
            click(window, 250, 76)
            submit()
            wait_for(lambda: 'choice' in state(), 'choice')
            assert state()['choice'] == 'b', state()
            if originals:
                window = focus('选项设置')
                screenshot('/tmp/quicker-form-settings.png')
                submit()
                wait_for(lambda: 'originalBuild' in state(), 'original settings form')
                assert state()['originalMode'] == '智能简繁互转' and state()['originalOutput'] == '文本窗口', state()
                assert state()['originalS2T'] == 's2t' and state()['originalT2S'] == 't2s', state()
                assert state()['originalBuild'] == '/tmp/original-build', state()
                window = focus('字典转换')
                screenshot('/tmp/quicker-form-conversion.png')
                submit()
                assert windows('字典转换') and 'originalInput' not in state(), state()
                choose(window, 140)
                focus('字典转换')
                click(window, 200, 180)
                type_value('/tmp/output-dictionary.ocd2')
                submit()
                wait_for(lambda: 'originalOutputFile' in state(), 'original conversion form')
                assert state()['originalFrom'] == 'text' and state()['originalTo'] == 'ocd2', state()
                assert state()['originalInput'] == str(source_file), state()
                assert state()['originalOutputFile'] == '/tmp/output-dictionary.ocd2', state()
            window = focus('Form Action Cancel')
            click(window, 160, 46)
            focus('Select file')
            command(str(BINARY), '--show')
            focus('Quicker-RS')
            command('xdotool', 'key', 'Escape')
            wait_for(lambda: not windows('Form Action Cancel'), 'cancelled action')
            wait_for(lambda: not windows('Select file'), 'cancelled child picker')
            assert 'afterCancel' not in state(), state()
            assert source_file.read_text() == 'keep this file'
            command(str(BINARY), '--quit')
            assert app.wait(timeout=10) == 0
            print('PASS: form validation, reset, cancel, title-bar closure, choices, Unicode file picker, picker cancellation, action cancellation')
            if originals:
                print('PASS: two unchanged hash-verified OpenCC form steps')
        except Exception:
            print('State:', state())
            screenshot('/tmp/quicker-forms-failure.png')
            log.seek(0)
            print(log.read())
            raise
        finally:
            for process in (app, manager):
                if process and process.poll() is None:
                    process.terminate()
                    process.wait(timeout=5)
