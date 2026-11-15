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

## Jalon 5 — conversations privées 1:1

Implémentation : OpenMLS 0.8.1 dans un crate client indépendant, clés E2EE distinctes de l'identité, bundles et événements signés, vérification explicite des empreintes et du roster, relais HTTP opaque et migration 0007. Le panneau Windows propose création, acceptation, envoi, synchronisation, réessai et consultation hors ligne. Le cache privé et l'état MLS sont chiffrés avec age ; sa clé reste dans le coffre Windows.

Les tests Rust vérifient notamment l'altération, le rejeu, la substitution de clé MLS et le refus d'un mauvais coffre sans consommer l'état cryptographique valide. Le scénario `dm.py` avec `KOVA_TEST_RUST_CLIENT=1` a réussi : destinataire hors ligne avant création, réception après reprise, envoi en attente conservé après redémarrage, réessai sans doublon, vérification d'appareil et absence des textes testés dans les données serveur et SQLite. Les contrôles HTTP couvrent signatures, accès par appareil, consommation unique des packages, concurrence, pagination, dépendances, révocation et persistance.

Limites du prototype : un appareil par participant et conversation ; attestations initiales de 24 heures maximum, puis nouvelle conversation avec preuves renouvelées ; synchronisation manuelle ; pas de transfert/récupération d'historique, de protection anti-rollback du disque ou de suppression physique garantie des anciennes clés. La réplication et la bascule restent aux jalons 6/7. Voir `windows-dm.md` et `decisions/e2ee.md`. Aucun audit de l'intégration Kova ni passage de CI distante n'est revendiqué.

### Validation et clôture du jalon 5 — 8 novembre 2026

`cargo test --workspace --locked`, `cargo build -p kova-desktop --locked`, `cargo fmt --all -- --check` et `git diff --check` réussissent. Les suites PostgreSQL `dm.py` (avec le véritable client Rust), `persistence.py`, `sessions.py` et `backup_restore.py` réussissent ; la transition historique couvre maintenant 0001→0007. Le test de migration a été adapté au septième numéro sans modifier les migrations antérieures. L'avertissement transitif préexistant `proc-macro-error2 2.0.1` demeure.

Le desktop compilé s'ouvre et affiche l'entrée MP chiffrés. La tentative de navigation automatisée a échoué dans l'outil Computer Use (`failed to activate captured window`) : seule l'ouverture a été observée, pas une recette graphique connectée. Le parcours réseau et cryptographique est validé par les tests Rust/HTTP ci-dessus. Le jalon 5 est livré pour ce périmètre de prototype ; les limites fonctionnelles et de sécurité restent celles décrites ci-dessus.

## Finalisation du jalon 6 — 9 novembre 2026

**Jalon 6 terminé pour la réplication bilatérale des MP du prototype.** Les deux participants signent une politique versionnée avant son activation. Les serveurs épinglent leurs clés de fédération et authentifient les requêtes et réponses. La migration 0008 conserve politiques, nonces, conflits et curseurs de reprise ; les migrations antérieures restent inchangées. La synchronisation paginée déduplique les événements sans déchiffrer leur contenu et refuse les instances non autorisées. Le client conserve les politiques et les curseurs par origine dans son cache chiffré ; le panneau Windows permet proposition, examen, approbation, retrait et synchronisation explicites.

Validation locale sous Windows MSVC : `cargo test --workspace --locked` réussit (25 tests unitaires), ainsi que les builds serveur et desktop, le formatage et `git diff --check`. Les huit suites PostgreSQL/Docker `sessions.py`, `persistence.py`, `communities.py`, `chat.py`, `dm.py`, `replication.py`, `revocations.py` et `backup_restore.py` réussissent. Les trois tests réseau Rust ignorés par défaut sont exécutés par les suites chat, MP et réplication avec `KOVA_TEST_RUST_CLIENT=1`.

Le scénario de réplication utilise trois serveurs et des bases isolées : consentement des deux participants, convergence A/B, refus de C, pagination, rejeux et signatures altérées, doublons, dépendances manquantes, arrêt/reprise, retrait répété, révocation et persistance après redémarrage. Le véritable client MLS échange des messages via les deux origines. Clippy termine sans erreur ; des avertissements préexistants et l'avertissement transitif de compatibilité future `proc-macro-error2 2.0.1` demeurent. La CI distante, le déploiement HTTPS public et une recette graphique connectée exhaustive n'ont pas été exécutés.

