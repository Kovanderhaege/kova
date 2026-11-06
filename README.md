# Kova

Kova est un projet d’alternative auto-hébergeable à Discord. L’idée est de permettre à chaque communauté d’héberger ses discussions sur son propre ordinateur ou serveur et de garder la maîtrise de ses données.

## Première version prévue

Depuis un navigateur, on pourra créer une communauté, inviter ses amis et discuter en temps réel dans des salons textuels. L’historique sera conservé sur la machine qui héberge la communauté.

## État du projet

Le socle Rust/Axum démarre avec PostgreSQL et applique la [migration initiale](backend/migrations/0001_initial.sql). Le deuxième jalon associe une identité Ed25519 globale à un profil local et authentifie chaque appareil par challenge signé.

## Démarrage local

Avec Docker et la variable d’environnement `POSTGRES_PASSWORD` définie, lancer depuis la racine du dépôt :

```sh
docker compose -f deployment/compose.yaml up -d --build --wait
```

[La sonde de disponibilité](http://127.0.0.1:8080/health/ready) renvoie HTTP 200 lorsque le serveur peut interroger la base. Pour arrêter les services en conservant les données, utiliser `docker compose -f deployment/compose.yaml down`.

Le test de persistance se lance avec `python backend/tests/persistence.py`. Il crée une instance temporaire, écrit un message, recrée les conteneurs et relit le message.

## Première connexion

Le [client de référence](client/identity.py) s’exécute sur la machine de l’utilisateur avec Python 3.10+ et `cryptography>=41`. Dans un environnement Python dédié, générer deux coffres chiffrés. La commande `init` affiche la clé publique racine à communiquer à l’exploitant. Conserver une sauvegarde du coffre racine et sa phrase secrète pour pouvoir autoriser un appareil de remplacement.

```sh
python -m pip install "cryptography>=41"
python client/identity.py init alice-racine.kova-vault
python client/identity.py device alice-racine.kova-vault portable.kova-vault
```

Sur l’instance, associer le premier profil administrateur à la clé publique affichée par **init**. Remplacer `CLE_PUBLIQUE_RACINE` par les 64 caractères hexadécimaux.

```sh
docker compose -f deployment/compose.yaml exec api kova-server bootstrap-admin alice Alice CLE_PUBLIQUE_RACINE
```

Sur la machine cliente, ouvrir une session et lire le profil renvoyé par `/me`. Le coffre d’appareil suffit pour les connexions suivantes.

```sh
python client/identity.py login portable.kova-vault http://127.0.0.1:8080
```

L’option `--join` demande un jeton d’invitation existant et le profil à partager. La vérification consomme l’invitation, crée ou retrouve le profil, ajoute l’adhésion et ouvre la session dans une même transaction. La création de communautés et l’émission d’invitations via HTTP relèvent du jalon 3.

## Mise à niveau depuis la version à mot de passe

Sauvegarder la base avant de démarrer la nouvelle version. La [migration 0002](backend/migrations/0002_identity.sql) invalide toutes les anciennes sessions, efface les empreintes de mots de passe et conserve les UUID des profils, les adhésions et les messages. L’exploitant vérifie hors bande la clé publique de chaque titulaire avant de rattacher son ancien identifiant. Pour le premier administrateur, utiliser `bootstrap-admin` avec son identifiant existant. Pour les autres profils historiques, utiliser `bind-profile HANDLE NOM CLE_PUBLIQUE_RACINE` depuis le même conteneur. Un profil historique garde son nom affiché.

## Protocole de session v1

Les clés publiques Ed25519 brutes et les signatures sont encodées en hexadécimal minuscule. L’identifiant global vaut `kova1-` suivi du SHA-256 hexadécimal de la clé racine brute. Le `device_id` est le SHA-256 hexadécimal de la clé publique brute de l’appareil. La racine signe directement chaque autorisation. Le client de référence fixe sa durée à 90 jours.

1. `POST /api/v1/auth/challenges` reçoit `global_identity_id`, `device_id` et `operation` (`login`, ou `join` avec `invite`). La réponse contient `challenge_id`, le texte UTF-8 à signer et `expires_at`. Le client vérifie son origine, son opération, son identité, son appareil, son invitation et l’échéance avant de signer. Le challenge expire après deux minutes.
2. `POST /api/v1/auth/verify` reçoit la signature de l’appareil, les clés publiques, l’autorisation signée par la racine et l’identifiant du challenge. Pour une première adhésion, joindre aussi `invite` et `profile: {handle, display_name}`. Le client de référence fournit les représentations exactes à signer dans `authorize` et `prove`.
3. `GET /api/v1/me` utilise le cookie hôte `HttpOnly; SameSite=Lax; Path=/`. La durée de session est limitée à sept jours et à l’échéance de l’autorisation d’appareil. `POST /api/v1/auth/logout` avec un corps vide révoque la session présentée. Les mutations exigent un en-tête `Origin` égal à `KOVA_ORIGIN`. En HTTPS, le cookie porte aussi `Secure`. HTTP est réservé à la boucle locale.
4. `GET /api/v1/identity/devices` liste les appareils connus de cette identité sur l’instance. Avec une session active, `POST /api/v1/identity/devices/revoke` accepte `device_public_key` et `signature`. La racine signe en UTF-8 les trois lignes `kova-revoke-v1`, identifiant global et `device_id`, séparées par LF sans LF final. La révocation est définitive pour cette clé et prend effet sur les sessions locales dès validation. Présenter la même preuve à chaque autre instance concernée.

Exécuter `python backend/tests/sessions.py` dans l’environnement Python du client pour tester les preuves, les sessions indépendantes, la révocation et l’usage concurrent d’une invitation sur une base PostgreSQL temporaire. Le client Vue, l’appairage par QR et la diffusion des révocations entre instances restent des jalons ultérieurs.
