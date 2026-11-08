# Contrats de protocole

## Authentification et autorisation v2

JSON UTF-8 sous `/api/v1`. Clés publiques : 32 octets hexadécimaux minuscules. Identité : `kova1-` + SHA-256 de la clé racine brute. Appareil : SHA-256 de la clé publique brute. Clés racine et appareil distinctes. Signatures Ed25519 de 64 octets, hexadécimales ; le vérificateur historique accepte aussi les lettres majuscules des signatures.

Les représentations signées sont UTF-8, séparées par LF, sans LF final :

```text
kova-device-v2\n{identity}\n{device_id}\n{device_public_key}\n{issued_at_unix_seconds}\n{expires_at_unix_seconds}
kova-revoke-v1\n{identity}\n{device_id}
kova-auth-v1\n{origin}\n{operation}\n{identity}\n{device_id}\n{invite_hash_or_empty}\n{challenge_id}\n{nonce}\n{expires_at}
```

Dans ces exemples, `\n` représente un séparateur LF. L'échéance du challenge a six décimales et le suffixe Z. La ligne d'invitation est présente même vide. Le client doit comparer chaque champ au contexte attendu avant de signer. Les fonctions de `kova-protocol` composent des champs déjà validés ; elles ne valident pas les entrées réseau.

| Route | Contrat actuel |
|---|---|
| POST `/auth/challenges` | Identité, appareil, opération login/join ; invite obligatoire seulement pour join ; 201 avec challenge_id, challenge, expires_at ; validité 2 minutes |
| POST `/auth/verify` | challenge_id, signature, clés publiques, device_authorization ; invite et profile pour première adhésion ; 200/201 et profil + cookie |
| GET `/me` | Profil local id/handle/display_name et global_identity_id ; 401 sans session valide |
| POST `/auth/logout` | Corps vide, révoque la session ; 204 |
| GET `/identity/devices` | Appareils connus de l'identité sur cette instance ; inclut attestations connues (payload, signature, expiration) et signature de révocation ; valeurs nulles pour preuves historiques absentes |
| POST `/identity/devices/revoke` | device_public_key et signature racine ; révocation permanente locale, soumission répétable |

Les mutations (POST, PATCH, DELETE) exigent Origin exact. Erreur commune `{ "error": { "code": "…", "message": "…" } }`. Les endpoints d'auth utilisent 400/401/403/409/429/500. Les routes communautaires ajoutent 404 pour une ressource invisible. Les tests HTTP sont dans `crates/kova-server/tests` ; les formats et vecteurs cryptographiques ont des tests Rust indépendants.

## Sessions desktop implémentées

`POST /auth/challenges` accepte `session_mode: "bearer"` (défaut `cookie` pour compatibilité). Le mode est stocké dans le challenge ; celui-ci remplace le domaine `kova-auth-v1` par `kova-auth-desktop-v1`, avec les huit champs suivants inchangés. La réponse contient aussi `session_mode`. Le client Rust vérifie domaine, origine exacte, opération, identité, appareil, invitation, identifiant, nonce et échéance avant de signer. Une preuve cookie ne peut pas authentifier un challenge desktop.

`POST /auth/verify` retrouve le mode depuis le challenge enregistré. Pour desktop, il renvoie `{access_token, token_type: "Bearer", expires_in, profile}`, sans Set-Cookie. `expires_in` est en secondes, au plus 3600 et borné par l'attestation. Le serveur ne conserve que l'empreinte du jeton et son transport. Renouvellement par un nouveau challenge, aucun refresh token.

Les appels authentifiés utilisent `Authorization: Bearer <jeton>`. Origin exact reste obligatoire sur POST, y compris pour le desktop. Mélange cookie/Authorization, cookies de session multiples et en-têtes Authorization multiples sont refusés. Un jeton cookie ne fonctionne pas comme Bearer, et inversement. Logout ne renvoie pas de cookie en mode desktop. Révocation d'appareil et échéance rendent immédiatement les sessions locales inutilisables.

La migration 0003 conserve les octets canoniques de l'attestation vérifiée, la signature et l'échéance maximale connue, ainsi que la signature de révocation. Un renouvellement d'attestation ne remet jamais un appareil révoqué en service. Les lignes historiques restent sans preuve jusqu'à sa présentation ; aucune preuve n'est fabriquée. La migration 0004 ajoute le journal monotone décrit ci-dessous. Aucun ordre global des événements n’est revendiqué.