Limites conservées : synchronisation manuelle, roster de deux appareils et attestations initiales de 24 heures ; huit réplicas, 64 versions de politique, dix événements par échange, corps de 2 Mio et 64 lots par action client. Un retrait ne garantit pas l'effacement des copies anciennes et une partition retarde les révocations. La bascule automatique relève du jalon 7. Voir [le contrat de réplication](replication.md) et [le parcours Windows](windows-dm.md).


## Finalisation du jalon 7 — 15 novembre 2026

Le jalon 7 livre la sélection automatique d'un réplica disponible parmi les origines bilatéralement approuvées. Chaque origine conserve sa session ; le client vérifie la clé de fédération et la chaîne de politiques. Seules les pannes de transport, erreurs HTTP 5xx et sessions locales absentes/expirées permettent un repli. Les refus d'accès, mauvaises identités, clés inattendues et conflits restent bloquants.

Le desktop synchronise et reprend les événements déjà préparés de la conversation sélectionnée toutes les 30 secondes, avec délai progressif jusqu'à cinq minutes après échec. Les opérations restent hors thread UI et sérialisées. Hors ligne et l'examen d'une politique suspendent ce travail. Le panneau défile pour rendre toutes les commandes accessibles.

Deux cas de reprise sont traités : une création dont l'accusé est perdu renvoie le même manifeste après réouverture du cache, sans recréer le groupe MLS ; un réplica en retard reçoit les ancêtres signés par l'appareil local avant le message en attente. Les identifiants et ciphertexts sont conservés. Si un ancêtre du correspondant manque et qu'aucune source disponible ne le possède, l'envoi reste en attente jusqu'au retour de la réplication.

