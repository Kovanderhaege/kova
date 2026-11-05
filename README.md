# Kova — architecture du MVP

Alice lance une instance, crée la communauté « Atelier » et invite Bob. Chacun ouvre le salon général dans son navigateur. Bob voit le message d’Alice sans actualiser la page et le retrouve après un redémarrage. Un membre d’une autre communauté ne peut ni lire l’historique ni écouter ce salon. Ce parcours constitue le premier critère de livraison.

## Unité de déploiement

Une instance correspond à une installation de Kova, avec sa propre origine HTTPS, sa configuration et sa base PostgreSQL. Un compte appartient à cette instance et peut rejoindre plusieurs communautés. La communauté est la frontière de permissions. Tous ses salons textuels sont accessibles à ses membres. Deux installations peuvent fonctionner indépendamment, y compris si elles utilisent les mêmes pseudonymes.

Le navigateur charge Vue et adresse REST et WebSocket sur la même origine. En développement, Vite relaiera les deux flux à Axum. En déploiement, Caddy terminera TLS et transmettra les requêtes au processus Rust, qui servira aussi les fichiers Vue compilés. Un seul processus Tokio détient le pool SQLx et les abonnements WebSocket en mémoire. PostgreSQL conserve les écritures validées. Une panne du processus coupe les connexions mais préserve l’historique.

## Monorepo cible

Les deux fichiers fournis avec cette proposition sont [le schéma SQL](docs/schema.sql) et [le Compose de développement](deployment/compose.yaml). Les autres chemins ci-dessous seront créés au jalon concerné.

```text
backend/
  Cargo.toml
  migrations/              migrations SQLx versionnées
  src/
    main.rs                configuration, pool, migrations, écoute et arrêt
    http.rs                routeur, limites, erreurs JSON et origine autorisée
    auth/                  comptes, sessions, extracteur de session
    communities/           communautés, membres, invitations et permissions
    channels/              création et recherche des salons
    messages/              envoi idempotent, ordre et historique
    realtime/              abonnements, diffusion, reprise
  tests/                   tests HTTP et WS contre PostgreSQL
frontend/
  src/
    views/                 connexion, inscription, communauté, invitation
    components/            navigation, historique, éditeur, membres
    api/                   DTO TypeScript et client HTTP
    stores/                session et communauté active
    realtime/              connexion WS et synchronisation des salons
deployment/
  compose.yaml
docs/
  schema.sql
  decisions/               décisions acceptées et raisons de les réviser
```

Un handler Axum désérialise un DTO, obtient l’identité depuis la session et appelle une opération métier. L’opération contrôle les droits et délimite la transaction SQLx. Les structs de réponse sélectionnent explicitement les champs exposés. Les types contenant des empreintes ou des secrets restent internes. Chaque module garde ses requêtes SQL près de ses opérations. Le module temps réel reçoit un événement après validation de la transaction.

## Données et invariants

Le schéma joint contient les colonnes, contraintes et index proposés. Une installation ayant une seule instance, sa configuration relève d’abord de l’environnement. La création d’une communauté insère dans une même transaction la communauté, l’adhésion du propriétaire et le salon général. La clé étrangère différée impose au propriétaire d’être membre à la validation. Le rôle de propriétaire se déduit de `owner_id`.

