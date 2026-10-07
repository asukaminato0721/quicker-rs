#!/usr/bin/env python3
"""Download a Quicker v1 action by shared ID and check it without execution."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]
LIMIT = 16 * 1024 * 1024
API = 'https://api.getquicker.net/api/SharedAction/Download'


class CheckError(Exception):
    def __init__(self, code, message):
        self.code = code
        super().__init__(message)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise CheckError('redirect_refused', 'Download redirected; no credentials were forwarded.')


def shared_id(value):
    """Accept a GUID or an official share URL, never an arbitrary download host."""
    parsed = urllib.parse.urlsplit(value)
    if parsed.scheme:
        if (parsed.scheme != 'https' or parsed.hostname not in ('getquicker.net', 'www.getquicker.net')
                or parsed.username or parsed.password or parsed.port not in (None, 443)
                or parsed.path.lower().rstrip('/') != '/sharedaction'):
            raise CheckError('invalid_id', 'Use a shared action GUID or https://getquicker.net/Sharedaction?code=GUID.')
        values = urllib.parse.parse_qs(parsed.query).get('code', [])
        if len(values) != 1:
            raise CheckError('invalid_id', 'Share URL must contain one code parameter.')
        value = values[0]
    if not re.fullmatch(r'[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}', value):
        raise CheckError('invalid_id', 'Shared action ID must be a GUID.')
    return str(uuid.UUID(value))


def download(url, token=None, opener=None):
    headers = {'Accept': 'application/json', 'User-Agent': 'quicker-rs-compat/1'}
    if token:
        if not url.startswith(API + '?') or any(c.isspace() for c in token):
            raise CheckError('invalid_token', 'Token must be a single Bearer token for the official API.')
        headers['Authorization'] = 'Bearer ' + token
    opener = opener or urllib.request.build_opener(NoRedirect())
    try:
        with opener.open(urllib.request.Request(url, headers=headers), timeout=30) as response:
            body = response.read(LIMIT + 1)
    except urllib.error.HTTPError as error:
        if error.code in (401, 403):
            raise CheckError('authentication_required',
                             f'Official download returned HTTP {error.code}. Set QUICKER_API_TOKEN locally, or use --file for an exported action.') from None
        raise CheckError('http_error', f'Download returned HTTP {error.code}.') from None
    except (urllib.error.URLError, TimeoutError, OSError):
        # Do not echo network exception messages or headers: they may contain credentials.
        raise CheckError('network_error', 'Download failed (network, TLS, or timeout).') from None
    if len(body) > LIMIT:
        raise CheckError('response_too_large', 'Download exceeds 16 MiB limit.')
    return body


def decode_action(body, action_id=None, revision=None, official=False):
    try:
        value = json.loads(body.decode('utf-8-sig'))
    except (UnicodeError, ValueError):
        raise CheckError('invalid_response', 'Download is not UTF-8 action JSON.') from None
    if not isinstance(value, dict):
        raise CheckError('invalid_response', 'Expected a JSON object.')
    if official:
        # ApiResult<SharedActionDto>, verified in Quicker.Common.dll from 1.45.5.
        success = value.get('IsSuccess', value.get('isSuccess'))
        if success is not True:
            raise CheckError('api_error', 'API did not return a successful ApiResult. Response was not cached.')
        value = value.get('Data', value.get('data'))
        if not isinstance(value, dict):
            raise CheckError('invalid_response', 'API Data is not a SharedActionDto.')
        # Support Json.NET camelCase transport. Keep original bytes separately.
        value = {key[:1].upper() + key[1:]: item for key, item in value.items()}
    if not isinstance(value.get('ActionType'), int) or not isinstance(value.get('Title'), str):
        raise CheckError('invalid_response', 'Response has no action type or title.')
    if action_id:
        actual = value.get('Id') if official else value.get('SharedActionId')
        if str(actual).lower() != action_id:
            raise CheckError('id_mismatch', 'Downloaded shared action ID differs from requested ID.')
    if official and revision is not None and value.get('Revision') != revision:
        raise CheckError('revision_mismatch', 'Downloaded revision differs from requested revision.')
    return value


def atomic_write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as stream:
        temp = Path(stream.name)
        try:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
            stream.close()
            os.replace(temp, path)
        finally:
            temp.unlink(missing_ok=True)


def json_bytes(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + '\n').encode()


def check(binary, action_path, dependency_dir=None):
    # Never pass the API token to the checker or its child processes.
    env = {key: val for key, val in os.environ.items() if key != 'QUICKER_API_TOKEN'}
    if dependency_dir is not None:
        env['QUICKER_SUBPROGRAM_DIR'] = str(dependency_dir.resolve())
    try:
        result = subprocess.run([str(binary), '--check-plugin', str(action_path)],
                                capture_output=True, timeout=60, env=env)
    except (OSError, subprocess.TimeoutExpired):
        raise CheckError('checker_unavailable', 'Could not run checker. Run cargo build --locked or set --binary.') from None
    try:
        report = json.loads(result.stdout)
        if not isinstance(report, dict) or report.get('schema_version') != 1 or result.returncode not in (0, 1, 2):
            raise ValueError()
    except (ValueError, UnicodeError):
        raise CheckError('checker_failed', 'Checker returned no valid report.') from None
    return report, result.returncode


def subprogram_id(value):
    if not re.fullmatch(r'[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}', value):
        raise CheckError('invalid_id', 'Subprogram reference ID must be a GUID.')
    return str(uuid.UUID(value))


def shared_references(document):
    """Find calls in a workflow, including embedded definitions, without execution."""
    try:
        data = json.loads(document.get('Data', '{}'))
    except (ValueError, TypeError):
        raise CheckError('invalid_workflow', 'Cannot inspect dependencies in invalid workflow Data.') from None
    pending = [(data, '/Data')]
    while pending:
        value, path = pending.pop()
        if isinstance(value, list):
            pending.extend((v, f'{path}/{i}') for i, v in enumerate(value))
        elif isinstance(value, dict):
            if value.get('Disabled') is True:
                continue
            if value.get('StepRunnerKey') == 'sys:subprogram':
                params = value.get('InputParams', {})
                binding = params.get('subProgram', {}) if isinstance(params, dict) else None
                name = binding.get('Value', '') if isinstance(binding, dict) else ''
                if (not isinstance(binding, dict) or binding.get('VarKey') is not None
                        or not isinstance(name, str) or name.startswith('$=') or '{' in name):
                    yield {'path': path, 'status': 'dynamic', 'detail': 'Subprogram reference needs runtime validation.'}
                elif name.startswith('@@'):
                    parts = name[2:].split('@', 2)
                    try:
                        if len(parts) != 3 or not re.fullmatch(r'[0-9]+', parts[1]):
                            raise ValueError()
                        action_id = subprogram_id(parts[0])
                        revision = int(parts[1])
                        if not 0 < revision <= 0xffffffff:
                            raise ValueError()
                    except (ValueError, CheckError):
                        yield {'path': path, 'status': 'error', 'code': 'invalid_subprogram_reference', 'reference': name}
                    else:
                        yield {'path': path, 'shared_id': action_id, 'revision': revision, 'reference': name}
                elif name.startswith('%%'):
                    yield {'path': path, 'status': 'global', 'reference': name,
                           'detail': 'Global subprogram requires a local export.'}
            pending.extend((v, path + '/' + str(k).replace('~', '~0').replace('/', '~1'))
                           for k, v in value.items() if isinstance(v, (dict, list)))


def validate_subprogram(document, action_id, revision):
    if str(document.get('Id', '')).lower() != action_id:
        raise CheckError('id_mismatch', 'Subprogram ID differs from the requested ID.')
    if type(document.get('Revision')) is not int or document.get('Revision') != revision:
        raise CheckError('revision_mismatch', 'Subprogram revision differs from the requested revision.')
    if document.get('ActionType') != 25:
        raise CheckError('wrong_dependency_type', 'Dependency is not a Quicker XSubProgram (type 25).')
    if not isinstance(document.get('Data'), str):
        raise CheckError('invalid_workflow', 'Subprogram has no workflow Data.')
    try:
        data = json.loads(document['Data'])
        if not isinstance(data, dict):
            raise ValueError()
    except ValueError:
        raise CheckError('invalid_workflow', 'Subprogram Data is not a workflow object.') from None


def fetch_dependencies(document, directory, soft_version, token=None):
    """Store revision-pinned dependencies. Never run downloaded steps."""
    results, seen = [], {}
    pending = [(document, 'root', 0)]
    while pending:
        parent, parent_id, depth = pending.pop()
        for reference in shared_references(parent):
            reference['parent'] = parent_id
            if reference.get('status') == 'global':
                record = None
                try:
                    action_id = subprogram_id(reference['reference'][2:])
                    identity = (action_id, None)
                    use = {'parent': parent_id, 'path': reference['path']}
                    if identity in seen:
                        seen[identity]['uses'].append(use)
                        continue
                    record = {'global_id': action_id, 'uses': [use]}
                    seen[identity] = record
                    results.append(record)
                    if len(seen) > 128 or depth >= 32:
                        record.update(status='error', code='dependency_limit', detail='Dependency graph exceeds 128 files or 32 levels.')
                        return results
                    path = directory / 'global' / f'{action_id}.json'
                    with path.open('rb') as stream:
                        body = stream.read(LIMIT + 1)
                    if len(body) > LIMIT:
                        raise CheckError('response_too_large', 'Global subprogram exceeds 16 MiB.')
                    program = json.loads(body.decode('utf-8-sig'))
                    if not isinstance(program, dict) or str(program.get('Id', '')).lower() != action_id:
                        raise CheckError('id_mismatch', 'Global subprogram ID differs from the reference.')
                    if program.get('UseServerVersion') is True:
                        raise CheckError('server_template_required', 'Global subprogram requires its server template.')
                    record.update(status='cached_global', file=str(path.resolve()), action_sha256=hashlib.sha256(body).hexdigest())
                    pending.append(({'Data': json.dumps(program)}, '%%' + action_id, depth + 1))
                except CheckError as error:
                    reference.update(status='error', code=error.code, detail=str(error))
                    # Invalid GUIDs have no record in the dependency table.
                    if record is None:
                        results.append(reference)
                    else:
                        record.update(status='error', code=error.code, detail=str(error))
                except (OSError, ValueError, UnicodeError):
                    record.update(status='error', code='global_export_unavailable', detail='Cannot read the required global subprogram export.')
                continue
            if 'status' in reference:
                results.append(reference)
                continue
            action_id, revision = reference['shared_id'], reference['revision']
            identity = (action_id, revision)
            use = {'parent': parent_id, 'path': reference['path']}
            if identity in seen:
                seen[identity]['uses'].append(use)
                continue
            record = {'shared_id': action_id, 'revision': revision, 'uses': [use]}
            seen[identity] = record
            results.append(record)
            if len(seen) > 128 or depth >= 32:
                record.update(status='error', code='dependency_limit', detail='Dependency graph exceeds 128 files or 32 levels.')
                return results
            path = directory / 'shared' / action_id / f'{revision}.json'
            source_path = path.with_suffix('.source.json')
            try:
                if path.exists():
                    with path.open('rb') as stream:
                        body = stream.read(LIMIT + 1)
                    if len(body) > LIMIT:
                        raise CheckError('response_too_large', 'Cached subprogram exceeds 16 MiB.')
                    child = decode_action(body)
                    validate_subprogram(child, action_id, revision)
                    digest = hashlib.sha256(body).hexdigest()
                    if source_path.exists():
                        source = json.loads(source_path.read_text())
                        if source.get('action_sha256') != digest:
                            raise CheckError('hash_mismatch', 'Cached subprogram differs from its recorded SHA-256.')
                    record.update(status='cached', action_sha256=digest)
                else:
                    query = urllib.parse.urlencode({'id': action_id, 'revision': revision, 'softVersion': soft_version})
                    url = API + '?' + query
                    body = download(url, token)
                    child = decode_action(body, action_id, revision, official=True)
                    validate_subprogram(child, action_id, revision)
                    action_bytes = json_bytes(child)
                    source = {'kind': 'official_api', 'url': url, 'shared_id': action_id, 'revision': revision,
                              'response_sha256': hashlib.sha256(body).hexdigest(),
                              'action_sha256': hashlib.sha256(action_bytes).hexdigest(),
                              'checked_at': datetime.now(timezone.utc).isoformat()}
                    atomic_write(path.with_suffix('.response.json'), body)
                    atomic_write(source_path, json_bytes(source))
                    atomic_write(path, action_bytes)
                    record.update(status='downloaded', **source)
                record['file'] = str(path.resolve())
                pending.append((child, f'{action_id}@{revision}', depth + 1))
            except CheckError as error:
                record.update(status='error', code=error.code, detail=str(error))
            except (OSError, ValueError):
                record.update(status='error', code='dependency_file_error', detail='Cannot read or store subprogram files.')
    return results


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', nargs='?', help='shared GUID or official share URL')
    parser.add_argument('--file', type=Path, help='check an existing export without network access')
    parser.add_argument('--revision', type=int, help='request a specific official revision')
    parser.add_argument('--soft-version', default='1.45.5.0', help='API client version (MSI reference version)')
    parser.add_argument('--public-export', action='store_true', help='use a pinned author export from the sample registry, without a token')
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/quicker-rs')
    parser.add_argument('--output-dir', type=Path, default=ROOT / '.compat')
    parser.add_argument('--with-dependencies', action='store_true', help='download pinned shared subprogram dependencies recursively, without execution')
    parser.add_argument('--dependency-dir', type=Path, help='read/store subprograms here (default with --with-dependencies: OUTPUT_DIR/subprograms)')
    args = parser.parse_args(argv)
    try:
        if bool(args.file) == bool(args.action):
            raise CheckError('invalid_arguments', 'Specify one shared ID/link or --file PATH.')
        if args.file and (args.revision is not None or args.public_export):
            raise CheckError('invalid_arguments', '--revision and --public-export require an action ID.')
        if args.revision is not None and args.revision < 1:
            raise CheckError('invalid_arguments', 'Revision must be positive.')
        if not re.fullmatch(r'\d+\.\d+\.\d+(?:\.\d+)?', args.soft_version):
            raise CheckError('invalid_arguments', 'Invalid client version.')
        if args.file:
            with args.file.open('rb') as stream:
                body = stream.read(LIMIT + 1)
            if len(body) > LIMIT:
                raise CheckError('response_too_large', 'File exceeds 16 MiB limit.')
            # Let the production parser report malformed exports, including legacy JSON.
            action_bytes = body
            source = {'kind': 'local_export', 'path': str(args.file.resolve())}
            label = 'local'
        else:
            action_id = shared_id(args.action)
            label = action_id
            if args.public_export:
                if args.revision is not None:
                    raise CheckError('invalid_arguments', 'Public exports are commit-pinned, not official action revisions.')
                registry = json.loads((ROOT / 'tests/compat/public-exports.json').read_text())
                entry = registry.get(action_id)
                if not entry:
                    raise CheckError('public_export_unavailable', 'No pinned public export for this shared ID; use the official API or --file.')
                body = download(entry['url'])
                if hashlib.sha256(body).hexdigest() != entry['sha256']:
                    raise CheckError('hash_mismatch', 'Public export does not match the recorded SHA-256.')
                document = decode_action(body, action_id)
                source = {'kind': 'public_author_export', 'shared_id': action_id, **entry}
            else:
                query = urllib.parse.urlencode({'id': action_id, 'revision': args.revision or '',
                                               'softVersion': args.soft_version, 'forPreview': 'true'})
                url = API + '?' + query
                body = download(url, os.environ.get('QUICKER_API_TOKEN'))
                document = decode_action(body, action_id, args.revision, official=True)
                source = {'kind': 'official_api', 'url': url, 'shared_id': action_id,
                          'requested_revision': args.revision, 'revision': document.get('Revision')}
            action_bytes = json_bytes(document)
        digest = hashlib.sha256(body).hexdigest()
        directory = args.output_dir / label / digest
        action_path = directory / 'action.json'
        atomic_write(directory / 'response.json', body)
        atomic_write(action_path, action_bytes)
        source.update({'response_sha256': digest,
                       'action_sha256': hashlib.sha256(action_bytes).hexdigest(),
                       'checked_at': datetime.now(timezone.utc).isoformat()})
        atomic_write(directory / 'source.json', json_bytes(source))
        dependency_dir = args.dependency_dir
        dependencies = None
        if args.with_dependencies:
            dependency_dir = dependency_dir or args.output_dir / 'subprograms'
            document = decode_action(action_bytes)
            dependencies = fetch_dependencies(document, dependency_dir, args.soft_version,
                                              os.environ.get('QUICKER_API_TOKEN'))
        report, code = check(args.binary.resolve(), action_path.resolve(), dependency_dir)
        if dependencies is not None:
            report['dependencies'] = {'directory': str(dependency_dir.resolve()), 'items': dependencies}
            if any(item.get('status') == 'error' for item in dependencies):
                report['runtime']['status'] = 'blocked'
                report['runtime']['issues'].append({'code': 'dependency_download_failed', 'severity': 'blocker',
                                                   'detail': 'See dependencies.items for failed downloads or invalid cache files.'})
                if code == 0:
                    code = 1
        report['source'] = source
        report['artifacts'] = str(directory.resolve())
        atomic_write(directory / 'report.json', json_bytes(report))
        print(json_bytes(report).decode(), end='')
        return code
    except CheckError as error:
        print(json.dumps({'schema_version': 1, 'error': {'code': error.code, 'message': str(error)}}, ensure_ascii=False))
        return 2
    except (OSError, ValueError, RecursionError):
        print(json.dumps({'schema_version': 1, 'error': {'code': 'local_error', 'message': 'Invalid local file, registry, URL, or output directory.'}}))
        return 2


if __name__ == '__main__':
    sys.exit(main())