Validation finale locale sous Windows MSVC : `cargo test --workspace --locked` réussit (28 tests unitaires, dont perte d'accusé de création et reprise du cache) ; `cargo fmt --all -- --check`, `git diff --check`, la compilation Python et `cargo clippy --workspace --locked` réussissent. Clippy conserve des avertissements de style, ainsi que l'avertissement transitif de compatibilité future `proc-macro-error2 2.0.1`. Les builds serveur/desktop ont réussi. La suite `dm.py` avec client Rust a réussi lors de l'implémentation ; la suite finale `replication.py` avec client Rust réussit après relance : arrêt réel de A, bascule vers B, parent local absent de B, outbox après réouverture, retour de A, convergence et unicité vérifiées côté serveur, déchiffrement par Bob, refus de C et des sessions étrangères/invalides. Une première relance finale a échoué avant bascule sur un dossier temporaire de coordination introuvable ; ce défaut de test n'a pas été reproduit à la relance. La CI distante et les suites non affectées n'ont pas été relancées.

Contrôle visuel du desktop compilé : ouverture, navigation MP, défilement donnant accès à toutes les commandes, fermeture normale. Ce contrôle ne remplace pas une recette graphique connectée exhaustive. Les limites du prototype demeurent : sessions préalables sur les réplicas, politique/manifeste déjà répliqués, attestations de 24 heures, synchronisation limitée au panneau ouvert, absence de service permanent et de garantie instantanée pendant une partition. Voir [le contrat](replication.md) et [le guide Windows](windows-dm.md).

## Distribution du jalon 8 — 15 novembre 2026

Le jalon 8 livre la distribution de développement pour le périmètre Windows du prototype : Compose de production autonome avec Caddy 2.11.9, HTTPS, secrets fichiers et volumes persistants ; outil de sauvegarde PostgreSQL custom et restauration transactionnelle dans une nouvelle base ; paquet Windows avec licences, guides, inventaire et SHA-256. L'API et PostgreSQL n'exposent pas de port hôte. Les limites mémoire/processus et la rotation des journaux sont explicites ; les quotas de disque restent à configurer sur l'hôte. Aucun installeur signé ou service de mise à jour n'est revendiqué. Voir [le guide d'exploitation et de recette](distribution.md).

Validation native Windows MSVC : `cargo test --workspace --locked` réussit (32 tests unitaires ; les tests réseau/fixture restent ignorés par défaut), ainsi que les builds serveur/desktop. Le formatage, Clippy et `git diff --check` ont réussi ; les avertissements de style préexistants et l'avertissement transitif `proc-macro-error2 2.0.1` demeurent. Les six tests Python de sauvegarde et le test PowerShell de packaging passent : profils Debug/Release, échec de build, licences et sommes vérifiées ; les faux exécutables restent dans un fixture temporaire, hors des ZIP publiés par la CI. Un véritable ZIP Debug a été produit, extrait et exécuté ; son exécutable correspond par SHA-256 au build MSVC testé. La production locale d'un ZIP Release n'est pas revendiquée.

Le scénario automatisé `replication.py` avec `KOVA_TEST_RUST_CLIENT=1` réussit désormais avec trois projets PostgreSQL indépendants, chacun possédant son volume, secret et port. Il vérifie chat public Alice/Bob sur A, isolation des communautés, véritables MP MLS, approbation bilatérale, refus de C, panne de A, outbox après réouverture, reprise et convergence. Les suites existantes `sessions.py` et `backup_restore.py` réussissent également. Le test `distribution.py` a vérifié HTTPS local avec validation du nom et de la chaîne du certificat Caddy, redirection HTTP, refus d'Origin hostile, absence du secret dans l'environnement API, redémarrage et restauration indépendante avec conservation des checksums de migration ; les tentatives d'écrasement sont refusées.

Recette graphique réellement observée : deux processus du desktop extrait du ZIP, sur la même session Windows, utilisent deux profils synthétiques distincts et deux serveurs natifs avec PostgreSQL indépendants. La fixture `desktop_recipe.py` et son test Rust ignoré préparent identités, sessions, communauté, MP et consentement pour B. Depuis les fenêtres : Alice envoie un message public sur A, Bob le synchronise et le lit ; le MP initial est déchiffré ; A est arrêté (connexion à la sonde refusée), Alice envoie un MP transmis via B, Bob le déchiffre et répond via B, Alice déchiffre la réponse. Après redémarrage de A, la reprise depuis l'interface réplique les événements : trois événements distincts, mêmes identifiants dans A et B, historique local conservé et aucun doublon. Les deux fenêtres sont fermées normalement et la fixture supprime ses conteneurs/volumes temporaires.

La création initiale des identités et les approbations de cette recette sont automatisées par la fixture, pas cliquées dans l'interface. La reprise d'un envoi en attente après redémarrage du cache est vérifiée par le test Rust réseau, pas par une fermeture/réouverture graphique avec un message en attente. Les profils de recette nommés dans `target/desktop-recipe-…/state.json` restent dans Credential Manager et `%LOCALAPPDATA%/Kova/profiles` pour inspection ; le profil `default` n'a pas été utilisé. Cette preuve locale ne couvre ni deux machines physiques, ni Windows vierge, ni un certificat ACME public, ni ARM64/macOS/Linux desktop. La CI distante n'a pas été exécutée. Les limites cryptographiques et de disponibilité des jalons précédents restent applicables.

## Préparation du jalon 9 — 15 novembre 2026

Le [périmètre de publication](release-scope.md) consolide les preuves des jalons
0–8 et les plateformes réellement exercées. Les notifications sont affectées au
12, la disponibilité publique et l'installation guidée au 13, et la recette
finale de publication au 14. Ces reports ne valent pas validation des fonctions.

Les six manifestes et textes de licence concordent. La [revue de
distribution](license-review.md) documente les droits, dépendances, sources et
limites de preuve. Le packaging ajoute les textes tiers, un inventaire avec
empreintes/provenance et une archive des sources Kova/Cargo verrouillées.
Les suppléments conservent les octets originaux ; les copyrights absents ne
sont pas inventés et restent signalés. Les nouvelles contributions ont une
politique explicite. Aucune répartition de licence des six crates n'est rouverte.

La CI distante du commit `b8284667efd1e4ebf6c9d813c3b786ba756fb5f5` a été
inspectée : [run 43735416464](https://github.com/Kovanderhaege/kova/actions/runs/43735416464),
job Linux `verify` réussi (toutes les suites PostgreSQL incluses), job Windows
en échec après le test PowerShell qui annonçait pourtant son succès. Le mock
d'échec conservait `LASTEXITCODE=17` ; le harnais termine désormais à zéro
uniquement lorsque ses assertions réussissent. Les erreurs réelles, l'absence
de notices, les empreintes altérées et les sources modifiées pendant le build
restent bloquantes. Les actions Node 20 signalées par ce run passent à v6/Node24,
et la CI doit conserver les sources et notices de la nouvelle révision.

Validation locale : `cargo test --workspace --locked`, `cargo fmt --all --
--check` et `cargo clippy --workspace --locked` réussissent sous Windows MSVC
(Rust 1.99.1). Les avertissements et leur impact sont documentés dans le
périmètre de publication. Les 16 tests Python de sauvegarde/conformité et le
test PowerShell (Debug/Release, erreurs, mutation des sources, contenu et sommes
récursives) passent, y compris le wrapper de processus utilisé par GitHub Actions.
La lecture effective des matériaux de licence valide 780 paquets tiers et
1 604 fichiers de notices/provenance. Les suites PostgreSQL non affectées ne
sont pas réexécutées localement ; leur succès distant ci-dessus appartient à
la révision précédente.

Un véritable ZIP Debug de 142 790 354 octets a été assemblé sur cette branche de
travail. Ses 1 622 fichiers sont rapprochés du manifeste SHA-256, et l'empreinte
du desktop correspond au build MSVC. L'archive source a été extraite : toutes
les empreintes des fichiers Kova correspondent à l'inventaire ; `cargo metadata
--offline --locked --format-version 1` réussit avec un `CARGO_HOME` vide et
retrouve les 786 paquets du workspace depuis les sources fournies. Cela valide
la résolution hors ligne, pas une recompilation indépendante ni un build
reproductible. Le paquet déclare honnêtement sa base `b828466` et son arbre
modifié ; la CI doit produire les artefacts de la révision retenue propre.
Les suppléments distinguent 66 révisions exactes, sept révisions ultérieures et
neuf références canoniques associées aux archives crates.io contrôlées. Douze
paquets sans avis concret de copyright restent signalés dans l'inventaire ;
aucun titulaire ni année n'a été déduit des noms d'auteurs.

**État de la préparation avant le push :** une nouvelle CI Linux et Windows réussie sur la
révision retenue doit être obtenue avant clôture. Aucun push n'est effectué
sans demande explicite, conformément à `AGENTS.md`.

Le propriétaire a autorisé le push de la branche renommée `milestone-9`.
La vérification de l'artefact serveur du [run 43740561215](https://github.com/Kovanderhaege/kova/actions/runs/43740561215)
sur `2e29acd` a détecté l'omission des fichiers de provenance cachés par
`upload-artifact` (notamment `.cargo_vcs_info.json`). L'upload du seul dossier
de matériaux générés inclut désormais ces fichiers ; aucun secret de profil
ou d'environnement n'est ajouté. L'inventaire exige leur conservation et la
nouvelle exécution doit être vérifiée avant clôture.

Sur la révision `1215179c48d0d65f7516e534a49892bd7cfd30d9`, le job Linux du
[run 43741114553](https://github.com/Kovanderhaege/kova/actions/runs/43741114553)
réussit, y compris toutes les suites PostgreSQL. L'artefact corrigé
`server-source-and-notices` (ID `14172790503`, SHA-256
`c8e4a0efba4123141125aa3e9ded1244d37c9ece4a617deb24ef8f93c59fa686`)
a été téléchargé et vérifié : révision exacte, arbre propre, six licences Kova
conformes aux manifestes, 780 dépendances, 1 604 notices/provenances et leurs
empreintes, 420 fichiers source Kova et leurs empreintes, 780 sources vendored
avec checksums Cargo et configuration relative `vendor`. Tous les fichiers
référencés, y compris les fichiers cachés, sont présents.

## Clôture du jalon 9 — 15 novembre 2026

**Jalon 9 terminé pour le périmètre du prototype**, sur la révision technique
retenue `1215179c48d0d65f7516e534a49892bd7cfd30d9`. Le [run 43741114553](https://github.com/Kovanderhaege/kova/actions/runs/43741114553)
est terminé avec succès : job Linux `verify` et job `windows`, incluant toutes
les suites serveur, les tests du cœur client, le contrôle desktop, les tests
Python/PowerShell et la production effective du paquet Windows Release.

L'artefact `windows-development-unsigned` (ID `14172469432`, SHA-256
`a5768777e3c7546a0a1310da80c3bfc83425e39796e8029d46ff7bc787701e0c`)
a été téléchargé. Le ZIP embarqué contient les deux exécutables PE Windows
x86-64 attendus, une configuration `Release`, la révision exacte et un arbre
propre. Ses 1 622 fichiers correspondent à `SHA256SUMS`, avec correspondance
des 1 604 notices/provenances à l'inventaire. Les licences des six crates,
les empreintes des 420 fichiers Kova dans l'archive source, les 780 sources
Cargo avec checksums et la configuration relative `vendor` sont vérifiées.
Le matériel serveur corrigé et vérifié est décrit ci-dessus ; les deux
artefacts appartiennent à cette même révision. Le contrôle Release porte sur
le build, le format et le contenu distribués, pas sur une nouvelle recette
graphique ni un essai sur Windows vierge.

Le commit de clôture ne change que le bilan et le statut documentaire, en
conservant ce code et ces artefacts validés. Les limites, avertissements et
travaux affectés aux jalons 10–14 restent ceux de [release-scope.md](release-scope.md).
Le ZIP demeure non signé, les copyrights absents sont signalés, l'hébergement
durable des sources avant publication reste à organiser et aucun audit
cryptographique ou juridique indépendant n'est revendiqué.


## Finalisation du jalon 10 — 15 novembre 2026

**Jalon 10 terminé pour le périmètre Windows du MVP.** Le manifeste et ses deux appareils restent immuables ; l'état d'autorisation public est renouvelable sans changer le `conversation_id`, le Welcome, l'état MLS, l'historique ou l'outbox. Le cœur persiste la preuve avant publication et la republie lors de la reprise ; l'interface dispose d'une action dédiée. Un appareil associé conserve ses droits limités et reçoit son certificat renouvelé via son appareil racine. Voir [le contrat](dm-renewal.md) et [le guide Windows](windows-dm.md).

La migration additive 0009 conserve toutes les enveloppes existantes. Les nouveaux dépôts et consentements exigent les deux autorisations courantes fraîches, toujours limitées à 24 h. Les réplicas échangent les renouvellements et révocations même après expiration ; les signatures historiques se vérifient indépendamment de cette fraîcheur. Le contrôle valide est committé avant les lots d'événements, avec relecture de la politique après changement de transaction. Les révocations reçues alimentent aussi le journal global de l'instance et les conversations locales du client concernées, sans restaurer un appareil révoqué.

Preuves exécutées localement :

- Windows MSVC / Rust 1.99.1 : `cargo test --workspace --locked`, **42 tests réussis** ; `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked`, `cargo build -p kova-server --locked` et `cargo build -p kova-desktop --locked` réussissent. Les nouveaux tests couvrent plusieurs échéances de 24 h à horloge contrôlée, expiration hors ligne sans mutation, certificat invalide/futur ou trop long, substitution de clés/package, renouvellements désordonnés, priorité de révocation, appareil associé sans racine, et conservation exacte manifeste/état MLS/UUID/ciphertext d'outbox après redémarrage. Les tests live marqués ignorés dans la commande workspace sont exécutés séparément par les deux harness ci-dessous pour les MP et la fédération.
- `KOVA_TEST_RUST_CLIENT=1 python crates/kova-server/tests/replication.py` : succès avec trois serveurs natifs et trois PostgreSQL Docker isolés. Mise à niveau d’une base au schéma 0008 contenant un manifeste et 29 événements, sans modification de leurs octets ; expiration réelle avec preuves de quatre secondes, rejet avant renouvellement, concurrence des renouvellements sans retour arrière, rattrapage historique pendant expiration, B en retard, reprise exacte de l'événement en attente, contrôle de révocation dans les deux sens et après redémarrage, refus de nouveaux dépôts et invalidation de session du dispositif révoqué, accès historique du participant restant, refus de C. Le test Rust MLS Alice/Bob avec panne A, bascule B et convergence passe également.
- `KOVA_TEST_RUST_CLIENT=1 python crates/kova-server/tests/dm.py` : succès sur serveur Linux construit en Release dans Docker ; le test Rust live renouvelle les preuves des deux côtés pendant une outbox persistante, poursuit le même fil après redémarrage et déchiffre les messages une seule fois.
- `persistence.py` et `backup_restore.py` : succès, migrations 0001→0009, conservation du message historique après recréation et restauration indépendante d'une sauvegarde SQL avec checksums conservés. Les migrations 0001–0008 restent inchangées. Syntaxe Python et `git diff --check` vérifiés.

Le premier build natif Debug a échoué par manque d'espace disque ; le cache Cargo Debug a été nettoyé, puis les validations finales ont réussi avec le target sur D:, `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0` et `CARGO_PROFILE_TEST_DEBUG=0`. Aucun changement de dépendance, de toolchain ou de licence. Clippy conserve les avertissements de style préexistants et d'organisation des modules de tests ; `proc-macro-error2 2.0.1` conserve l'avertissement de compatibilité future déjà documenté. Le test de transport HTTP existant vérifie désormais qu'il reçoit effectivement des octets, correction minimale du lint bloquant découvert avec `--all-targets`.

Limites : renouvellement explicite par chaque participant et reconnexion si la session HTTP a expiré ; intervention de la racine pour un appareil associé ; un appareil par participant, aucune récupération d'état MLS perdu ni effacement des copies. Les événements v1 n'attestent pas l'heure de création : une outbox est admise sous les droits courants. Une partition peut cacher une révocation. Mettre à jour clients et réplicas ensemble. Pas de nouvelle CI distante, de recette graphique exhaustive ni d'essai réellement étalé sur plusieurs jours dans ce jalon ; la recette graphique et de distribution de plusieurs jours reste au jalon 14.
