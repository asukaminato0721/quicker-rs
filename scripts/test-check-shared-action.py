#!/usr/bin/env python3
"""Offline contract tests for download errors, authentication, and API decoding."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

spec = importlib.util.spec_from_file_location('checker', Path(__file__).with_name('check-shared-action.py'))
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)
ID = '6803b583-78f7-400d-a4c1-08de12ec7091'
ACTION = {'Id': ID, 'ActionType': 24, 'Title': 'Sample', 'Revision': 3,
          'Data': '{"Steps":[{"StepRunnerKey":"sys:future"}]}'}


class Response(io.BytesIO):
    pass


class Opener:
    def __init__(self, response):
        self.response = response
        self.request = None

    def open(self, request, timeout):
        self.request = request
        if isinstance(self.response, Exception):
            raise self.response
        return Response(self.response)


class DownloadTests(unittest.TestCase):
    def test_ids_and_official_links(self):
        self.assertEqual(checker.shared_id(ID.upper()), ID)
        self.assertEqual(checker.shared_id('https://getquicker.net/Sharedaction?code=' + ID), ID)
        for value in ['../file', ID + '&x=1', 'https://evil.test/Sharedaction?code=' + ID,
                      'https://getquicker.net@evil.test/Sharedaction?code=' + ID,
                      'https://getquicker.net/Sharedaction?code=' + ID + '&code=' + ID]:
            with self.assertRaises(checker.CheckError):
                checker.shared_id(value)

    def test_bearer_header_and_size_bound(self):
        opener = Opener(b'{}')
        self.assertEqual(checker.download(checker.API + '?id=' + ID, 'test-secret', opener), b'{}')
        self.assertEqual(opener.request.get_header('Authorization'), 'Bearer test-secret')
        with self.assertRaises(checker.CheckError):
            checker.download('https://example.org/export', 'test-secret', opener)
        with self.assertRaises(checker.CheckError):
            checker.download(checker.API + '?id=' + ID, 'bad\nheader', opener)
        with patch.object(checker, 'LIMIT', 8), self.assertRaises(checker.CheckError) as caught:
            checker.download(checker.API, opener=Opener(b'x' * 9))
        self.assertEqual(caught.exception.code, 'response_too_large')

    def test_http_errors_do_not_echo_secrets(self):
        for status in [401, 403, 404, 429, 500]:
            error = urllib.error.HTTPError(checker.API, status, 'test-secret', {}, None)
            with self.assertRaises(checker.CheckError) as caught:
                checker.download(checker.API + '?id=' + ID, 'test-secret', Opener(error))
            self.assertNotIn('test-secret', str(caught.exception))
            self.assertEqual(caught.exception.code, 'authentication_required' if status in (401, 403) else 'http_error')

    def test_redirects_are_not_followed(self):
        with self.assertRaises(checker.CheckError):
            checker.NoRedirect().redirect_request(None, None, 302, '', {}, 'https://evil.test')

    def test_msi_api_envelope_and_camelcase(self):
        for envelope in [{'IsSuccess': True, 'Data': ACTION},
                         {'isSuccess': True, 'data': {k[0].lower() + k[1:]: v for k, v in ACTION.items()}}]:
            self.assertEqual(checker.decode_action(json.dumps(envelope).encode(), ID, 3, True), ACTION)
        export = {**ACTION, 'SharedActionId': ID}
        self.assertEqual(checker.decode_action(json.dumps(export).encode(), ID), export)

    def test_rejects_wrong_ids_revisions_and_error_envelopes(self):
        cases = [({'IsSuccess': False, 'Data': ACTION}, ID, 3),
                 ({'IsSuccess': True, 'Data': ACTION}, '00000000-0000-0000-0000-000000000000', 3),
                 ({'IsSuccess': True, 'Data': ACTION}, ID, 4),
                 ({'IsSuccess': True, 'Data': []}, ID, 3)]
        for value, action_id, revision in cases:
            with self.assertRaises(checker.CheckError):
                checker.decode_action(json.dumps(value).encode(), action_id, revision, True)
        for body in [b'<html>login</html>', b'[]', b'{}', b'\xff']:
            with self.assertRaises(checker.CheckError):
                checker.decode_action(body)

    def test_auth_failure_does_not_create_artifacts(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / 'out'
            with patch.object(checker, 'download', side_effect=checker.CheckError('authentication_required', 'HTTP 401')):
                with contextlib.redirect_stdout(io.StringIO()) as stream:
                    code = checker.main([ID, '--output-dir', str(output)])
            self.assertEqual(code, 2)
            self.assertEqual(json.loads(stream.getvalue())['error']['code'], 'authentication_required')
            self.assertFalse(output.exists())

    def test_official_download_to_production_checker(self):
        with tempfile.TemporaryDirectory() as tmp:
            with patch.object(checker, 'download', return_value=json.dumps({'IsSuccess': True, 'Data': ACTION}).encode()):
                with contextlib.redirect_stdout(io.StringIO()) as stream:
                    code = checker.main([ID, '--revision', '3', '--output-dir', tmp])
            report = json.loads(stream.getvalue())
            self.assertEqual(code, 1)
            self.assertEqual(report['editor_round_trip']['status'], 'pass')
            self.assertFalse(report['runtime']['executed'])
            self.assertEqual(report['source']['revision'], 3)
            path = Path(report['artifacts'])
            self.assertEqual(json.loads((path / 'action.json').read_text()), ACTION)
            self.assertEqual((path / 'source.json').stat().st_mode & 0o777, 0o600)

    def test_offline_mode_and_invalid_file_use_production_parser(self):
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / 'input.json'
            for data, expected in [(b'\xef\xbb\xbf' + json.dumps(ACTION).encode(), 1), (b'invalid json', 2)]:
                source.write_bytes(data)
                with patch.object(checker, 'download', side_effect=AssertionError('unexpected network')):
                    with contextlib.redirect_stdout(io.StringIO()) as stream:
                        code = checker.main(['--file', str(source), '--output-dir', str(Path(tmp) / 'out')])
                self.assertEqual(code, expected)
                report = json.loads(stream.getvalue())
                self.assertEqual(report['source']['kind'], 'local_export')

    def test_checker_does_not_inherit_token(self):
        result = type('Result', (), {'stdout': b'{"schema_version":1}', 'returncode': 0})()
        with patch.dict(os.environ, {'QUICKER_API_TOKEN': 'test-secret'}):
            with patch.object(checker.subprocess, 'run', return_value=result) as run:
                checker.check(Path('/checker'), Path('/action'))
        self.assertNotIn('QUICKER_API_TOKEN', run.call_args.kwargs['env'])

    def test_known_runner_check_has_no_side_effects(self):
        with tempfile.TemporaryDirectory() as tmp:
            victim = Path(tmp) / 'keep.txt'
            victim.write_text('keep this file')
            source = Path(tmp) / 'action.json'
            action = {**ACTION, 'Data': json.dumps({'Steps': [{
                'StepRunnerKey': 'sys:fileOperation',
                'InputParams': {'type': {'Value': 'deleteFile'}, 'path': {'Value': str(victim)}}
            }]})}
            source.write_text(json.dumps(action))
            with contextlib.redirect_stdout(io.StringIO()) as stream:
                code = checker.main(['--file', str(source), '--output-dir', str(Path(tmp) / 'out')])
            self.assertEqual(code, 0)
            self.assertEqual(victim.read_text(), 'keep this file')
            self.assertEqual(json.loads(stream.getvalue())['runtime']['status'], 'needs_runtime_validation')


if __name__ == '__main__':
    unittest.main()
