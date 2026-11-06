# Contrats de protocole

## Authentification v1 existante

JSON UTF-8 sous `/api/v1`. Clés publiques : 32 octets hexadécimaux minuscules. Identité : `kova1-` + SHA-256 de la clé racine brute. Appareil : SHA-256 de la clé publique brute. Clés racine et appareil distinctes. Signatures Ed25519 de 64 octets, hexadécimales ; le vérificateur historique accepte aussi les lettres majuscules des signatures.

Les représentations signées sont UTF-8, séparées par LF, sans LF final :

```text
kova-device-v1\n{identity}\n{device_id}\n{device_public_key}\n{expires_at_unix_seconds}
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

La migration 0003 conserve les octets canoniques de l'attestation vérifiée, la signature et l'échéance maximale connue, ainsi que la signature de révocation. Un renouvellement d'attestation ne remet jamais un appareil révoqué en service. Les lignes historiques restent sans preuve jusqu'à sa présentation ; aucune preuve n'est fabriquée. Cela ne constitue pas encore un journal global ordonné de révocations.

Le client Windows conserve clés et sessions dans Credential Manager, par origine pour les sessions. HTTP n'est accepté qu'en boucle locale ; redirections HTTP désactivées. SQLite conserve uniquement adresses publiques et brouillons locaux de démonstration. Aucun secret ni jeton n'y est écrit. Les brouillons MP devront être protégés avant leur introduction.

## Contrats futurs

Les routes communautaires, événements WS, MP et fédération de MVP_target sont des propositions, pas des endpoints disponibles. Les MP auront leur version et contexte de signature propres ; ne pas étendre le format d'authentification ni réutiliser les seq locaux. Choix de canonisation, tailles maximales, dépendances et bibliothèque E2EE à figer ensemble avant leur implémentation.