Le client Windows conserve clés et sessions dans Credential Manager, par origine pour les sessions. HTTP n'est accepté qu'en boucle locale ; redirections HTTP désactivées. SQLite conserve adresses publiques, preuves publiques de révocation et brouillons locaux de démonstration. Aucun secret ni jeton n'y est écrit. Les brouillons MP devront être protégés avant leur introduction.

## Fraîcheur et journal signé

`device_authorization` contient `issued_at`, `expires_at`, `signature`. Le serveur exige `0 < issued_at <= maintenant < expires_at` et une durée totale comprise entre 1 et 86400 secondes. Les horloges doivent être synchronisées ; un certificat futur ou une date absente échoue sans dérogation. Les autorisations v1 sont refusées, même pendant leur dernier jour de validité ; leur ancienne fonction canonique reste disponible pour les tests historiques. La migration 0004 invalide toutes les sessions ; renouveler les autorisations depuis la racine.

- `POST /identity/revocations` reçoit `{global_identity_id, device_public_key, signature}`. Pas de session requise : la signature racine et l’origine exacte sont obligatoires, avec quota d’authentification. L’identité doit être connue de l’instance. Un appareil encore inconnu peut être révoqué. La preuve répétée est idempotente ; aucune autorisation ultérieure ne peut effacer le tombstone.
- `GET /identity/revocations` exige une session et renvoie `{items:[{event_id,payload,signature,device_public_key}]}` pour son identité. `event_id = SHA-256(payload UTF-8)`. L’ensemble est monotone, sans séquence globale ni suppression. Au-delà de 1024 éléments, réponse 429 plutôt qu’un journal tronqué.
- Le client vérifie la racine épinglée, le payload canonique, la signature et l’identifiant de chaque événement, puis conserve l’union en SQLite. Un journal vide/ancien ne supprime aucune preuve connue. Il retransmet les preuves aux origines connues après connexion et via le bouton de nouvel essai. Les échecs restent visibles et les preuves conservées pour réessai ; aucune réplication serveur-à-serveur automatique n’est annoncée.
- Une instance qui ignore une révocation peut accepter un certificat encore valide. La durée totale de 24 h borne cette fenêtre pour un appareil sans racine ; un renouvellement nécessite la racine. Une racine compromise peut autoriser de nouvelles clés et n’est pas couverte par la révocation d’un appareil. Le journal ne prouve pas à lui seul qu’un serveur ne cache aucun événement.

## Association et récupération Windows

Chaque profil local (`default` ou nom ASCII de 1–32 caractères) possède ses entrées Credential Manager. Une sauvegarde portable age v1/scrypt chiffre uniquement la racine ; une restauration vers un profil vide génère un nouvel appareil et exclut les sessions. Phrase secrète de 12 caractères minimum à la création, fichiers limités à 64 Kio, aucun écrasement implicite. Sa perte n’est pas récupérable par le serveur.

La demande publique signe `kova-link-request-v1\n{device_public_key}\n{nonce}\n{expires_at}` (nonce 32 octets, échéance 10 minutes). L’empreinte est SHA-256 de ce payload. L’utilisateur doit la comparer intégralement sur un canal de confiance. La réponse contient l’autorisation v2 et signe `kova-link-response-v1\n{request_hash}\n{authorization_payload}`. Le nouvel appareil vérifie sa demande, la racine attendue, les deux signatures et la validité ; sa clé privée reste dans son coffre. La demande locale est retirée après installation. Renouveler suit le même parcours avec la même clé d’appareil. Le QR n’est pas implémenté.

## Contrats futurs

Les événements MP du jalon 5 sont décrits ci-dessous ; la fédération reste une proposition. Le chat public WS est décrit ci-dessous. Les MP ont leurs domaines de signature propres et ne réutilisent pas les séquences des salons.

## Communautés locales — jalon 3

Session cookie ou Bearer obligatoire sur toutes ces routes. Les réponses utilisent `Cache-Control: no-store`. Noms : 1–80 caractères après suppression des espaces périphériques, sans caractères de contrôle. Identifiants : UUID locaux ; aucun droit implicite d'administrateur d'instance.

