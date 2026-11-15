import importlib.util
import json
from pathlib import Path
import tempfile
import tarfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("compliance", Path(__file__).parents[1] / "compliance.py")
compliance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(compliance)


class AttributionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.package = self.root / "crate"
        self.package.mkdir()
        self.output = self.root / "output"
        self.supplements = self.root / "supplements"
        self.metadata = {
            "name": "example", "version": "1.0", "license": "MIT",
            "source": "registry+https://github.com/rust-lang/crates.io-index",
            "manifest_path": str(self.package / "Cargo.toml"),
        }

    def test_preserves_nested_native_notices_and_hashes(self):
        native = self.package / "native"
        native.mkdir()
        original = b"Copyright upstream\r\nLicense text\r\n"
        (native / "COPYING").write_bytes(original)
        licenses = self.package / "LICENSES"
        licenses.mkdir()
        (licenses / "MIT.txt").write_text("Upstream MIT text")
        result = compliance.collect_package(self.metadata, self.output, self.supplements)
        self.assertEqual(2, len(result["notices"]))
        record = next(record for record in result["notices"] if record["path"].endswith("COPYING"))
        copied = self.output / record["path"]
        self.assertEqual(original, copied.read_bytes())
        self.assertEqual(compliance.digest(copied), record["sha256"])

    def test_missing_material_blocks_assembly(self):
        with self.assertRaisesRegex(ValueError, "Missing upstream"):
            compliance.collect_package(self.metadata, self.output, self.supplements)

    def test_missing_declaration_blocks_assembly(self):
        (self.package / "LICENSE").write_text("upstream")
        self.metadata["license"] = None
        with self.assertRaisesRegex(ValueError, "Missing license declaration"):
            compliance.collect_package(self.metadata, self.output, self.supplements)

    def test_upstream_supplement_is_copied_with_provenance(self):
        supplement = self.supplements / "example-1.0"
        supplement.mkdir(parents=True)
        text = supplement / "LICENSE"
        text.write_text("Original upstream text")
        (supplement / "provenance.json").write_text(json.dumps({
            "revision": "upstream-commit", "status": "recovered",
            "files": [{"path": "LICENSE", "sha256": compliance.digest(text)}],
        }))
        result = compliance.collect_package(self.metadata, self.output, self.supplements)
        self.assertEqual(2, len(result["notices"]))

    def test_blocked_provenance_does_not_count_as_license_text(self):
        supplement = self.supplements / "example-1.0"
        supplement.mkdir(parents=True)
        (supplement / "provenance.json").write_text(json.dumps({"status": "blocked", "files": []}))
        with self.assertRaisesRegex(ValueError, "Unresolved upstream"):
            compliance.collect_package(self.metadata, self.output, self.supplements)

    def test_altered_supplement_blocks_assembly(self):
        supplement = self.supplements / "example-1.0"
        supplement.mkdir(parents=True)
        (supplement / "LICENSE").write_text("altered")
        (supplement / "provenance.json").write_text(json.dumps({
            "status": "recovered", "files": [{"path": "LICENSE", "sha256": "wrong"}],
        }))
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            compliance.collect_package(self.metadata, self.output, self.supplements)

    def test_license_file_cannot_escape_crate(self):
        (self.root / "outside").write_text("unrelated license")
        self.metadata["license_file"] = "../outside"
        with self.assertRaisesRegex(ValueError, "Invalid license_file"):
            compliance.collect_package(self.metadata, self.output, self.supplements)

    def test_existing_output_is_preserved(self):
        self.output.mkdir()
        sentinel = self.output / "sentinel"
        sentinel.write_text("keep")
        with self.assertRaisesRegex(ValueError, "already exists"):
            compliance.build_bundle(self.root, self.output)
        self.assertEqual("keep", sentinel.read_text())

    def assemble_fixture(self, mutate_source=False):
        source = self.root / "Cargo.toml"
        source.write_text("original workspace source")
        metadata = {"packages": [], "workspace_members": []}

        def run(arguments, workspace):
            if arguments[:2] == ["cargo", "metadata"]:
                return json.dumps(metadata)
            if arguments[:2] == ["git", "rev-parse"]:
                return "fixture-revision\n"
            if arguments[:2] == ["git", "status"]:
                return ""
            if arguments[:2] == ["cargo", "vendor"]:
                vendor = Path(arguments[-1])
                (vendor / "example-1.0").mkdir(parents=True)
                (vendor / "example-1.0/Cargo.toml").write_text("vendored source")
                if mutate_source:
                    source.write_text("changed while assembling")
                return '[source.vendored-sources]\ndirectory = ' + json.dumps(str(vendor)) + '\n'
            self.fail(f"Unexpected command: {arguments}")

        snapshot = ([source], {"Cargo.toml": compliance.digest(source)})
        with patch.object(compliance, "command", side_effect=run), patch.object(
            compliance, "source_snapshot", return_value=snapshot
        ):
            compliance.build_bundle(self.root, self.output)

    def test_archive_delivers_sources_vendor_config_and_revision(self):
        self.assemble_fixture()
        with tarfile.open(self.output / "corresponding-source.tar.gz") as archive:
            self.assertEqual(b"original workspace source", archive.extractfile("kova/Cargo.toml").read())
            self.assertEqual(b"vendored source", archive.extractfile("kova/vendor/example-1.0/Cargo.toml").read())
            config = archive.extractfile("kova/.cargo/config.toml").read().decode()
            self.assertIn('directory = "vendor"', config)
            inventory = json.load(archive.extractfile("kova/source-inventory.json"))
            self.assertEqual("fixture-revision", inventory["source_revision"])
        self.assertIn("corresponding-source.tar.gz", (self.output / "SOURCE.md").read_text())

    def test_source_mutation_during_archive_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "Source changed during assembly"):
            self.assemble_fixture(mutate_source=True)
        self.assertFalse((self.output / "SOURCE.md").exists())


if __name__ == "__main__":
    unittest.main()
