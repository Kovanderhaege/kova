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