| Méthode et route | Corps / résultat |
|---|---|
| GET `/communities` | `{items:[{id,name,owner_id,role}]}` ; uniquement ses adhésions |
| POST `/communities` | `{name}` ; 201 et communauté ; adhésion propriétaire et salon `général` créés atomiquement |
| GET `/communities/{id}` | Communauté et rôle du demandeur |
| GET `/communities/{id}/channels` | `{items:[{id,community_id,name}]}` |
| POST `/communities/{id}/channels` | `{name}` ; 201 et salon, 409 si nom déjà présent dans cette communauté |
| GET `/communities/{id}/members` | `{items:[{id,handle,display_name,role}]}` ; rôle effectif `owner`, `moderator` ou `member` |
| POST `/communities/{id}/invites` | `{expires_in}` en secondes, 1–604800 ; 201 `{invite,community_id,expires_in}` ; jeton de 32 octets hex, usage unique, empreinte seule en base |
| POST `/invites/redeem` | `{invite}` ; session existante ; 201 `{community_id}` si nouvelle adhésion, 200 si déjà membre ; invitation consommée dans les deux cas |
| PATCH `/communities/{id}/members/{user_id}` | `{role:"member"\|"moderator"}` ; propriétaire seul, cible autre que propriétaire ; 204 |
| DELETE `/communities/{id}/members/{user_id}` | Corps vide ; exclusion persistante et retrait d'adhésion atomiques ; 204 |

Les membres lisent communauté, salons et membres. Propriétaire/modérateurs créent salons et invitations ; seul le propriétaire attribue les rôles. Un modérateur exclut seulement un membre simple ; le propriétaire peut aussi exclure un modérateur. Personne ne peut exclure le propriétaire ni s'exclure par cette route. Pas de transfert de propriété ou de levée d'exclusion dans ce jalon.

Une ressource absente ou une communauté dont le demandeur n'est pas membre renvoie 404 ; un membre sans le droit requis reçoit 403. Une invitation invalide, expirée ou consommée reçoit 400. L'exclusion est vérifiée aussi à la rédemption et au challenge signé `join` : une nouvelle invitation ne la contourne pas (403). Un échec annule la consommation. Les profils historiques et les autres communautés/sessions restent intacts ; aucun droit MP n'en découle.

La première connexion garde le parcours challenge `join` + `/auth/verify` du jalon 2, avec profil local et session créés dans la même transaction que l'adhésion. Les opérations d'une communauté et les rédemptions verrouillent sa ligne avant de relire droits/exclusions. Une lecture déjà terminée ne peut pas être effacée chez le client ; après commit de l'exclusion, toute nouvelle lecture est refusée. Les abonnements WS du jalon 4 suivent les mêmes contrôles, décrits ci-dessous.


## Chat public — jalon 4

Session valide et adhésion à la communauté du salon obligatoires. Ressource absente ou invisible : 404. Les mutations et le handshake WS exigent une seule origine exacte. Le desktop utilise Authorization Bearer, jamais un jeton dans une URL. Aucun droit implicite de l'administrateur d'instance.

| Méthode et route | Contrat |
|---|---|
| POST `/channels/{channel}/messages` | `{client_id,body}` ; UUID canonique minuscule, corps non blanc de 1–4000 caractères Unicode sans NUL ; 201 et message, 200 pour un rejeu identique |
| GET `/channels/{channel}/messages?after=0` | `{items,next_seq,last_seq}` ; au plus 100 messages, ordre croissant, séquence strictement supérieure à `after` |
| GET `/channels/{channel}/events?after=0` | Upgrade WebSocket HTTP/1.1 ; chaque trame texte contient une page de même format, y compris les pages vides de maintien du flux |

Un message contient `{channel_id,seq,author_id,client_id,body,created_at}`. `created_at` est une date UTC ; seul `seq` définit l'ordre local au salon. `after` vaut zéro par défaut, doit être positif ou nul et ne peut dépasser `last_seq`. `next_seq` est la séquence du dernier élément retourné, ou le curseur demandé à vide. Continuer jusqu'à `next_seq == last_seq`, puis s'abonner depuis le curseur conservé.

L'idempotence porte sur `(salon,auteur,client_id)` : même identifiant et même texte renvoient le message initial ; autre texte : 409 `message_conflict`. Vérification des droits, incrément et insertion sont sérialisés avec les opérations communautaires. Un rollback ou un rejeu ne consomme pas de séquence. Pas de modification/suppression de messages dans ce jalon. Le corps HTTP des routes chat est limité à 32 Kio.

Le serveur relit le journal committé toutes les 250 ms, par lots de 100. Il revalide session et adhésion à chaque lot, même vide. L'envoi borné à deux secondes conserve le verrou communautaire : une exclusion ne peut se terminer entre le contrôle d'adhésion et cette livraison. Perte de session/adhésion : fermeture 1008 ; erreur interne : 1011. Une livraison déjà effectuée peut encore arriver après exclusion. Les trames entrantes sont limitées à 8 Kio ; le flux sert à recevoir, l'envoi utilise HTTP.

