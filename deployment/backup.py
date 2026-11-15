"""Create and restore PostgreSQL backups through the production Compose project."""

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path


DATABASE_NAME = re.compile(r"[a-z][a-z0-9_]{0,62}\Z")
RESERVED_DATABASES = {"kova", "postgres", "template0", "template1"}


def compose_command(project: str, compose_file: Path, *arguments: str) -> list[str]:
    return ["docker", "compose", "--project-name", project, "--file", str(compose_file), *arguments]


def create_backup(project: str, compose_file: Path, destination: Path) -> None:
    flags = os.O_CREAT | os.O_EXCL | os.O_WRONLY | getattr(os, "O_BINARY", 0)
    descriptor = os.open(destination, flags, 0o600)
    try:
        if hasattr(os, "fchmod"):
            os.fchmod(descriptor, 0o600)
        archive_file = os.fdopen(descriptor, "wb")
    except BaseException:
        os.close(descriptor)
        destination.unlink(missing_ok=True)
        raise

    with archive_file as archive:
        try:
            subprocess.run(
                compose_command(
                    project,
                    compose_file,
                    "exec",
                    "--no-TTY",
                    "db",
                    "pg_dump",
                    "--format=custom",
                    "--no-owner",
                    "--no-acl",
                    "--username=kova",
                    "--dbname=kova",
                ),
                check=True,
                stdout=archive,
            )
            archive.flush()
            if destination.stat().st_size == 0:
                raise RuntimeError("pg_dump a produit une sauvegarde vide.")
        except BaseException:
            archive.close()
            destination.unlink(missing_ok=True)
            raise


def restore_backup(project: str, compose_file: Path, source: Path, database: str) -> None:
    if not DATABASE_NAME.fullmatch(database) or database in RESERVED_DATABASES:
        raise ValueError(
            "Le nom doit commencer par une minuscule, contenir uniquement a-z, 0-9 ou _, "
            "et ne pas désigner une base système ou kova."
        )
    if not source.is_file():
        raise ValueError("La sauvegarde doit être un fichier existant.")
    if source.stat().st_size == 0:
        raise ValueError("La sauvegarde est vide.")

    subprocess.run(
        compose_command(
            project,
            compose_file,
            "exec",
            "--no-TTY",
            "db",
            "createdb",
            "--username=kova",
            "--template=template0",
            database,
        ),
        check=True,
    )
    with source.open("rb") as archive:
        subprocess.run(
            compose_command(
                project,
                compose_file,
                "exec",
                "--no-TTY",
                "--interactive",
                "db",
                "pg_restore",
                "--username=kova",
                f"--dbname={database}",
                "--single-transaction",
                "--exit-on-error",
                "--no-owner",
                "--no-acl",
            ),
            check=True,
            stdin=archive,
        )


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--project", required=True, help="Nom du projet Docker Compose.")
    result.add_argument(
        "--compose-file",
        type=Path,
        default=Path(__file__).with_name("compose.production.yaml"),
        help="Fichier Compose de production (par défaut : deployment/compose.production.yaml).",
    )
    commands = result.add_subparsers(dest="command", required=True)
    backup = commands.add_parser("backup", help="Créer une archive PostgreSQL.")
    backup.add_argument("path", type=Path)
    restore = commands.add_parser("restore", help="Restaurer vers une nouvelle base.")
    restore.add_argument("path", type=Path)
    restore.add_argument("--database", required=True, help="Nom inédit de la base restaurée.")
    return result


def main() -> int:
    arguments = parser().parse_args()
    try:
        if arguments.command == "backup":
            create_backup(arguments.project, arguments.compose_file, arguments.path)
            print(f"Sauvegarde créée : {arguments.path}")
        else:
            restore_backup(
                arguments.project,
                arguments.compose_file,
                arguments.path,
                arguments.database,
            )
            print(
                f"Restauration terminée dans {arguments.database}. "
                "Vérifiez les données et les migrations avant de basculer l’instance."
            )
        return 0
    except (OSError, subprocess.CalledProcessError, RuntimeError, ValueError) as error:
        print(f"Échec de {arguments.command} : {error}", file=sys.stderr)
        if arguments.command == "restore" and arguments.database:
            print(
                f"La base {arguments.database} a été créée ou peut contenir des données ; "
                "inspectez-la manuellement. Elle n’a pas été supprimée.",
                file=sys.stderr,
            )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