Un message a pour identité le couple `(channel_id, seq)`. Pour envoyer, verrouiller la ligne du salon, vérifier l’adhésion, rechercher la clé de répétition puis, pour un nouvel envoi, incrémenter `last_seq` et insérer le message dans la même transaction. Deux auteurs concurrents obtiennent ainsi des numéros dans l’ordre de validation du salon. Le client peut reprendre après un numéro sans perdre un message validé tardivement. Ce choix sérialise les écritures d’un salon et devra être mesuré sous charge. [Verrous de ligne PostgreSQL](https://www.postgresql.org/docs/17/explicit-locking.html)

Les migrations seront appliquées avant l’ouverture du port HTTP. Tester chaque migration sur base vide et, après la première version, depuis la version publiée précédente. Avant mise à jour d’une instance, sauvegarder puis vérifier la restauration dans une base séparée.

## Identité et droits

Pour le pilote, l’exploitant crée le premier compte via une commande locale interactive. Un utilisateur connecté peut créer une communauté. Son propriétaire crée des invitations à usage unique, valables 24 h. Le lien place le jeton dans le fragment URL. Vue le retire de la barre d’adresse et le transmet dans le corps JSON. Le serveur stocke l’empreinte SHA-256 du jeton aléatoire de 32 octets. L’inscription crée le compte et l’adhésion dans la transaction qui consomme l’invitation. Un compte existant peut simplement rejoindre. Deux consommations concurrentes ne réussissent qu’une fois.

À la connexion, vérifier le mot de passe avec Argon2id et un sel individuel. Point de départ proposé pour le pilote : 19 Mio, deux passes, parallélisme 1, à calibrer sur la machine cible. Borner les calculs simultanés et limiter les tentatives par compte et adresse. Utiliser une erreur générique pour les identifiants invalides. [Paramètres Argon2id recommandés par OWASP](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html)

Une connexion ou inscription réussie génère un nouveau jeton aléatoire de 32 octets. Seule son empreinte est persistée, avec une expiration absolue proposée de sept jours. Le navigateur reçoit un cookie hôte `HttpOnly; Secure; SameSite=Lax; Path=/`. Le mode HTTP de développement est explicitement limité à la boucle locale. Vérifier l’origine exacte sur les mutations et à l’ouverture WS, accepter uniquement JSON pour les corps, interdire le CORS tiers. La déconnexion supprime la session, expire le cookie et ferme les sockets associés. Fermer aussi les sockets à l’expiration. Exclure mots de passe, jetons et contenus des journaux.

| Action | Membre | Modérateur | Propriétaire |
|---|---|---|---|
| Lire, envoyer, écouter, voir les membres | Oui | Oui | Oui |
| Créer un salon | — | Oui | Oui |
| Inviter, changer un rôle, exclure | — | — | Oui |

Le serveur résout la communauté depuis le salon demandé et vérifie les droits sur chaque accès. Pour les mutations, verrouiller l’adhésion pendant la vérification afin qu’une exclusion concurrente soit ordonnée avec l’écriture. Une exclusion retire aussi les abonnements concernés. Avant livraison WS, revalider session et appartenance, y compris pour les événements en attente. Le propriétaire conserve son adhésion. Le transfert de propriété demandera une opération explicite.

## Contrat REST v1

Préfixe `/api/v1`. JSON UTF-8, identifiants UUID, dates UTC RFC 3339. Encoder les compteurs SQL bigint en chaînes décimales pour JavaScript. Les listes renvoient `{"items":[...]}`. Les ressources compactes sont `User{id,handle,display_name}`, `Community{id,name,owner_id}`, `Channel{id,community_id,name}`, `Member{user,role}` et `Message{channel_id,seq,author_id,client_id,body,created_at}`. Le rôle effectif peut aussi valoir `owner`.

| Méthode et chemin | Entrée | Succès |
|---|---|---|
| POST /auth/register | handle, display_name, password, invite | 201 User et cookie |
| POST /auth/login | handle, password | 200 User et cookie |
| GET /me · POST /auth/logout | cookie | 200 User · 204 |
| GET /communities · POST /communities | session · name | 200 liste des adhésions sous forme de Community · 201 Community |
| POST /communities/{id}/invites | session | 201 token, expires_at |
| POST /invites/redeem | token | 200 Community |
| GET /communities/{id}/members | session | 200 liste de Member |
| PATCH ou DELETE /communities/{id}/members/{user_id} | role pour PATCH | 200 Member ou 204 |
| GET ou POST /communities/{id}/channels | name pour POST | 200 liste de Channel ou 201 Channel |
| GET /channels/{id}/messages | before ou after exclusif, limit de 1 à 100 (50 par défaut) | 200 liste chronologique |
| POST /channels/{id}/messages | client_id (UUID du client), body (1 à 4 000 caractères) | 201 Message, ou 200 sur répétition identique |

Sans curseur, renvoyer les derniers messages. Avec `before`, prendre les plus proches avant le curseur. Avec `after`, prendre les premiers après. Refuser la combinaison des deux. Une répétition du même `client_id` avec un contenu différent produit 409. Format d’erreur commun : `{"error":{"code":"invalid_invite","message":"Invitation invalide ou expirée."}}`. Utiliser 400 pour un corps invalide, 401 pour une session absente ou expirée, 404 pour une ressource invisible, 403 pour une action interdite sur une ressource visible et 429 pour une limite atteinte.

## Contrat WebSocket

Ouvrir `GET /api/v1/events` avec le cookie. Le client s’abonne au salon affiché. Après contrôle d’accès et installation de l’abonnement, le serveur confirme. Exemple de conversation, avec identifiants abrégés :

```jsonl
{"type":"subscribe","channel_id":"c"}
{"type":"subscribed","channel_id":"c"}
{"type":"message.created","message":{"channel_id":"c","seq":"18","author_id":"u","client_id":"k","body":"Salut !","created_at":"2026-11-05T18:00:00Z"}}
{"type":"unsubscribe","channel_id":"c"}
```

Après `subscribed`, mettre les événements en tampon pendant la lecture REST de l’historique. Fusionner par identité et trier par numéro. À la reconnexion, reprendre après le dernier numéro contigu, paginer jusqu’au rattrapage, puis fusionner le tampon. Une rupture de séquence déclenche ce même rattrapage. La réponse REST à l’envoi peut arriver après l’événement : réconcilier le brouillon via `client_id`. Fermer les connexions lentes si leur file bornée déborde. Reconnexion avec délai croissant et aléa. Une session révoquée impose une nouvelle connexion utilisateur. Une exclusion envoie `access.revoked{channel_id}` puis désabonne. Une commande invalide reçoit `error{code}`. Présence et saisie pourront rester éphémères.

## Client Vue 3

`AppShell` charge `/me` au démarrage. `LoginView` et `InviteView` couvrent les parcours d’entrée. `CommunityNav` et `ChannelList` alimentent la route du salon, qui peut être ouverte directement. `MessageTimeline` charge l’historique par pages et préserve la position de lecture. `MessageComposer` conserve le brouillon en cas d’échec et réutilise son UUID au nouvel essai. Afficher le texte avec l’échappement Vue. `MemberPanel` et `CommunitySettings` présentent les actions correspondant aux droits. Le client distingue salon vide, chargement, hors connexion et accès révoqué. Changer de communauté annule les requêtes obsolètes et libère l’abonnement précédent.

## Premier démarrage de développement

Le Compose fourni lance uniquement la base du premier jalon, avec volume persistant et port limité à la machine locale. Depuis la racine, définir un mot de passe local dans la variable d’environnement `POSTGRES_PASSWORD`, puis lancer :

```sh
docker compose -f deployment/compose.yaml up -d --wait
docker compose -f deployment/compose.yaml ps
```

Le schéma joint est une proposition à porter en migration au premier jalon. Au jalon de distribution, ajouter l’image API avec les ressources Vue, attendre le contrôle de santé PostgreSQL et exposer uniquement Caddy. Le compte applicatif de production aura des privilèges limités. Tester images x86-64 et ARM64 sur machines cibles. Pour l’accès Internet, l’assistant devra vérifier DNS, ports entrants et connectivité réelle avant d’annoncer une instance joignable. Le fonctionnement local reste possible derrière CGNAT. [Ordre de démarrage Compose](https://docs.docker.com/compose/how-tos/startup-order/)

## Étapes et critères de passage

| Jalon | Dépendance | Preuve attendue |
|---|---|---|
| 1. Socle Rust, migration et Compose | Aucune | Démarrer sur base vide, vérifier la disponibilité, redémarrer avec les mêmes données. |
| 2. Comptes et sessions | 1 | Créer le premier utilisateur localement, connecter deux navigateurs, révoquer une seule session, tester expiration et origine refusée. |
| 3. Communautés et salons | 2 | Inviter un second compte, tester le double usage concurrent d’une invitation, la matrice de rôles et l’isolation entre deux communautés. |
| 4. Messages REST | 3 | Tester le renvoi après perte de réponse, les envois concurrents et la pagination sans trou ni doublon. |
| 5. Diffusion WebSocket | 4 | Couper la connexion pendant un échange, reprendre après le curseur, exclure un lecteur connecté et vérifier l’arrêt de livraison. |
| 6. Parcours Vue | 3 pour la navigation, 4–5 pour le chat | Jouer le scénario Alice/Bob dans deux navigateurs, puis tester un lien direct et une reprise après coupure. |
| 7. Distribution locale | 6 | Déployer sur machine vierge, sauvegarder, restaurer ailleurs et vérifier l’historique. Mesurer RAM au repos, CPU et latence sous charge avant de fixer des seuils. |

## Décisions encore ouvertes

- **Diffusion du code.** Choisir la licence avant publication. La politique d’inscription sur invitation et le droit de créer une communauté sont des choix proposés pour le pilote.
- **Exploitation.** Décider qui peut récupérer un compte, limiter stockage et inscriptions, modérer ou purger l’historique. La suppression et l’édition de messages nécessiteront un mécanisme de synchronisation des changements pour les clients déconnectés.
- **Messages privés.** Concevoir un module distinct avec appareils, clés publiques, enveloppes chiffrées et pièces jointes chiffrées côté client. Comparer Signal et MLS sur vérification des appareils, multi-appareil, récupération et licences. Traiter explicitement la confiance dans le client web livré par l’administrateur : celui-ci peut modifier le JavaScript servi. Définir la distribution vérifiable du client avant de promettre une protection contre un hébergeur malveillant. Les identités et conversations resteront locales à l’instance pour cette phase.
- **Voix et vidéo.** Le backend pourra autoriser une entrée dans un salon puis émettre un jeton court limité à cette salle. Le client établira le flux média directement avec une SFU auto-hébergée. Évaluer LiveKit, TURN, réseau UDP sous macOS et coût en bande passante lors d’un prototype séparé.
- **Déploiement et desktop.** Après le pilote, définir l’assistant local puis SSH, la gestion des secrets, les sauvegardes et les diagnostics CGNAT. Tauri pourra embarquer le client Vue et fournir le stockage sécurisé des clés. La fédération reste une décision ultérieure.