Le cache client conserve les messages et son curseur contigu atomiquement, séparés par origine, identité et salon. Il rejette les trous et les contradictions, accepte les doublons identiques. Après coupure, le desktop repart du curseur durable. Un envoi incertain conserve texte et UUID pour un réessai explicite, y compris après redémarrage. Aucun envoi automatique hors ligne. Le cache public n'est pas chiffré et une exclusion n'efface pas les copies déjà reçues.

La migration 0006 recale les compteurs sur les messages historiques ; elle refuse une histoire à trous au lieu de renuméroter les messages. Sauvegarder avant mise à niveau et réparer explicitement un import incohérent avant de réessayer.

## MP E2EE 1:1 — jalon 5

Routes sous `/api/v1`, session et Origin habituels. Aucun accès implicite par rôle communautaire ou administrateur. La lecture d'une conversation exige l'identité **et l'appareil** d'un des deux participants du manifeste.

| Route | Contrat |
|---|---|
| `POST /dm/key-packages` | Publier/remplacer son `DeviceBundle` signé ; aucune clé privée |
| `GET /dm/key-packages/{identity}/{device}` | Récupérer le bundle public d'un appareil connu ; session requise |
| `POST /dm/conversations` | Déposer un `ConversationManifest` signé par le créateur ; consommer atomiquement le package du destinataire ; réessai identique idempotent |
| `GET /dm/conversations` | Manifestes accessibles à l'appareil courant |
| `GET /dm/conversations/{id}` | Manifeste et Welcome chiffré ; l'acceptation cryptographique reste locale |
| `POST /dm/conversations/{id}/events` | Événement signé opaque ; réponse `{cursor,event}` après commit durable |
| `GET /dm/conversations/{id}/events?after=0&limit=100` | `{items:[{cursor,event}],next_cursor}` ; curseur local exclusif, limite 1–100 |

Les définitions normatives sont `kova-protocol/src/dm.rs`. Toutes les données binaires sont hexadécimales minuscules ; les identifiants conversation/événement sont des UUID canoniques. Les signatures Ed25519 couvrent un tableau JSON compact UTF-8 sérialisé dans cet ordre :

- Bundle : `["kova-dm-device-v1",global_identity_id,device_id,root_public_key,device_public_key,issued_at,expires_at,authorization_signature,key_package]`.
- Manifeste : `["kova-dm-conversation-v1",conversation_id,origin,creator,recipient,welcome]` ; `creator`/`recipient` sont les objets complets du bundle, ordre des champs ci-dessus puis `signature`.
- Événement : `["kova-dm-event-v1",conversation_id,event_id,author_identity,author_device,parents,ciphertext]`.

Aucun espace superflu ni LF final. Les chaînes suivent l'échappement JSON de serde_json, les timestamps sont des entiers Unix. L'autorisation racine reste le format `kova-device-v2` existant. Le client vérifie signatures, échéances, empreintes obtenues par canal indépendant et roster MLS exact (identité, appareil, clé de signature MLS) avant acceptation. Les événements historiques vérifient les signatures sans considérer leur ancienne autorisation comme fraîche aujourd'hui.

Les parents sont triés, uniques, au plus 16 ; chaque dépendance doit déjà exister dans cette conversation. Un parent absent ou un identifiant rejoué avec d'autres octets produit un conflit. Le plaintext MLS contient aussi conversation, event_id, parents et corps : le client refuse toute divergence avec l'enveloppe signée. Le corps possède les mêmes limites textuelles que le chat public. Le serveur limite le ciphertext à 16 Kio, les KeyPackages et Welcome à 32 Kio chacun et le corps HTTP MP à 200 000 octets. Les erreurs existantes restent inchangées hors routes MP.

La création et les nouveaux messages vérifient les appareils actifs ; révoquer un participant bloque les nouveaux dépôts. Un participant encore actif peut lire l'historique après révocation de son correspondant. L'instance peut cacher une révocation ; le client borne donc les nouveaux envois à l'échéance des attestations du manifeste, au maximum 24 heures. Le renouvellement en place, les notifications WS MP et les réplicas sont hors jalon 5. L'ordre d'affichage local est topologique, avec départage par UUID, pas par horloge serveur.
