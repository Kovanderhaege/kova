# Registre des décisions techniques

Vérification du dépôt et consultation des références officielles : **6 novembre 2026**. Ce registre conserve les choix existants ; il ne prescrit pas une mise à niveau générale. Les versions exactes ci-dessous proviennent de `Cargo.lock`, les contraintes des `Cargo.toml`. Une documentation consultée ne constitue ni un audit de sécurité ni une validation d'exécution.

Le chemin de référence est `docs/tech-stack.md`, conformément à `AGENTS.md`. Les contrats détaillés restent dans [protocol.md](protocol.md), les preuves et limites dans [migration-plan.md](migration-plan.md) et [windows-spike.md](windows-spike.md).

## 1. Rust et séparation des composants

- **Versions :** édition Rust 2024, workspace Cargo avec `resolver = "3"`. L'édition exige au moins Rust 1.85 ; ce seuil n'est pas un MSRV validé pour l'ensemble des dépendances. Aucun `rust-version` ni toolchain exacte n'est fixé ; CI sur `stable`, build Docker sur `rust:1-bookworm`.
- **Approche :** `kova-server` porte HTTP et PostgreSQL ; `kova-protocol` les formats signés ; `kova-identity` la vérification publique ; `kova-client-core` les secrets et services clients ; `apps/desktop` l'interface. Les bibliothèques partagées et le cœur client ne dépendent pas du serveur. Conserver `Cargo.lock` et utiliser `--locked` pour les vérifications et builds.
- **Raison :** partager les contrats sans coupler les clients au stockage serveur. Ne créer d'autres crates que pour une fonctionnalité testable ; ne pas anticiper une infrastructure de microservices.
- **Références :** [édition 2024](https://doc.rust-lang.org/edition-guide/rust-2024/index.html), [résolution Cargo](https://doc.rust-lang.org/cargo/reference/resolver.html#resolver-versions). Vérifiées le 2026-11-06.

## 2. API serveur asynchrone

- **Versions :** Axum `0.8` → **0.8.9**, Tokio `1` → **1.54.1**, Serde `1` → **1.0.229**, serde_json `1` → **1.0.154**.
- **Approche :** routeur Axum, état partagé via `State`, écoute `TcpListener`, `axum::serve` et arrêt gracieux. Tokio active `macros`, `rt-multi-thread`, `net`, `signal`, `time`. Échanges HTTP JSON sous `/api/v1` ; sonde `/health/ready` interrogeant réellement PostgreSQL.
- **Raison :** prolonger le serveur modulaire existant avec le même runtime que SQLx. Ne pas reprendre les exemples d'anciennes versions reposant sur `axum::Server`.
- **Limite :** les routes communautaires, le chat et WebSocket ne sont pas implémentés ; leur présence dans la cible MVP ne constitue pas un choix de bibliothèque supplémentaire.
- **Références :** [Axum 0.8.9](https://docs.rs/axum/0.8.9/axum/), [Tokio 1.54.1](https://docs.rs/tokio/1.54.1/tokio/). Vérifiées le 2026-11-06.

## 3. Persistance serveur et migrations

- **Versions :** PostgreSQL **17**, image `postgres:17-alpine` (mineure et digest non figés) ; SQLx `0.8` → **0.8.6**.
- **Approche :** pool PostgreSQL, SQL explicite et transactions ; SQLx sans fonctionnalités par défaut, avec `runtime-tokio`, `postgres`, `macros`, `migrate`. Les migrations embarquées par `sqlx::migrate!()` sont appliquées au démarrage ; `build.rs` surveille leur répertoire.
- **Raison :** conserver les garanties transactionnelles des sessions, invitations et adhésions. Les migrations déjà appliquées restent immuables ; toute évolution passe par une nouvelle migration. Les fichiers `docs/schema.sql` et `docs/migrations.md` décrivent des propositions, pas le schéma effectivement livré.
- **Limites :** PostgreSQL 17 appartient à une branche supportée selon la politique officielle consultée. SQLx n'active aucun backend TLS dans le manifeste actuel : ne pas considérer la connexion à une base distante comme sécurisée sans décision de déploiement supplémentaire.
- **Références :** [SQLx 0.8.6, runtime et TLS](https://docs.rs/sqlx/0.8.6/sqlx/), [support PostgreSQL](https://www.postgresql.org/support/versioning/). Vérifiées le 2026-11-06.

## 4. Identité et sessions

- **Versions :** protocole Kova **v1** ; ring `0.17` → **0.17.14**, sha2 `0.10` → **0.10.9**, hex `0.4` → **0.4.3**. Le serveur utilise rand `0.8` → **0.8.8** ; les autres versions de rand/sha2 du lockfile sont transitives.
- **Approche :** racine et appareil Ed25519 générés côté client ; autorisation de l'appareil signée par la racine, challenge contextualisé puis preuve signée par l'appareil. Identifiants dérivés par SHA-256. Conserver exactement les octets signés v1 et les vecteurs de test partagés.
- **Sessions :** Bearer desktop limité à une heure et lié à l'origine ; cookie historique limité à sept jours pour l'outil Python. L'échéance d'autorisation borne aussi la session. Empreintes des jetons côté base, rejet du mélange des transports, révocation locale et preuves conservées par la migration 0003.
- **Raison et alternative historique :** identité indépendante de l'hébergeur ; aucun secret privé reçu par le serveur. L'authentification par mot de passe a été retirée par 0002 : ne pas la réintroduire en copiant l'ancien modèle.
- **Limite :** Ed25519 assure ici l'authenticité ; aucun protocole E2EE de messages n'est choisi. Révocations propagées, journal signé et politique de fraîcheur restent à concevoir.
- **Références :** [signatures ring 0.17.14](https://docs.rs/ring/0.17.14/ring/signature/index.html), [contrat Kova](protocol.md), [modèle de menace](threat-model.md). Vérifiés le 2026-11-06 ; les règles Kova sont propres au projet, pas une recommandation de ring.

## 5. Cœur client : HTTP, cache et coffre

| Composant | Contrainte → version verrouillée | Approche existante et raison |
|---|---|---|
| HTTP | reqwest `0.12` → **0.12.28** | `default-features = false`, `blocking`, `json`, `rustls-tls`. Client réutilisé, aucune redirection, délai de 10 secondes ; HTTPS sauf boucle locale. API synchrone utilisée par le diagnostic ; intégration réseau à l'UI encore à définir pour ne pas bloquer son thread. |
| Cache | rusqlite `0.32` → **0.32.1** | SQLite embarqué via `bundled`, mode WAL, instances et brouillons persistants. Aucun secret dans le cache ; l'outbox et l'historique synchronisé ne sont pas encore livrés. |
| Coffre Windows | keyring `3` → **3.6.3** | Fonctionnalité `windows-native`, dépendance conditionnelle Windows ; clés privées et sessions dans le coffre natif, sans repli en fichier clair. Autres plateformes non implémentées pour ce coffre. |
| Effacement mémoire | zeroize `1` → **1.9.1** | Effacement explicite des buffers détenus par les structures d'identité et de session ; ne garantit pas l'absence de copies dans les bibliothèques ou le système. |

Références consultées le 2026-11-06 : [reqwest 0.12.28](https://docs.rs/reqwest/0.12.28/reqwest/), [rusqlite 0.32.1](https://docs.rs/rusqlite/0.32.1/rusqlite/), [keyring 3.6.3](https://docs.rs/keyring/3.6.3/keyring/). L'usage de zeroize est constaté dans le code ; sa documentation n'a pas été revalidée lors de cette passe. La validation native du coffre reste ouverte, ainsi que l'export/restauration chiffré et l'appairage.

## 6. Interface native Windows : choix de prototype

- **Versions :** GPUI épinglé à **`=0.2.2`**, fonctionnalités par défaut désactivées, `windows-manifest` activé ; unicode-segmentation `1` → **1.13.3**.
- **Approche :** Windows x86-64 en premier, cible MSVC pour la distribution prévue. Fenêtre native, liste virtualisée, éditeur Unicode adapté de l'exemple officiel, brouillon SQLite. Aucun frontend Vue/Tauri présent.
- **Raison :** évaluer le choix Rust/GPUI déjà retenu pour le MVP à travers un prototype mesurable, avant d'étendre l'interface.
- **Réserve explicite :** GPUI est pré-1.0 et sa documentation avertit de changements incompatibles fréquents ; elle mentionne encore macOS/Linux dans les prérequis. Ce choix existant est donc une évaluation bornée, pas une stack UI stable validée pour production. Conserver la version exacte et vérifier les exemples de cette version.
- **Preuves restantes :** compilation croisée GNU réussie selon le bilan existant ; exécution native, coffre, IME, presse-papiers, notifications et distribution restent à valider. GNU ne remplace pas la décision MSVC. Voir les prérequis C++/SDK dans [windows-spike.md](windows-spike.md).
- **Référence :** [GPUI 0.2.2](https://docs.rs/gpui/0.2.2/gpui/), vérifiée le 2026-11-06. Aucune nouvelle exécution native pendant cette passe documentaire.

## 7. Déploiement local et vérifications

- **Versions/configuration constatées :** Docker Compose (`docker compose`, version minimale non fixée), build `rust:1-bookworm`, exécution `debian:bookworm-slim` sous UID/GID `65534`. Images non épinglées par digest. Python **3.10+** pour le client de référence, **3.11** en CI, `cryptography>=41` sans version exacte verrouillée.
- **Approche :** API et PostgreSQL dans Compose, ports exposés uniquement en boucle locale, volume PostgreSQL persistant, mot de passe fourni par l'environnement. `depends_on: service_healthy` attend PostgreSQL ; les tests attendent aussi la sonde HTTP. Cette configuration sert au développement, pas de configuration HTTPS publique livrée.
- **Vérifications prévues :** `cargo fmt --all -- --check`, `cargo test --workspace --locked`, puis scripts Python `persistence.py`, `sessions.py`, `backup_restore.py` avec Docker. CI Windows : tests du cœur client, vérification desktop et ZIP non signé via `deployment/package-windows.ps1`.
- **Raison :** vérifier les contrats Rust et les transactions/persistance sur PostgreSQL réel. Python reste un outil temporaire de test et d'interopérabilité, pas le client cible.
- **Limites :** la présence d'un workflow ne prouve pas son passage. Aucun installeur signé, mécanisme de mise à jour, reverse proxy public ou support ARM64 validé. Les versions des outils/images demandent une décision de reproductibilité avant distribution.
- **Référence :** [ordre de démarrage Compose](https://docs.docker.com/compose/how-tos/startup-order/), vérifiée le 2026-11-06. Les versions Python et les jobs sont constatés dans le README et `.github/workflows/ci.yml`, sans revalidation du support de chaque outil.

## 8. Décisions encore ouvertes

- **E2EE :** bibliothèque/protocole à sélectionner après étude de maintenance, licences et audits ; ne pas déduire ce choix des primitives d'identité.
- **MP et réplication :** consentement bilatéral, authentification interserveurs, événements, outbox et reprise sont des conceptions dans [replication.md](replication.md), pas une implémentation approuvée à réutiliser telle quelle.
- **Distribution :** HTTPS public, secrets d'exploitation, signature, mises à jour, support des autres OS et ARM64 restent à décider/valider.
- **Licences :** orientation serveur `AGPL-3.0-only`, protocole `Apache-2.0` ; répartition restante et licence desktop à finaliser selon [la décision dédiée](decisions/licensing.md). Ce registre n'effectue pas de validation juridique.

## Maintenance du registre

- Consulter ce fichier avant une nouvelle décision ; réutiliser les choix documentés tant que leurs hypothèses restent valables.
- Distinguer contrainte déclarée, version verrouillée, vérification documentaire et preuve d'exécution. Ne pas présenter toutes les dépendances existantes comme les dernières versions recommandées.
- Revalider lors d'un changement majeur, d'une dépréciation, d'une alerte de sécurité ou d'un besoin invalidant le choix ; vérifier alors les notes de version et de migration officielles ciblées.
- Ajouter pour chaque choix significatif la version, l'approche, la raison, les alternatives pertinentes et la source datée ; remplacer les décisions dépassées sans dupliquer les contrats détaillés.
