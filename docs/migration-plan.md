# Alignement sur MVP_target v2 — audit du 6 novembre 2026

## Constat et périmètre

Le dépôt possède un serveur Axum/PostgreSQL, deux migrations SQLx, un outil Python de génération locale et des tests Compose. Aucun frontend Vue/Tauri ni `docs/schema.sql` n'était présent. Aucun client Rust, chat HTTP/WS ou MP n'est implémenté. La présence des tables communautaires ne prouve pas le jalon 3.

Cette passe transforme le socle en workspace, partage les formats et la vérification publique d'identité sans importer Axum/SQLx dans les bibliothèques, adapte Compose et les tests, et formalise les limites. Elle ne valide pas la livraison du MVP. Le client Python reste un outil d'interopérabilité et de test temporaire.

## État initial des jalons (avant la passe Windows)

| Jalon | État constaté | Preuve restante |
|---|---|---|
| 0 GPUI | Absent | Prototype fenêtre, liste virtualisée, saisie, presse-papiers, notifications et packaging ; Windows en premier, puis macOS/Linux sur machines réelles |
| 1 Workspace/stockage | Workspace créé ; migrations historiques conservées ; CI ajoutée | Passage CI, sauvegarde/restauration et ARM64 encore à prouver |
| 2 Identité | Challenge signé, profils locaux, révocation locale, client Python | Client Rust et coffre OS, jetons desktop courts, conservation des attestations, journal signé et politique de fraîcheur |
| 3 Communautés | Tables seulement | Transactions création+propriétaire+général, invitations HTTP, rôles, isolation et exclusion |
| 4 Chat public | Table seulement | REST/WS, séquences contiguës, cache SQLite, reprise et exclusion WS |
| 5–8 MP/distribution | Absents | Sélection E2EE, consentement bilatéral, sync, bascule et parcours sur deux desktops |

1. Valider le spike GPUI et la licence du projet avant d'ajouter une dépendance UI. Documenter chaque OS réellement testé, sans extrapoler la portabilité.
2. Ajouter `kova-client-core` avec identité locale, coffre protégé, sessions par origine et SQLite ; réutiliser `kova-protocol` et `kova-identity`. Créer les autres crates seulement lorsqu'elles portent une fonctionnalité testable.
3. Finaliser le contrat de sessions desktop et le stockage des preuves (voir protocole et migrations proposées), sans modifier silencieusement les octets signés v1.
4. Implémenter le jalon 3, puis le chat local. Aucune appartenance communautaire ne donnera de droit sur un MP.
5. Choisir une bibliothèque E2EE après étude maintenance/licence/audits, puis seulement concevoir et tester les événements et la réplication avec deux participants et trois instances dont une non autorisée.

## Déploiement

Compose utilise maintenant la racine du workspace comme contexte de build. Il reste un déploiement local sur boucle locale ; Caddy, HTTPS public, secrets d'exploitation, sauvegarde/restauration, limites de stockage et validation ARM64 sont à ajouter au jalon distribution. Le health endpoint vérifie PostgreSQL ; `--wait` de Compose ne garantit pas à lui seul la disponibilité HTTP de l'API. Les tests attendent explicitement `/health/ready`.

Les migrations 0001/0002 restent immuables pour préserver les checksums SQLx et les installations existantes. La migration 0002 supprime déjà les mots de passe et conserve les profils historiques sans identité jusqu'à rattachement hors bande. Aucun identifiant global ne doit être fabriqué pour ces profils.

## Validation de la première passe

- Formatage Rust et `cargo test --workspace --locked` via l'image `rust:1-bookworm` : succès ; tests de contrat v1 et vecteur Ed25519 RFC 8032, avec rejet de signature invalide.
- `python crates/kova-server/tests/persistence.py` : succès ; passage de la base historique, sessions invalidées, profil rattaché, arrêt PostgreSQL et persistance après recréation.
- `python crates/kova-server/tests/sessions.py` : succès ; signatures invalides/rejeu, deux appareils, révocation, invitation concurrente et même identité sur deux instances indépendantes avec profils distincts.
- Migrations déplacées comparées octet par octet aux originales : identiques. Syntaxe Python/TOML et `git diff --check` : succès.

Ces vérifications ont été exécutées localement avec Docker Linux x86-64. Elles ne valident ni un desktop GPUI, ni ARM64/macOS/Linux desktop, ni les fonctionnalités futures. Le compilateur signale un import inutilisé préexistant dans auth/mod.rs. La CI distante n'a pas été lancée dans cette passe.

## Passe Windows du 6 novembre 2026

Le choix Windows prioritaire est confirmé par le propriétaire. Ajouts : prototype GPUI 0.2.2, cœur client Rust avec signatures/contextes, HTTP, SQLite, coffre Windows et client de diagnostic ; sessions Bearer à une heure, transport lié au challenge signé, rejet des authentifications ambiguës ; migration 0003 pour conserver les attestations et preuves de révocation. La CI comporte désormais un job Windows et un paquet ZIP de développement non signé. La licence reste à choisir dans `decisions/license-comparison.md`.

