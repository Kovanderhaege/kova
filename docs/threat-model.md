# Modèle de menace initial

## Actifs et frontières

Les secrets racine, appareil et E2EE restent sur les clients. Les profils, adhésions et messages communautaires sont confiés à leur instance. Les enveloppes MP et leurs métadonnées ne sont confiées qu'aux réplicas approuvés. Le réseau public, les autres utilisateurs et les instances tierces ne sont pas de confiance ; une instance autorisée peut elle-même être malveillante.

| Menace | Défense actuelle ou exigence | Limite / test attendu |
|---|---|---|
| Usurpation/rejeu de login | Signature racine puis appareil, challenge aléatoire, contexte origine/opération, expiration, consommation transactionnelle | Tests signature invalide, challenge rejoué/expiré ; un essai invalide annule la transaction et ne consomme pas le challenge, soumis à quota |
| Vol de session | Empreinte SHA-256 côté base, cookie HttpOnly, contrôle Origin, expiration, révocation appareil | Desktop : une heure maximum, coffre Windows ; cookie historique : sept jours ; ne pas journaliser les jetons |
| Compromission de l'instance | Aucun secret privé d'identité reçu | Messages communautaires lisibles ; E2EE non implémenté ; serveur peut mentir ou supprimer des données |
| Appareil compromis | Révocation signée locale conservée avec sa preuve, contrôle à chaque accès authentifié | Révocation non propagée ; autre instance peut accepter l'appareil tant qu'elle ne connaît pas la preuve |
| Invitation volée/concurrente | Jeton aléatoire, empreinte, expiration et consommation transactionnelle | Premier consommateur gagne ; routes d'émission et exclusions à construire |
| Fuite entre communautés | Adhésion à vérifier à chaque REST et chaque livraison WS | Routes absentes ; tests négatifs obligatoires au jalon 3/4 |
| Réplica non autorisé | Futur consentement signé d'Alice ET Bob, authentification interserveurs | Aucune API de MP/fédération aujourd'hui ; tests de refus avant ouverture |
| Suppression/rétention/rejeu MP | Futurs event_id, signatures contextualisées, causalité, réconciliation | Pas de garantie d'effacement, livraison totale ou anonymat ; signaler dépendances manquantes |
| Client distribué par hébergeur hostile | Desktop signé distribué indépendamment | Packaging et chaîne de mise à jour à définir ; poste compromis hors protection E2EE |
| Déni de service | Limite de corps et quota auth en mémoire | Quotas persistants, stockage, réplication et connexions restent à définir |

La fraîcheur des révocations ne dispose actuellement d'aucune garantie inter-instance. Avant les MP, définir un journal signé monotone et une durée maximale d'état périmé ; refuser un nouvel appareil/une modification de politique lorsque l'état nécessaire n'est pas vérifiable. Ne pas annoncer cette protection comme déjà implémentée.

Les horloges serveur servent aux délais locaux, jamais à un ordre global des MP. IP, volumes, participants et horaires restent des métadonnées visibles. Les copies déjà conservées par un réplica révoqué ne sont pas effaçables à distance.

## Limites du prototype Windows

La racine et l'appareil sont protégés par le coffre du compte Windows, mais un processus compromis du même utilisateur peut accéder aux identifiants selon les garanties du système. L'effacement mémoire explicite réduit la durée de présence des secrets détenus par nos structures, sans garantir l'absence de copies dans les bibliothèques ou le système. Récupération chiffrée portable, appairage et interface de restauration restent à construire. Les attestations signées conservées ne fournissent pas encore de fraîcheur globale ni de journal signé monotone. La révocation inter-instance doit encore être soumise à chaque instance.
