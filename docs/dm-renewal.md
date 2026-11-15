# Renouvellement des MP — contrat du jalon 10

## Identité et autorisations

Le manifeste, son `conversation_id`, le Welcome et les deux appareils MLS restent immuables. Un renouvellement est un `DeviceBundle` du même participant, avec exactement les mêmes clés racine/appareil et le même KeyPackage public. Ce package sert ici à lier la preuve au roster existant ; il n'est pas consommé une seconde fois pour créer un groupe. L'autorisation racine v2 et la signature de possession de l'appareil sont revérifiées. La durée reste positive et au plus 86 400 secondes ; une preuve future n'autorise aucun envoi.

L'état public indépendant contient `authorizations` (au plus une preuve par participant) et `revocations` (au plus une révocation racine par participant). Le manifeste constitue la preuve initiale. L'ordre total `(issued_at, expires_at, signature)` sélectionne la meilleure preuve valide, indépendamment de l'ordre de livraison. Une ancienne livraison ne remplace jamais une preuve plus récente. Les mutations sont sérialisées sous le verrou de conversation ; les clients conservent le même état dans leur snapshot chiffré.

## Admission et historique

Un nouvel événement local ou une nouvelle approbation de politique exige une session valide et les autorisations courantes fraîches des deux appareils, sans révocation connue. Le renouvellement d'un participant n'exige pas la fraîcheur de l'autre : les deux côtés doivent pouvoir sortir de l'expiration. Une clé racine locale peut signer ; un appareil associé doit recevoir une autorisation renouvelée par son appareil racine, via l'association du même appareil déjà disponible. Aucune élévation implicite de ses droits n'est permise.

Les anciens événements gardent leur signature sur les octets v1 et se vérifient contre les clés du manifeste. Ils n'ont pas d'horodatage signé fiable : le serveur ne prétend pas prouver l'heure de leur création. Un événement d'outbox préparé avant expiration peut être soumis après renouvellement, avec son identifiant et son ciphertext d'origine ; il est admis sous l'autorisation courante, sans régénération MLS. Les événements historiques transférés depuis un réplica approuvé sont contrôlés par signature, causalité et déduplication, indépendamment de l'expiration de leur ancienne attestation. Une révocation connue bloque les nouveaux dépôts ; elle ne détruit pas l'historique local ni les copies existantes.

## Transport et reprise

- `GET /api/v1/dm/conversations/{id}/authorizations` retourne l'état public vérifiable aux participants authentifiés.
- `POST` sur cette route reçoit le `DeviceBundle` renouvelé du participant authentifié et retourne l'état fusionné. Le manifeste n'est jamais remplacé.
- Les requêtes et réponses de fédération transportent un champ `authorizations` contenant cet état. Le contrôle est propagé même après expiration ou révocation ; l'échange de contrôle ne doit pas dépendre de l'admission d'un nouveau message. Les preuves restent vérifiées contre le manifeste et les révocations sont définitives.
- Le client persiste son renouvellement avant publication et republie ses preuves lors de la reprise. Une erreur réseau conserve conversation, état MLS, messages et outbox. Les réplicas rattrapent leur contrôle avant d'admettre de nouveaux envois.

Le contrôle valide et le nonce de fédération sont committés avant les lots de messages. Un lot invalide ne fait pas disparaître une révocation ; le réessai emploie un nouveau nonce. Une révocation reçue est aussi ajoutée au journal global de l'instance et aux autres conversations locales du client où cet appareil apparaît. Après révocation connue, les échanges fédérés transportent le contrôle sans nouveaux événements ; un participant restant autorisé peut toujours consulter les enveloppes déjà conservées sur son réplica.

Une partition peut cacher une révocation jusqu'à la prochaine synchronisation. Les attestations de 24 heures bornent l'autorisation sans renouvellement, mais une racine compromise reste capable d'en signer d'autres. Ni l'effacement des copies, ni la récupération d'un état MLS perdu, ni un nouveau dispositif ne sont fournis. Les anciens binaires ne bénéficient pas du renouvellement ; mettre à jour clients et réplicas ensemble.

## Références

Les formats de signature et bibliothèques existants sont réutilisés, sans nouvelle primitive cryptographique. Vérification des références le 15 novembre 2026 : [Ed25519, RFC 8032 §5.1.7](https://www.rfc-editor.org/rfc/rfc8032#section-5.1.7), [authentification MLS, RFC 9420 §16](https://www.rfc-editor.org/rfc/rfc9420#section-16). Les règles d'expiration, de fusion et d'admission ci-dessus sont des règles Kova ; ces RFC ne constituent pas un audit de leur intégration.