La validation native est bloquée par l'absence des outils C++ et SDK Windows. L'installation silencieuse nécessite une élévation administrateur indisponible (code 5007). Voir `windows-spike.md` pour lever ce blocage et effectuer les essais. Le prototype n'est pas encore un client MVP complet.

Avant le jalon 3 restent : validation native du prototype et du coffre Windows, notifications, preuve de packaging, intégration de l'interface d'identité/connexion, sauvegarde/restauration chiffrée et appairage, journal d'état signé et politique de fraîcheur concrète. Les attestations conservées ne remplacent pas ces derniers mécanismes. Le renommage des tables historiques est une migration future ; `users` désigne déjà des profils locaux, aucun mot de passe ne subsiste après 0002.

Les fonctionnalités MP, outbox, E2EE et réplication restent dans leurs jalons futurs ; elles ne sont pas déclarées implémentées à travers ce travail de préparation.

### Preuves de la passe Windows

`cargo fmt --all -- --check` et `cargo test --workspace --locked` passent (Linux). Les tests HTTP couvrent maintenant le contexte desktop signé, l'absence de cookie, la limite d'une heure, le mélange de transports, les en-têtes ambigus, logout, la révocation d'un Bearer actif et les preuves conservées. `persistence.py`, `sessions.py` et `backup_restore.py` passent : migration 0001→0003, redémarrage, deux instances et restauration complète dans une base distincte. La restauration de sauvegarde du jalon 1 dispose donc maintenant d'une preuve locale.

GPUI est vérifié et construit pour Windows GNU en compilation croisée. L'exécution graphique et le coffre natif restent non vérifiés à cause du moteur Computer Use indisponible ; MSVC manque toujours de ses prérequis. Voir `windows-spike.md` pour les résultats exacts. Les jalons 0/2 ne sont pas déclarés clos ; aucun travail communautaire du jalon 3 n'a été introduit.

## Finalisation du jalon 2 — 7 novembre 2026

**Jalon 2 terminé pour la cible Windows du MVP.** Les résultats ci-dessous remplacent les réserves historiques relatives à l'identité ; ils ne clôturent pas les autres jalons.

Le parcours Windows comprend maintenant création et consultation d'identité, connexion/invitation, liste des appareils, déconnexion, sauvegarde age de la racine et restauration dans un profil vide. L'association par fichiers publics vérifie l'empreinte de la demande et la racine attendue ; l'appareil associé ne reçoit jamais la clé racine. Les commandes de coffre, chiffrement et réseau s'exécutent hors du thread d'interface. Voir [le guide utilisateur](windows-identity.md).

Les autorisations v2 signent l'intégralité de leur intervalle de validité, limité à 24 heures. La migration 0004 invalide les sessions historiques et conserve un journal de révocations racine signées. Le client vérifie et conserve l'union des preuves, puis la transmet aux instances connues, avec échecs visibles et réessai explicite. Aucun serveur central ni propagation instantanée n'est supposé.

Les quatre suites PostgreSQL ont été exécutées localement avec Docker : `persistence.py`, `sessions.py`, `revocations.py` et `backup_restore.py` réussissent. Elles couvrent la migration 0001→0004, la conservation des données après recréation, les signatures invalides/rejeux, les sessions par appareil et instance, l'invitation concurrente, la révocation retardée entre deux instances, le rejet des certificats v1/expirés/trop longs et la restauration indépendante d'une sauvegarde SQL.

`cargo fmt --all -- --check` et `cargo test --workspace --locked` réussissent nativement sous Windows MSVC (12 tests). Les tests utilisent réellement Credential Manager dans des profils de test distincts : association/renouvellement, sessions isolées par origine, restauration de racine avec nouvel appareil et refus d'écrasement. Les entrées de test sont supprimées en fin de test. La sauvegarde refuse aussi mot de passe incorrect et corruption ; l'association rejette la substitution de demande/racine/signature et l'expiration.

`cargo build -p kova-desktop --locked` réussit également. Contrôle graphique automatisé avec Computer Use : ouverture/fermeture de la fenêtre, navigation Identité/Sauvegarde/Connexion, saisie Unicode masquée et retour d'erreur de restauration sans fichier avec effacement du champ secret. Ce contrôle est un smoke test visuel ; les parcours cryptographiques/coffre et HTTP sont validés par les suites ci-dessus, pas par une recette graphique exhaustive. Aucun secret du profil utilisateur n'a été créé ou modifié. L'avertissement de compatibilité future de `proc-macro-error2 2.0.1` reste présent dans les dépendances ; la CI distante n'a pas été exécutée.

