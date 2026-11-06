# Réplication MP — conception, non implémentée

Une conversation est globale et référence des identités globales. Le client génère et chiffre les événements avec un protocole E2EE éprouvé à sélectionner. Un réplica ne reçoit que l'enveloppe chiffrée et les métadonnées nécessaires.

Avant la première API MP, figer :

- Identité de serveur et authentification mutuelle HTTPS, liaison de la signature à l'origine, méthode, chemin, empreinte du corps, nonce et échéance ; cache anti-rejeu, rotation et quotas.
- Politique canonique versionnée, conversation, politique précédente et ensemble des réplicas ; approbations des deux participants portant sur exactement le même contenu. Une proposition seule n'autorise aucun transfert.
- Gestion des modifications concurrentes de politique : suspendre toute extension des autorisations tant que le conflit n'est pas résolu et approuvé ; ne jamais fusionner par simple union.
- `event_id` global, auteur/appareil, conversation, version, dépendances et enveloppe signés. Déduplication idempotente ; rejeter un même identifiant avec des octets différents.
- Pagination par curseur propre au réplica, reprise persistante et détection des dépendances manquantes. Ne pas transformer un curseur local en ordre distribué. Déterminer un ordre topologique avec départage déterministe des événements concurrents, indépendant des horloges murales.
- Autorisation à chaque acceptation et lecture ; communauté et session locale ne suffisent pas. Révocation d'un réplica : plus de transferts autorisés, sans prétendre effacer ses copies.

Les adresses A/B sont ajoutées explicitement ; aucun annuaire central. Le cœur client conservera l'outbox SQLite jusqu'à accusé durable, sélectionnera un réplica joignable déjà approuvé et synchronisera les autres au retour. WS sera un indice de disponibilité d'événements, pas la source de vérité.

Preuve attendue : Alice et Bob approuvent A/B ; même historique sur A/B ; C refusée ; envois concurrents et doublons sans divergence ; arrêt A, échanges via B, reprise A ; transferts interrompus, dépendances absentes, politique rejouée ou concurrente, appareil révoqué. Aucun de ces comportements n'est revendiqué aujourd'hui.
