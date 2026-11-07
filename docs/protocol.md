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

Les mutations exigent Origin exact. Erreur commune `{ "error": { "code": "…", "message": "…" } }`. Les endpoints d'auth utilisent 400/401/403/409/429/500. Les routes communautaires devront ajouter 404 pour une ressource invisible. Les tests HTTP restent dans `crates/kova-server/tests/sessions.py` ; les formats et vecteurs cryptographiques ont des tests Rust indépendants.

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

Les routes communautaires, événements WS, MP et fédération de MVP_target sont des propositions, pas des endpoints disponibles. Les MP auront leur version et contexte de signature propres ; ne pas étendre le format d'authentification ni réutiliser les seq locaux. Choix de canonisation, tailles maximales, dépendances et bibliothèque E2EE à figer ensemble avant leur implémentation.
