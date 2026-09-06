#!/usr/bin/env python3
"""Portable Arcadia backups. Stop all writers before backup or restore.
Requires the PostgreSQL client tools matching your server major version.
Connection comes from PGDATABASE or UTOPIA_MIGRATION_URL/UTOPIA_DATABASE_URL.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile


def digest(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for block in iter(lambda: f.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def db_env():
    env = os.environ.copy()
    env['PGDATABASE'] = env.get('PGDATABASE') or env.get('UTOPIA_MIGRATION_URL') or env.get('UTOPIA_DATABASE_URL', '')
    if not env['PGDATABASE']:
        raise ValueError('Set PGDATABASE or UTOPIA_DATABASE_URL first')
    return env


def run(args, **kwargs):
    result = subprocess.run(args, env=db_env(), stderr=subprocess.PIPE, **kwargs)
    if result.returncode:
        # Connection errors can contain sensitive connection parameters.
        raise RuntimeError(f'{args[0]} failed (exit {result.returncode}); check connectivity and PostgreSQL versions')
    return result


def unpack_checked(archive, dest):
    with tarfile.open(archive, 'r:gz') as tar:
        seen = set()
        for member in tar.getmembers():
            path = Path(member.name)
            if member.name in seen or not path.parts:
                raise ValueError('Duplicate or empty backup path')
            seen.add(member.name)
            if path.is_absolute() or '..' in path.parts or not (member.isfile() or member.isdir()):
                raise ValueError('Backup contains an unsafe path or link')
            if path.parts[0] not in {'database.dump', 'manifest.json', 'data'}:
                raise ValueError('Unexpected backup member')
        tar.extractall(dest, filter='data')
    manifest = json.loads((dest / 'manifest.json').read_text())
    if manifest.get('format') != 'arcadia-backup-v1':
        raise ValueError('Unsupported backup format')
    actual = {str(p.relative_to(dest)) for p in dest.rglob('*') if p.is_file() and p != dest / 'manifest.json'}
    if actual != set(manifest['sha256']):
        raise ValueError('Backup file list does not match its manifest')
    for name, expected in manifest['sha256'].items():
        if digest(dest / name) != expected:
            raise ValueError(f'Backup checksum mismatch: {name}')
    if not (dest / 'database.dump').is_file() or not (dest / 'data' / 'files').is_dir():
        raise ValueError('Backup is missing its database or blob directory')
    return manifest


def backup(data, output):
    if not data.is_dir() or not (data / 'files').is_dir():
        raise ValueError('Data directory must contain files/')
    if output.exists():
        raise ValueError('Output exists; choose a new filename')
    if output.resolve().is_relative_to(data.resolve()):
        raise ValueError('Write backups outside the live data directory')
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='arcadia-backup-') as tmp:
        stage = Path(tmp)
        run(['pg_dump', '--format=custom', '--no-owner', '--no-privileges', '--file', str(stage / 'database.dump')])
        # Indexes and encryption key are included; keep the application stopped throughout.
        if any(p.is_symlink() for p in data.rglob('*')):
            raise ValueError('Data directory contains symlinks; use a plain data directory')
        shutil.copytree(data, stage / 'data')
        manifest = {'format': 'arcadia-backup-v1', 'sha256': {str(p.relative_to(stage)): digest(p) for p in stage.rglob('*') if p.is_file()}}
        (stage / 'manifest.json').write_text(json.dumps(manifest, indent=2))
        # Exclusive create prevents overwriting a concurrent backup.
        with output.open('xb') as raw:
            os.chmod(output, 0o600)
            with tarfile.open(fileobj=raw, mode='w:gz') as tar:
                for name in ('database.dump', 'data', 'manifest.json'):
                    tar.add(stage / name, arcname=name)
    print(f'Backup saved: {output}')


def restore(archive, data):
    if data.is_symlink() or (data.exists() and (not data.is_dir() or any(data.iterdir()))):
        raise ValueError('Restore requires an empty destination data directory')
    count = run(['psql', '-X', '-A', '-t', '-v', 'ON_ERROR_STOP=1', '-c', "SELECT count(*) FROM pg_tables WHERE schemaname NOT IN ('pg_catalog','information_schema')"], stdout=subprocess.PIPE).stdout.decode().strip()
    if count != '0':
        raise ValueError('Restore requires a new, empty database; existing databases are never overwritten')
    with tempfile.TemporaryDirectory(prefix='arcadia-restore-') as tmp:
        stage = Path(tmp)
        unpack_checked(archive, stage)
        # Stage files on the destination filesystem before changing the database.
        data.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='.arcadia-restore-', dir=data.parent) as target_tmp:
            staged_data = Path(target_tmp) / 'data'
            shutil.copytree(stage / 'data', staged_data)
            os.chmod(staged_data, 0o700)
            run(['pg_restore', '--dbname', db_env()['PGDATABASE'], '--exit-on-error', '--single-transaction', '--no-owner', '--no-privileges', str(stage / 'database.dump')], stdout=subprocess.DEVNULL)
            if data.exists():
                data.rmdir()  # Still require an empty destination at the final step.
            staged_data.rename(data)
    print('Restore finished. Start Arcadia using the matching code version and encryption key.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['backup', 'verify', 'restore'])
    parser.add_argument('archive', type=Path)
    parser.add_argument('--data-dir', type=Path, default=Path('data'))
    parser.add_argument('--writers-stopped', action='store_true', help='Confirm every app instance and ingestion writer is stopped')
    args = parser.parse_args()
    if args.action != 'verify' and not args.writers_stopped:
        parser.error('Stop every writer, then provide --writers-stopped')
    if args.action == 'backup':
        backup(args.data_dir, args.archive)
    elif args.action == 'restore':
        restore(args.archive, args.data_dir)
    else:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = unpack_checked(args.archive, Path(tmp))
        print(f"Verified {len(manifest['sha256'])} files")


if __name__ == '__main__':
    try:
        main()
    except (ValueError, RuntimeError, OSError) as exc:
        raise SystemExit(str(exc))
