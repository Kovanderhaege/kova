"""Client de référence du jalon identité. Dépendance : cryptography."""
import argparse
from datetime import datetime, timezone
from getpass import getpass
from hashlib import sha256
from http.cookiejar import CookieJar
import json
import os
from pathlib import Path
import secrets
import time
from urllib.parse import urlsplit
from urllib.request import build_opener, HTTPCookieProcessor, HTTPRedirectHandler, Request

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.kdf.scrypt import Scrypt


def public(key):
    return key.public_key().public_bytes_raw().hex()


def digest(value):
    return sha256(bytes.fromhex(value)).hexdigest()


def authorize(root, device):
    root_key, device_key = public(root), public(device)
    identity, device_id = "kova1-" + digest(root_key), digest(device_key)
    expiry = int(time.time()) + 90 * 86400
    message = f"kova-device-v1\n{identity}\n{device_id}\n{device_key}\n{expiry}"
    return dict(root_public_key=root_key, device_public_key=device_key,
                device_authorization=dict(expires_at=expiry, signature=root.sign(message.encode()).hex()))


def prove(device, certificate, challenge, origin, invite=None):
    identity = "kova1-" + digest(certificate["root_public_key"])
    device_id = digest(public(device))
    fields = challenge["challenge"].split("\n")
    expected = ["kova-auth-v1", origin, "join" if invite else "login", identity,
                device_id, digest(invite) if invite else "", challenge["challenge_id"]]
    if (len(fields) != 9 or fields[:7] != expected or len(bytes.fromhex(fields[7])) != 32
            or fields[8] != challenge["expires_at"]
            or datetime.fromisoformat(fields[8].replace("Z", "+00:00")) <= datetime.now(timezone.utc)):
        raise ValueError("Challenge hors contexte ou expiré")
    return dict(certificate, challenge_id=challenge["challenge_id"], invite=invite,
                signature=device.sign(challenge["challenge"].encode()).hex())


def cipher(password, salt):
    key = Scrypt(salt=salt, length=32, n=2**17, r=8, p=1).derive(password.encode())
    return AESGCM(key)


def save(path, key, password, **metadata):
    salt, nonce = secrets.token_bytes(16), secrets.token_bytes(12)
    secret = cipher(password, salt).encrypt(nonce, key.private_bytes_raw(), b"kova-vault-v1")
    with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "w") as file:
        json.dump(dict(salt=salt.hex(), nonce=nonce.hex(), secret=secret.hex(), **metadata), file)


def unlock(path):
    data = json.loads(path.read_text())
    vault = cipher(getpass("Phrase secrète du coffre : "), bytes.fromhex(data["salt"]))
    raw = vault.decrypt(bytes.fromhex(data["nonce"]), bytes.fromhex(data["secret"]), b"kova-vault-v1")
    return Ed25519PrivateKey.from_private_bytes(raw), data


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, *args):
        return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("init").add_argument("root", type=Path)
    device = commands.add_parser("device")
    device.add_argument("root", type=Path)
    device.add_argument("vault", type=Path)
    login = commands.add_parser("login")
    login.add_argument("vault", type=Path)
    login.add_argument("origin")
    login.add_argument("--join", action="store_true", help="Rejoindre une communauté sur invitation")
    args = parser.parse_args()
    if args.command in ("init", "device"):
        key = Ed25519PrivateKey.generate()
        metadata = {}
        if args.command == "device":
            root, metadata_root = unlock(args.root)
            if "certificate" in metadata_root:
                parser.error("Ce coffre contient une clé d’appareil. Le coffre racine est requis.")
            metadata["certificate"] = authorize(root, key)
        password = getpass("Nouvelle phrase secrète du coffre : ")
        if len(password) < 12 or password != getpass("Confirmer la phrase secrète : "):
            parser.error("Deux saisies identiques de 12 caractères minimum requises.")
        save(args.root if args.command == "init" else args.vault, key, password, **metadata)
        print(public(key))
        return
    origin = args.origin
    url = urlsplit(origin)
    if (url.path or url.query or url.fragment or url.username or url.password or not url.hostname
            or not (url.scheme == "https" or url.scheme == "http" and url.hostname in ("localhost","127.0.0.1","::1"))):
        parser.error("Origine HTTPS exacte ou HTTP de boucle locale requise.")
    key, data = unlock(args.vault)
    cert = data["certificate"]
    client = build_opener(HTTPCookieProcessor(CookieJar()), NoRedirect())

    def request(path, body=None):
        req = Request(origin + "/api/v1/" + path, data=json.dumps(body).encode() if body else None,
                      headers={"Origin": origin, "Content-Type": "application/json"})
        with client.open(req, timeout=10) as response:
            return json.load(response)

    invite = getpass("Jeton d’invitation : ") if args.join else None
    challenge = request("auth/challenges", dict(global_identity_id="kova1-" + digest(cert["root_public_key"]),
                                                device_id=digest(public(key)), operation="join" if args.join else "login",
                                                invite=invite))
    proof = prove(key, cert, challenge, origin, invite)
    if args.join:
        proof["profile"] = dict(handle=input("Identifiant local : "), display_name=input("Nom affiché : "))
    request("auth/verify", proof)
    print(json.dumps(request("me"), ensure_ascii=False))


if __name__ == "__main__":
    main()
