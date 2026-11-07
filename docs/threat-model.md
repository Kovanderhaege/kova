# Modèle de menace initial

## Actifs et frontières

Les secrets racine, appareil et E2EE restent sur les clients. Les profils, adhésions et messages communautaires sont confiés à leur instance. Les enveloppes MP et leurs métadonnées ne sont confiées qu'aux réplicas approuvés. Le réseau public, les autres utilisateurs et les instances tierces ne sont pas de confiance ; une instance autorisée peut elle-même être malveillante.

| Menace | Défense actuelle ou exigence | Limite / test attendu |
|---|---|---|
| Usurpation/rejeu de login | Signature racine puis appareil, challenge aléatoire, contexte origine/opération, expiration, consommation transactionnelle | Tests signature invalide, challenge rejoué/expiré ; un essai invalide annule la transaction et ne consomme pas le challenge, soumis à quota |
| Vol de session | Empreinte SHA-256 côté base, cookie HttpOnly, contrôle Origin, expiration, révocation appareil | Desktop : une heure maximum, coffre Windows ; cookie : borné par le certificat, donc au plus 24 heures ; ne pas journaliser les jetons |
| Compromission de l'instance | Aucun secret privé d'identité reçu | Messages communautaires lisibles ; E2EE non implémenté ; serveur peut mentir ou supprimer des données |
| Appareil compromis | Révocation signée locale conservée avec sa preuve, contrôle à chaque accès authentifié | Transmission aux instances connues ; fenêtre de retard bornée par le certificat v2 de 24 h pour un appareil sans racine |
| Invitation volée/concurrente | Jeton aléatoire, empreinte, expiration et consommation transactionnelle | Premier consommateur gagne ; routes d'émission et exclusions à construire |
| Fuite entre communautés | Adhésion à vérifier à chaque REST et chaque livraison WS | Routes absentes ; tests négatifs obligatoires au jalon 3/4 |
| Réplica non autorisé | Futur consentement signé d'Alice ET Bob, authentification interserveurs | Aucune API de MP/fédération aujourd'hui ; tests de refus avant ouverture |
| Suppression/rétention/rejeu MP | Futurs event_id, signatures contextualisées, causalité, réconciliation | Pas de garantie d'effacement, livraison totale ou anonymat ; signaler dépendances manquantes |
| Client distribué par hébergeur hostile | Desktop signé distribué indépendamment | Packaging et chaîne de mise à jour à définir ; poste compromis hors protection E2EE |
| Déni de service | Limite de corps et quota auth en mémoire | Quotas persistants, stockage, réplication et connexions restent à définir |

Les révocations constituent un ensemble monotone signé : un journal ancien ne retire jamais une preuve connue. Le client vérifie les signatures puis transmet aux instances connues. Une instance inaccessible ou qui cache des événements peut accepter un appareil jusqu’à expiration de son autorisation v2 (durée totale ≤ 24 h). Les certificats futurs/expirés sont refusés ; un appareil associé ne possède pas la racine et doit demander son renouvellement. Une racine compromise n’est pas couverte par cette borne. Les futures politiques MP devront traiter leur propre fraîcheur et leurs dépendances avant ouverture.

Les horloges serveur servent aux délais locaux, jamais à un ordre global des MP. IP, volumes, participants et horaires restent des métadonnées visibles. Les copies déjà conservées par un réplica révoqué ne sont pas effaçables à distance.

## Limites du prototype Windows

La racine et l'appareil sont protégés par le coffre du compte Windows, mais un processus compromis du même utilisateur peut accéder aux identifiants selon les garanties du système. L'effacement mémoire explicite réduit la durée de présence des secrets détenus par nos structures, sans garantir l'absence de copies dans les bibliothèques ou le système. La récupération age/scrypt et l’association par fichiers publics sont implémentées. Une phrase secrète faible permet une attaque hors ligne ; conserver la sauvegarde et sa phrase séparément. Le champ de saisie est masqué et ne copie pas son contenu, mais les bibliothèques UI/OS peuvent conserver des copies en mémoire. L’association exige comparaison de l’empreinte et de la clé publique racine via un canal authentifié. Restaurer ne révoque pas les anciens appareils : transmettre leurs révocations aux instances concernées.
