"""Run two isolated Kova servers for the connected Windows desktop recipe."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "crates" / "kova-server" / "tests"))
from replication import DatabaseInstance, call, free_port, identity, login, public, wait_health


def main():
    if os.name != "nt":
        raise RuntimeError("The connected desktop recipe requires Windows Credential Manager.")

    recipe_id = uuid4().hex
    state_dir = ROOT / "target" / f"desktop-recipe-{recipe_id}"
    state_dir.mkdir(parents=True)
    private_temp = tempfile.TemporaryDirectory(prefix="kova-desktop-recipe-")
    private_dir = Path(private_temp.name)
    databases = [DatabaseInstance(name) for name in ("a", "b")]
    origins = [f"http://127.0.0.1:{free_port()}" for _ in databases]
    binary = Path(os.environ.get("KOVA_TEST_SERVER_BINARY", ""))
    if not binary.is_file():
        target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
        suffix = ".exe" if os.name == "nt" else ""
        binary = target / "debug" / f"kova-server{suffix}"
    if not binary.is_file():
        raise FileNotFoundError(f"kova-server binary not found: {binary}")

    processes = [None, None]
    logs = [None, None]
    keys = [private_dir / "a.pk8", private_dir / "b.pk8"]
    started = time.monotonic()

    def start_server(index, suffix=""):
        database = databases[index]
        name = "a" if index == 0 else "b"
        port = origins[index].rsplit(":", 1)[1]
        other = 1 - index
        peers = [{"origin": origins[other], "public_key": peer_public[other]}]
        env = dict(os.environ, PGHOST=database.host, PGPORT=database.port,
                   PGDATABASE="kova", PGUSER="kova", PGPASSWORD=database.password,
                   KOVA_ORIGIN=origins[index], KOVA_LISTEN=f"127.0.0.1:{port}",
                   KOVA_FEDERATION_KEY_FILE=str(keys[index]),
                   KOVA_FEDERATION_PEERS=json.dumps(peers))
        log = (private_dir / f"{name}{suffix}.log").open("w", encoding="utf-8")
        process = subprocess.Popen([str(binary)], cwd=ROOT, env=env,
                                   stdout=log, stderr=subprocess.STDOUT)
        processes[index], logs[index] = process, log
        wait_health(origins[index], process)

    def stop_server(index):
        process = processes[index]
        if process is None:
            return
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        if logs[index] is not None:
            logs[index].close()
        processes[index] = None
        logs[index] = None

    def write_ack(name, detail="ok"):
        (state_dir / name).write_text(detail, encoding="utf-8")

    try:
        for database in databases:
            database.start()

        admin = identity()
        peer_public = []
        for index, key_path in enumerate(keys):
            generated = subprocess.run(
                [str(binary), "generate-federation-key", str(key_path)],
                cwd=ROOT, check=True, capture_output=True, text=True,
            )
            peer_public.append(generated.stdout.strip())

        for index in range(2):
            start_server(index)

        # Admin profiles and invitations are separate on the two database origins.
        invites = {}
        community_ids = []
        channel_ids = []
        for index, database in enumerate(databases):
            env = dict(os.environ, PGHOST=database.host, PGPORT=database.port,
                       PGDATABASE="kova", PGUSER="kova", PGPASSWORD=database.password)
            subprocess.run(
                [str(binary), "bootstrap-admin", "recipe_admin", "Recipe admin", public(admin[0])],
                cwd=ROOT, env=env, check=True, capture_output=True, text=True,
            )
            admin_token = login(origins[index], admin, handle="recipe_admin")
            community = call(origins[index], "/communities", 201, admin_token,
                             {"name": f"Desktop recipe {index}"}, "POST")
            community_ids.append(community["id"])
            channel = call(origins[index], f"/communities/{community['id']}/channels", 200,
                           admin_token)
            channel_ids.append(channel["items"][0]["id"])
            for person in ("alice", "bob"):
                invite = call(origins[index], f"/communities/{community['id']}/invites", 201,
                              admin_token, {"expires_in": 3600}, "POST")
                invites[(person, index)] = invite["invite"]

        state = {
            "recipe_id": recipe_id,
            "origins": {"a": origins[0], "b": origins[1]},
            "profiles": {},
            "identities": {},
            "communities": {"a": community_ids[0], "b": community_ids[1]},
            "channels": {"a": channel_ids[0], "b": channel_ids[1]},
            "conversation_id": None,
            "instructions": (
                "Synthetic identities remain in Windows Credential Manager and their "
                f"%LOCALAPPDATA%\\Kova\\profiles folders: kova-{recipe_id[:12]}-alice "
                f"and kova-{recipe_id[:12]}-bob. They do not affect the default profile."
            ),
        }
        (state_dir / "fixture.json").write_text(json.dumps({
            "recipe_id": recipe_id,
            "origins": state["origins"],
            "profiles": {
                "alice": f"kova-{recipe_id[:12]}-alice",
                "bob": f"kova-{recipe_id[:12]}-bob",
            },
            "communities": state["communities"],
            "channels": state["channels"],
            "instructions": state["instructions"],
        }, indent=2), encoding="utf-8")

        cargo = shutil.which("cargo") or str(Path.home() / ".cargo" / "bin" /
                                                ("cargo.exe" if os.name == "nt" else "cargo"))
        test_env = dict(os.environ,
                        KOVA_DESKTOP_RECIPE_DIR=str(state_dir),
                        KOVA_DESKTOP_RECIPE_ID=recipe_id,
                        KOVA_DESKTOP_RECIPE_ORIGIN_A=origins[0],
                        KOVA_DESKTOP_RECIPE_ORIGIN_B=origins[1],
                        KOVA_DESKTOP_RECIPE_COMMUNITY_A=community_ids[0],
                        KOVA_DESKTOP_RECIPE_COMMUNITY_B=community_ids[1],
                        KOVA_DESKTOP_RECIPE_CHANNEL_A=channel_ids[0],
                        KOVA_DESKTOP_RECIPE_CHANNEL_B=channel_ids[1])
        for person in ("alice", "bob"):
            for index, suffix in enumerate(("A", "B")):
                test_env[f"KOVA_DESKTOP_RECIPE_INVITE_{person.upper()}_{suffix}"] = invites[(person, index)]
        result = subprocess.run(
            [cargo, "test", "-p", "kova-client-core", "--locked", "--test",
             "desktop_recipe", "--", "--ignored", "--nocapture"],
            cwd=ROOT, env=test_env, capture_output=True, text=True, timeout=900,
        )
        if result.returncode:
            raise RuntimeError(f"Desktop recipe Rust setup failed ({result.returncode}):\n{result.stdout}\n{result.stderr}")

        state_path = state_dir / "state.json"
        if not state_path.is_file():
            raise RuntimeError("Rust recipe test finished without writing state.json")
        state = json.loads(state_path.read_text(encoding="utf-8"))
        allowed_keys = {"recipe_id", "origins", "profiles", "identities", "communities",
                        "channels", "conversation_id", "instructions"}
        if set(state) != allowed_keys:
            raise RuntimeError("state.json contains unexpected fields")
        print(f"Recette prête. État public: {state_path}", flush=True)
        print("Commandes : créer stop-a, start-a ou stop dans ce dossier.", flush=True)

        deadline = started + 45 * 60
        while time.monotonic() < deadline:
            if (state_dir / "stop").exists():
                (state_dir / "stop").unlink()
                break
            stop_a = state_dir / "stop-a"
            if stop_a.exists():
                stop_a.unlink()
                stop_server(0)
                write_ack("stopped-a")
            start_a = state_dir / "start-a"
            if start_a.exists():
                start_a.unlink()
                if processes[0] is None or processes[0].poll() is not None:
                    start_server(0, "-resumed")
                write_ack("started-a")
            time.sleep(0.25)
    finally:
        for index in range(2):
            stop_server(index)
        for database in databases:
            database.stop()
        private_temp.cleanup()


if __name__ == "__main__":
    main()