Les limites restent explicites : association manuelle sans QR, renouvellement via la racine toutes les 24 heures au maximum, révocations transmises aux seules instances connues. Le jalon 0 conserve ses travaux de notifications et de packaging ; les communautés, le chat, l'E2EE et la distribution signée relèvent des jalons suivants.

## Finalisation du jalon 3 — 7 novembre 2026

**Jalon 3 terminé pour les communautés locales du MVP.** Le serveur fournit création atomique communauté/propriétaire/salon `général`, consultation des communautés/salons/membres, création de salons, invitations HTTP à usage unique, attribution des rôles et exclusion persistante. La migration 0005 conserve les exclusions par profil local ; les migrations historiques restent inchangées. La matrice de droits et les transactions sont consignées dans `tech-stack.md` et `protocol.md`.

Le cœur Rust et le prototype Windows proposent les opérations dans **Communautés** et **Modération**, en utilisant la session conservée dans le coffre et l'exécuteur de fond existants. L'adhésion initiale réutilise le challenge signé ; une session active dispose aussi de `/invites/redeem`. Voir [le guide Windows](windows-communities.md). Aucun accès administrateur implicite, droit MP ou fonctionnalité de chat n'est ajouté.

Preuves de validation : `communities.py` sur PostgreSQL 17/Docker (échec injecté du dernier INSERT et rollback complet, noms et permissions, origine PATCH/DELETE, invitation expirée et consommation concurrente unique, exclusion concurrente à une adhésion, refus de réadhésion, autres communautés intactes et persistance après redémarrage). `persistence.py` valide 0001→0005 ; `sessions.py` passe. `cargo test --workspace --locked` passe nativement sous Windows MSVC (14 tests), ainsi que le build desktop. Vérification Computer Use : ouverture, affichage et navigation des deux nouveaux panneaux, boutons visibles, fermeture normale. Ce contrôle visuel n'est pas une recette graphique connectée exhaustive.

La compilation a rencontré un disque C: plein ; le cache du projet a été déplacé vers `D:\CodexBuilds\kova-milestone3-cache`, puis le build a réussi. Le binaire est également disponible dans `target/debug/kova-desktop.exe`. L'avertissement préexistant de compatibilité future `proc-macro-error2 2.0.1` demeure. Les suites révocation/restauration passent après récupération du moteur Docker ; la restauration vérifie aussi les exclusions, les salons et l’absence d’adhésion du profil exclu.

Limites : interface de prototype avec identifiants copiés depuis les résultats, pas de transfert de propriété ou de levée d'exclusion ; pas de cache communautaire hors ligne ni d'abonnement WS (jalon 4). La CI inclut désormais la suite communautaire ; aucune exécution distante n'est revendiquée.


## Implémentation du jalon 4 — 8 novembre 2026

Le chat public dispose de routes HTTP d'envoi idempotent et d'historique paginé, ainsi que d'un flux WebSocket reprenant après une séquence locale. Le cœur Rust conserve un cache SQLite contigu et les envois incertains. Le panneau Windows Chat propose réception en direct, synchronisation, envoi, réessai et lecture hors ligne. Voir [le guide](windows-chat.md) et [le contrat](protocol.md#chat-public--jalon-4).

La migration 0006 préserve les messages historiques, recale les compteurs et refuse les imports à trous. Les limites sont explicites : messages publics en clair sur l'instance et dans le cache, pas d'édition/suppression ni de pièces jointes, un salon suivi à la fois, 100 derniers messages affichés. Le polling PostgreSQL par connexion et l'absence de quotas de connexions ne constituent pas une preuve de montée en charge. Les résultats de validation et la clôture restent à consigner après intégration finale.

### Validation du jalon 4 — 8 novembre 2026

Jalon 4 terminé pour le chat public du prototype Windows. `cargo test --workspace --locked`, `cargo build -p kova-desktop --locked`, le formatage et `git diff --check` passent. Les suites PostgreSQL `chat.py` (avec `KOVA_TEST_RUST_CLIENT=1`), `persistence.py`, `communities.py` et `sessions.py` passent localement. La suite chat vérifie concurrence, rollback sans trou, idempotence, pagination, deux abonnés, reprise, exclusion, expiration, logout, révocation, redémarrage et lots JSON de plus de 2,4 Mo. Le client Rust vérifie aussi un accusé perdu, un redémarrage du cache et une reprise WS sans doublon.

Contrôle graphique Windows : ouverture, navigation vers Chat, présence des champs et des six commandes, fermeture normale. Il s'agit d'un smoke test visuel ; le parcours réseau est vérifié par les tests automatisés, pas par une recette graphique connectée exhaustive. L'avertissement préexistant `proc-macro-error2 2.0.1` demeure. La CI distante et les suites révocation/restauration complètes n'ont pas été relancées pour ce jalon. Les limites du prototype figurent dans `windows-chat.md`.
