#!/usr/bin/env python3
"""Record the demo's CLI scenario in an isolated database using a trusted binary.

No model is downloaded by this script. Supply an installed MAG model directory.
Source revision is a caller declaration; binary and artifact digests are observed.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def capture(binary, models, revision):
    binary, models = Path(binary).resolve(strict=True), Path(models).resolve(strict=True)
    binary_hash = digest(binary)
    model_files = ('bge-small-en-v1.5-int8/model.onnx', 'bge-small-en-v1.5-int8/tokenizer.json')
    model_hashes = {name: digest(models / name) for name in model_files}
    attempts = []
    with tempfile.TemporaryDirectory(prefix='mag-demo-') as temporary:
        home = Path(temporary)
        data = home / '.mag'
        for name in model_files:
            target = data / 'models' / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(models / name, target)
        env = {'PATH': os.defpath, 'HOME': str(home), 'USERPROFILE': str(home),
               'MAG_DATA_ROOT': str(data), 'RUST_LOG': 'warn'}
        def run(args):
            start = time.monotonic()
            result = subprocess.run([str(binary), *args], cwd=home, env=env,
                                    text=True, capture_output=True, timeout=60)
            attempt = {'argv': args, 'exit_code': result.returncode,
                       'stdout': result.stdout,
                       'stderr_sha256': hashlib.sha256(result.stderr.encode()).hexdigest(),
                       'stderr_bytes': len(result.stderr.encode()),
                       'wall_seconds': time.monotonic() - start}
            attempts.append(attempt)
            if result.returncode:
                raise RuntimeError(f'CLI failed: {args[0]}: {result.stderr}')
            return result.stdout
        run(['--version'])
        seeds = [
            ('Retries use exponential backoff with jitter, capped at 30 seconds', 'project:api,decision', 'decision', '0.85', '3'),
            ('The payments worker leaked a socket when a task was cancelled mid-flight', 'project:worker,bugfix', 'error_pattern', '0.7', '4'),
            ('Schema changes must be verified against the fixture set before merge', 'project:migration,handoff', 'lesson_learned', '0.6', '4'),
        ]
        for text, tags, event, importance, priority in seeds:
            run(['ingest', text, '--tags', tags, '--event-type', event,
                 '--importance', importance, '--priority', priority, '--session-id', 'claude-code-01'])
        run(['list', '--limit', '10'])
        run(['advanced-search', 'how should retries work?', '--explain'])
        run(['advanced-search', 'what is our policy when a request fails and we try again', '--limit', '3'])
        if binary_hash != digest(binary):
            raise RuntimeError('binary changed during capture')
        if model_hashes != {name: digest(data / 'models' / name) for name in model_files}:
            raise RuntimeError('copied model artifacts changed during capture')
    return {'kind': 'recorded-cli-walkthrough-observation', 'schema_version': 1,
            'observed_at': datetime.now(timezone.utc).isoformat(),
            'declared_code_revision': revision, 'binary_sha256': binary_hash,
            'model_sha256': model_hashes, 'attempts': attempts,
            'limits': 'Trusted binary and installed models; isolated synthetic data. Not execution attestation, held-out quality, or authentication of the 4 September capture. Stderr is represented by byte count and SHA-256, not retained as text.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mag', type=Path, required=True)
    parser.add_argument('--models', type=Path, required=True)
    parser.add_argument('--code-revision', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    # Refuse replacement: an observation must not overwrite previous evidence.
    if args.output.exists():
        parser.error('output already exists')
    result = capture(args.mag, args.models, args.code_revision)
    with args.output.open('x', encoding='utf-8') as output:
        json.dump(result, output, indent=2, ensure_ascii=False, allow_nan=False)
        output.write('\n')


if __name__ == '__main__':
    main()
