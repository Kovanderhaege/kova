import importlib.util
import os
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "backup.py"
SPEC = importlib.util.spec_from_file_location("kova_backup", SCRIPT)
backup = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(backup)


class BackupTests(unittest.TestCase):
    def test_backup_streams_custom_dump_and_refuses_overwrite(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "backup.dump"

            def dump(command, *, check, stdout):
                self.assertTrue(check)
                stdout.write(b"\x00custom archive\xff")

            with patch.object(backup.subprocess, "run", side_effect=dump) as run:
                backup.create_backup("kova-prod", Path("compose.yaml"), destination)

            self.assertEqual(destination.read_bytes(), b"\x00custom archive\xff")
            command = run.call_args.args[0]
            self.assertEqual(command[:6], ["docker", "compose", "--project-name", "kova-prod", "--file", "compose.yaml"])
            self.assertIn("--format=custom", command)
            self.assertIn("--no-owner", command)
            self.assertIn("--no-acl", command)
            if os.name == "posix":
                self.assertEqual(stat.S_IMODE(destination.stat().st_mode), 0o600)
            with self.assertRaises(FileExistsError):
                backup.create_backup("kova-prod", Path("compose.yaml"), destination)

    def test_failed_dump_removes_partial_archive(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "backup.dump"

            def fail_dump(command, *, check, stdout):
                stdout.write(b"partial")
                raise subprocess.CalledProcessError(1, command)

            with patch.object(backup.subprocess, "run", side_effect=fail_dump):
                with self.assertRaises(subprocess.CalledProcessError):
                    backup.create_backup("prod", Path("compose.yaml"), destination)
            self.assertFalse(destination.exists())

    def test_empty_dump_is_removed(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "backup.dump"
            with patch.object(backup.subprocess, "run"):
                with self.assertRaises(RuntimeError):
                    backup.create_backup("prod", Path("compose.yaml"), destination)
            self.assertFalse(destination.exists())

    def test_restore_creates_new_database_then_streams_archive(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "backup.dump"
            source.write_bytes(b"\x00archive\xfe")

            restored_bytes = []

            def record_restore(command, *, check, stdin=None):
                self.assertTrue(check)
                if stdin is not None:
                    restored_bytes.append(stdin.read())

            with patch.object(backup.subprocess, "run", side_effect=record_restore) as run:
                backup.restore_backup("prod", Path("compose.yaml"), source, "restore_2026")

            self.assertEqual(run.call_count, 2)
            create, restore = (call.args[0] for call in run.call_args_list)
            self.assertIn("createdb", create)
            self.assertIn("--template=template0", create)
            self.assertEqual(create[-1], "restore_2026")
            self.assertIn("pg_restore", restore)
            for option in ("--single-transaction", "--exit-on-error", "--no-owner", "--no-acl"):
                self.assertIn(option, restore)
            self.assertIn("--interactive", restore)
            self.assertEqual(restored_bytes, [b"\x00archive\xfe"])

    def test_restore_refuses_unsafe_name_or_invalid_archive_without_running_docker(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "backup.dump"
            source.write_bytes(b"archive")
            with patch.object(backup.subprocess, "run") as run:
                for name in ("kova", "postgres", "template0", "template1", "Bad", "x;drop"):
                    with self.subTest(database=name), self.assertRaises(ValueError):
                        backup.restore_backup("prod", Path("compose.yaml"), source, name)
                with self.assertRaises(ValueError):
                    backup.restore_backup("prod", Path("compose.yaml"), Path(directory), "restored")
                source.write_bytes(b"")
                with self.assertRaises(ValueError):
                    backup.restore_backup("prod", Path("compose.yaml"), source, "restored")
            run.assert_not_called()

    def test_restore_failure_keeps_archive_and_does_not_drop_database(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "backup.dump"
            source.write_bytes(b"archive")
            with patch.object(
                backup.subprocess,
                "run",
                side_effect=[None, subprocess.CalledProcessError(1, ["pg_restore"])],
            ) as run:
                with self.assertRaises(subprocess.CalledProcessError):
                    backup.restore_backup("prod", Path("compose.yaml"), source, "restored")
            self.assertEqual(source.read_bytes(), b"archive")
            self.assertEqual(run.call_count, 2)
            self.assertFalse(any("dropdb" in str(call.args[0]) for call in run.call_args_list))


if __name__ == "__main__":
    unittest.main()
