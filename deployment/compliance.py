"""Assemble source and attribution material for the locked Kova workspace.

Uses only Python's standard library, Git and Cargo. This is an inventory and
source-delivery check, not a legal opinion or an SPDX expression solver.
"""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile


NOTICE_NAMES = ("license", "licence", "copying", "notice", "copyright")


def command(arguments, workspace):
    return subprocess.check_output(arguments, cwd=workspace, text=True, encoding="utf-8")


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def notice_files(root):
    return sorted(
        path for path in root.rglob("*")
        if path.is_file() and any(
            word in part.lower() for part in path.relative_to(root).parts for word in NOTICE_NAMES
        )
    )


def collect_package(package, output, supplements):
    root = Path(package["manifest_path"]).parent
    key = f"{package['name']}-{package['version']}"
    destination = output / "THIRD-PARTY-LICENSES" / key
    files = notice_files(root)
    explicit = package.get("license_file")
    if explicit:
        path = (root / explicit).resolve()
        if not path.is_relative_to(root.resolve()) or not path.is_file():
            raise ValueError(f"Invalid license_file for {key}: {explicit}")
        files = sorted(set(files + [path]))
    supplemental = supplements / key
    provenance_path = supplemental / "provenance.json"
    provenance = None
    if provenance_path.exists():
        provenance = json.loads(provenance_path.read_text(encoding="utf-8-sig"))
        if provenance.get("status") != "recovered" or not provenance.get("files"):
            if not files:
                raise ValueError(f"Unresolved upstream license material: {key}")
        for record in provenance.get("files", []):
            path = (supplemental / record["path"]).resolve()
            if not path.is_relative_to(supplemental.resolve()) or not path.is_file():
                raise ValueError(f"Invalid supplement path: {key}")
            if digest(path) != record["sha256"]:
                raise ValueError(f"Supplement checksum mismatch: {key}")
    records = []
    for base, paths, prefix in (
        (root, files, Path("upstream")),
        (supplemental, sorted(supplemental.rglob("*")), Path("supplement")),
    ):
        for path in paths:
            if not path.is_file():
                continue
            relative = prefix / path.relative_to(base)
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, target)
            records.append({"path": target.relative_to(output).as_posix(), "sha256": digest(target)})
    if not package.get("license") and not explicit:
        raise ValueError(f"Missing license declaration: {key}")
    if not files and not (provenance_path.exists() and provenance.get("status") == "recovered"):
        raise ValueError(f"Missing upstream license/notice material: {key}")
    return {
        "name": package["name"], "version": package["version"],
        "license": package.get("license"), "source": package.get("source"),
        "repository": package.get("repository"), "authors": package.get("authors", []),
        "notices": records,
        "supplement_basis": provenance.get("reason") if provenance else None,
        "copyright_notice_missing": bool(provenance and provenance.get("copyright_notice_missing")),
    }


def source_snapshot(workspace):
    untracked = subprocess.check_output(
        ["git", "ls-files", "--others", "--exclude-standard", "-z"], cwd=workspace
    )
    if untracked:
        raise ValueError("Untracked files present: stage release sources before assembling a distribution.")
    names = subprocess.check_output(
        ["git", "ls-files", "-z"], cwd=workspace
    ).decode("utf-8").split("\0")
    files = [workspace / name for name in names if name and (workspace / name).is_file()]
    return files, {path.relative_to(workspace).as_posix(): digest(path) for path in files}


def build_bundle(workspace, output):
    if output.exists():
        raise ValueError(f"Output already exists: {output}")
    metadata = json.loads(command(["cargo", "metadata", "--locked", "--format-version", "1"], workspace))
    revision = command(["git", "rev-parse", "HEAD"], workspace).strip()
    dirty = bool(command(["git", "status", "--porcelain", "--untracked-files=no"], workspace).strip())
    files, hashes = source_snapshot(workspace)
    output.mkdir(parents=True)
    packages = []
    for package in sorted(metadata["packages"], key=lambda item: (item["name"], item["version"])):
        if package["source"]:
            packages.append(collect_package(package, output, workspace / "deployment/license-supplements"))
    inventory = {
        "source_revision": revision, "source_dirty": dirty,
        "scope": "entire locked workspace; includes build, test and other-platform dependencies",
        "workspace_packages": [
            {"name": p["name"], "version": p["version"], "license": p["license"]}
            for p in metadata["packages"] if p["id"] in metadata["workspace_members"]
        ],
        "packages": packages,
        "source_files_sha256": hashes,
    }
    inventory_path = output / "dependency-inventory.json"
    inventory_path.write_text(json.dumps(inventory, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    # Cargo supplies checksums and the exact source replacement configuration.
    # Use a new temporary directory so vendor never touches an existing tree.
    with tempfile.TemporaryDirectory(prefix="kova-source-", dir=output.parent) as temporary:
        staging = Path(temporary)
        config = command(["cargo", "vendor", "--locked", "--versioned-dirs", str(staging / "vendor")], workspace)
        config = config.replace(str(staging / "vendor").replace("\\", "\\\\"), "vendor")
        config = config.replace((staging / "vendor").as_posix(), "vendor")
        (staging / "config.toml").write_text(config, encoding="utf-8")
        with tarfile.open(output / "corresponding-source.tar.gz", "w:gz") as archive:
            for path in files:
                archive.add(path, arcname="kova/" + path.relative_to(workspace).as_posix())
            archive.add(staging / "vendor", arcname="kova/vendor")
            archive.add(staging / "config.toml", arcname="kova/.cargo/config.toml")
            archive.add(inventory_path, arcname="kova/source-inventory.json")
    if any(digest(workspace / name) != checksum for name, checksum in hashes.items()):
        raise ValueError("Source changed during assembly; discard this bundle and rebuild.")
    (output / "SOURCE.md").write_text(
        f"# Corresponding source\n\nBase Git revision: `{revision}`. Working tree modified: `{dirty}`.\n\n"
        "The accompanying `corresponding-source.tar.gz` contains the exact tracked Kova files, "
        "Cargo.lock, build/deployment scripts and all Cargo-vendored dependencies. "
        "`dependency-inventory.json` binds source files and notices by SHA-256. "
        "Dependencies retain their original licenses, including MPL source files. "
        "The inventory includes unused test/build/other-platform crates; it is not a binary linkage report.\n\n"
        "Extract, enter `kova`, install Rust and the platform prerequisites from docs/windows-spike.md, "
        "then run `cargo build --offline --locked -p kova-desktop -p kova-client-core` "
        "(add `--release` for Release), or `cargo build --offline --locked -p kova-server` on Linux. "
        "The Rust toolchain, Windows SDK/MSVC and operating-system libraries are external prerequisites. "
        "This is source delivery, not a claim of bit-for-bit reproducible binaries.\n\n"
        "Publish this material alongside each binary at the same download location, without extra charge. "
        "For a modified network server, provide its corresponding source to remote users as required "
        "by AGPL section 13; retain it when replacing a deployed version. "
        "The repository is https://github.com/Kovanderhaege/kova .\n",
        encoding="utf-8",
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    try:
        build_bundle(Path(__file__).resolve().parents[1], args.output.resolve())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Compliance assembly failed: {error}\n")


if __name__ == "__main__":
    main()
