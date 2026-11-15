# Kova

Kova est un projet d’alternative auto-hébergeable à Discord. L’idée est de permettre à chaque communauté d’héberger ses discussions sur son propre ordinateur ou serveur et de garder la maîtrise de ses données.

## Première version prévue

Le MVP vise un client desktop Rust/GPUI, une identité globale, des communautés locales et des MP E2EE répliqués avec le consentement des deux participants. Le parcours cible continue sur une instance B lorsque A est arrêtée. Le web, le mobile et la voix/vidéo sont hors MVP.

## État du projet

Le socle Rust/Axum démarre avec PostgreSQL et applique ses migrations. Le jalon 2 est terminé pour Windows : identité Ed25519 globale, profils locaux, authentification par challenge signé, coffre natif, récupération chiffrée, association et révocation d'appareils. Les preuves et limites figurent dans le [bilan du jalon](docs/migration-plan.md#finalisation-du-jalon-2--7-novembre-2026).

Le workspace comprend le serveur, le protocole, la vérification d’identité, `kova-client-core` et un prototype GPUI Windows dans `apps/desktop`. Les bibliothèques clientes ne dépendent pas du serveur. Les sessions desktop Bearer durent au plus une heure ; le client Windows utilise le coffre natif. Le jalon 3 ajoute les communautés locales, salons, invitations à usage unique, rôles et exclusions persistantes, accessibles par HTTP et depuis le prototype Windows. Le jalon 4 ajoute le chat public HTTP/WebSocket, le cache SQLite et la reprise après coupure dans le prototype Windows. Le jalon 5 ajoute les conversations privées 1:1 MLS, la vérification explicite des appareils et le cache local chiffré. Le jalon 6 ajoute la réplication MP authentifiée avec consentement signé des deux participants, reprise persistante et déduplication. Voir le [bilan du jalon 6](docs/migration-plan.md#finalisation-du-jalon-6--9-novembre-2026). Le jalon 7 ajoute la bascule automatique vers les réplicas approuvés et la reprise de fond de la conversation sélectionnée. Voir le [bilan du jalon 7](docs/migration-plan.md#finalisation-du-jalon-7--15-novembre-2026).

Voir le [bilan et plan de migration](docs/migration-plan.md), les [domaines](docs/architecture.md), le [modèle de menace](docs/threat-model.md), les [contrats](docs/protocol.md), la [réplication](docs/replication.md) et les [migrations proposées](docs/migrations.md).

Voir aussi le [prototype Windows et ses prérequis](docs/windows-spike.md) et la [politique de licence et de modèle économique](docs/decisions/licensing.md). Le parcours d’identité Windows est décrit dans [le guide desktop](docs/windows-identity.md). Le parcours communautaire est décrit dans [le guide Windows](docs/windows-communities.md).

Le parcours privé est décrit dans [le guide MP Windows](docs/windows-dm.md), avec ses limites de récupération, de durée des attestations et de support multi-appareils. Le choix OpenMLS est détaillé dans [la décision E2EE](docs/decisions/e2ee.md).

Le parcours de chat est décrit dans [le guide Windows](docs/windows-chat.md). Les messages communautaires sont lisibles par leur instance ; ils ne sont pas chiffrés de bout en bout.

## Licence et modèle économique

Le logiciel et toutes ses fonctionnalités restent gratuits, y compris pour les entreprises. Les revenus éventuels proviennent exclusivement de services facultatifs : hébergement géré, déploiement, migration, maintenance, support, SLA et développements spécifiques. L'auto-hébergement reste possible sans service officiel obligatoire.

Les licences retenues sont `AGPL-3.0-only` pour le serveur, `GPL-3.0-only` pour le desktop et le cœur client, et `Apache-2.0` pour le protocole, l'identité et la bibliothèque cryptographique. Les six manifestes Cargo déclarent cette répartition. Les textes et leur périmètre figurent dans [LICENSE.md](LICENSE.md). La vérification des droits, dépendances et notices de distribution reste nécessaire avant publication. Voir la [décision complète](docs/decisions/licensing.md).

## Vérification

`cargo fmt --all -- --check` et `cargo test --workspace --locked` vérifient le socle Rust, y compris le coffre natif sur Windows. Les suites Python `persistence.py`, `sessions.py`, `communities.py`, `chat.py`, `dm.py`, `replication.py`, `revocations.py` et `backup_restore.py` dans `crates/kova-server/tests` nécessitent Docker actif et `cryptography>=41` (`chat.py` demande aussi `websockets==17.3`) ; elles construisent des instances jetables. Construire le serveur natif avec `cargo build -p kova-server --locked` avant `replication.py`. Définir `KOVA_TEST_RUST_CLIENT=1` pour inclure les scénarios réseau du client Rust dans les suites chat, MP et réplication. Ces vérifications ont réussi localement le 9 novembre 2026. La CI est configurée, mais son passage distant n'est pas revendiqué.

## Démarrage local

Avec Docker et la variable d’environnement `POSTGRES_PASSWORD` définie, lancer depuis la racine du dépôt :

```sh
docker compose -f deployment/compose.yaml up -d --build --wait
```

[La sonde de disponibilité](http://127.0.0.1:8080/health/ready) renvoie HTTP 200 lorsque le serveur peut interroger la base. Pour arrêter les services en conservant les données, utiliser `docker compose -f deployment/compose.yaml down`.

Le test de persistance se lance avec `python crates/kova-server/tests/persistence.py`. Il crée une instance temporaire, écrit un message, recrée les conteneurs et relit le message.

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

Sur la machine cliente, ouvrir une session et lire le profil renvoyé par `/me`. Le coffre d’appareil suffit tant que son autorisation est valide (24 heures maximum) ; renouveler ensuite depuis la racine.

```sh
python client/identity.py login portable.kova-vault http://127.0.0.1:8080
```

L’option `--join` demande un jeton d’invitation existant et le profil à partager. La vérification consomme l’invitation, crée ou retrouve le profil, ajoute l’adhésion et ouvre la session dans une même transaction. Le jalon 3 fournit la création de communautés et l’émission d’invitations via HTTP ; une session existante peut aussi utiliser `/invites/redeem`. Voir les [contrats communautaires](docs/protocol.md#communautés-locales--jalon-3).

## Mise à niveau depuis la version à mot de passe

Sauvegarder la base avant de démarrer la nouvelle version. La [migration 0002](crates/kova-server/migrations/0002_identity.sql) invalide toutes les anciennes sessions, efface les empreintes de mots de passe et conserve les UUID des profils, les adhésions et les messages. L’exploitant vérifie hors bande la clé publique de chaque titulaire avant de rattacher son ancien identifiant. Pour le premier administrateur, utiliser `bootstrap-admin` avec son identifiant existant. Pour les autres profils historiques, utiliser `bind-profile HANDLE NOM CLE_PUBLIQUE_RACINE` depuis le même conteneur. Un profil historique garde son nom affiché.

## Protocole de session v1

Les clés publiques Ed25519 brutes et les signatures sont encodées en hexadécimal minuscule. L’identifiant global vaut `kova1-` suivi du SHA-256 hexadécimal de la clé racine brute. Le `device_id` est le SHA-256 hexadécimal de la clé publique brute de l’appareil. La racine signe directement chaque autorisation. L’autorisation v2 signe ses dates d’émission et d’expiration ; sa durée totale est limitée à 24 heures.

1. `POST /api/v1/auth/challenges` reçoit `global_identity_id`, `device_id` et `operation` (`login`, ou `join` avec `invite`). La réponse contient `challenge_id`, le texte UTF-8 à signer et `expires_at`. Le client vérifie son origine, son opération, son identité, son appareil, son invitation et l’échéance avant de signer. Le challenge expire après deux minutes.
2. `POST /api/v1/auth/verify` reçoit la signature de l’appareil, les clés publiques, l’autorisation signée par la racine et l’identifiant du challenge. Pour une première adhésion, joindre aussi `invite` et `profile: {handle, display_name}`. Le client de référence fournit les représentations exactes à signer dans `authorize` et `prove`.
3. `GET /api/v1/me` utilise le cookie hôte `HttpOnly; SameSite=Lax; Path=/`. La durée de session est limitée à sept jours et à l’échéance de l’autorisation d’appareil. `POST /api/v1/auth/logout` avec un corps vide révoque la session présentée. Les mutations exigent un en-tête `Origin` égal à `KOVA_ORIGIN`. En HTTPS, le cookie porte aussi `Secure`. HTTP est réservé à la boucle locale.
4. `GET /api/v1/identity/devices` liste les appareils connus de cette identité sur l’instance. Avec une session active, `POST /api/v1/identity/devices/revoke` accepte `device_public_key` et `signature`. La racine signe en UTF-8 les trois lignes `kova-revoke-v1`, identifiant global et `device_id`, séparées par LF sans LF final. La révocation est définitive pour cette clé et prend effet sur les sessions locales dès validation. Présenter la même preuve à chaque autre instance concernée.

Exécuter `python crates/kova-server/tests/sessions.py` dans l’environnement Python du client pour tester les preuves, les sessions indépendantes, la révocation et l’usage concurrent d’une invitation sur une base PostgreSQL temporaire. Le desktop propose une association par fichiers publics et retransmet les révocations aux instances connues ; le QR et la réplication automatique restent hors de ce parcours. Python est un outil transitoire de test et d’interopérabilité.

Le contrat desktop et les preuves conservées depuis la migration 0003 sont décrits dans [protocol.md](docs/protocol.md). Le cookie documenté ci-dessus reste disponible pour l'outil Python historique. Sauvegarder la base avant toute mise à niveau.
