"""Archive safety and integrity checks; database restore requires a real PG drill."""
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import backup


class BackupTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.data = self.root / 'live'
        (self.data / 'files').mkdir(parents=True)
        (self.data / 'files' / 'document').write_text('Retained evidence')
        (self.data / 'secret.key').write_text('fixture-only-key')
        (self.data / 'files' / 'manifest.json').write_text('ordinary document')
        self.archive = self.root / 'backup.tar.gz'

    def tearDown(self):
        self.tmp.cleanup()

    def make(self):
        def dump(args, **kwargs):
            Path(args[args.index('--file') + 1]).write_bytes(b'fixture-database-dump')
        with patch.object(backup, 'run', side_effect=dump):
            backup.backup(self.data, self.archive)

    def test_roundtrip_preserves_key_and_blobs_and_checksums(self):
        self.make()
        target = self.root / 'verified'
        target.mkdir()
        manifest = backup.unpack_checked(self.archive, target)
        self.assertEqual(len(manifest['sha256']), 4)
        self.assertEqual((target / 'data' / 'secret.key').read_text(), 'fixture-only-key')
        self.assertEqual(self.archive.stat().st_mode & 0o777, 0o600)
        with self.assertRaises(ValueError):
            backup.backup(self.data, self.archive)

    def test_rejects_traversal_and_symlinks(self):
        for name, kind in [('../escaped', tarfile.REGTYPE), ('data/link', tarfile.SYMTYPE)]:
            with tarfile.open(self.archive, 'w:gz') as tar:
                info = tarfile.TarInfo(name)
                info.type = kind
                info.linkname = '/etc/passwd' if kind == tarfile.SYMTYPE else ''
                tar.addfile(info)
            with self.assertRaises(ValueError):
                backup.unpack_checked(self.archive, self.root / 'out')
        self.assertFalse((self.root / 'escaped').exists())

    def test_rejects_tampered_contents(self):
        self.make()
        stage = self.root / 'stage'
        stage.mkdir()
        backup.unpack_checked(self.archive, stage)
        (stage / 'data' / 'files' / 'document').write_text('tampered')
        with tarfile.open(self.archive, 'w:gz') as tar:
            for name in ['database.dump', 'data', 'manifest.json']:
                tar.add(stage / name, arcname=name)
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            backup.unpack_checked(self.archive, self.root / 'bad')

    def test_restore_refuses_existing_database_before_pg_restore(self):
        self.make()
        import subprocess
        with patch.object(backup, 'run', return_value=subprocess.CompletedProcess([], 0, b'12\n')) as run:
            with self.assertRaisesRegex(ValueError, 'empty database'):
                backup.restore(self.archive, self.root / 'restored')
            self.assertEqual(run.call_count, 1)


if __name__ == '__main__':
    unittest.main()
