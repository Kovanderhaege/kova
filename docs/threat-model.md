# Modèle de menace initial

## Actifs et frontières

Les secrets racine, appareil et E2EE restent sur les clients. Les profils, adhésions et messages communautaires sont confiés à leur instance. Les enveloppes MP et leurs métadonnées ne sont confiées qu'aux réplicas approuvés. Le réseau public, les autres utilisateurs et les instances tierces ne sont pas de confiance ; une instance autorisée peut elle-même être malveillante.

| Menace | Défense actuelle ou exigence | Limite / test attendu |
|---|---|---|
| Usurpation/rejeu de login | Signature racine puis appareil, challenge aléatoire, contexte origine/opération, expiration, consommation transactionnelle | Tests signature invalide, challenge rejoué/expiré ; un essai invalide annule la transaction et ne consomme pas le challenge, soumis à quota |
| Vol de session | Empreinte SHA-256 côté base, cookie HttpOnly, contrôle Origin, expiration, révocation appareil | Desktop : une heure maximum, coffre Windows ; cookie : borné par le certificat, donc au plus 24 heures ; ne pas journaliser les jetons |
| Compromission de l'instance | Aucun secret privé d'identité ou MLS reçu ; signatures et roster vérifiés côté client | Messages communautaires lisibles ; métadonnées MP visibles ; serveur peut retenir les événements ou cacher les révocations jusqu'à expiration des attestations (24 h maximum) |
| Appareil compromis | Révocation signée locale conservée avec sa preuve, contrôle à chaque accès authentifié | Transmission aux instances connues ; fenêtre de retard bornée par le certificat v2 de 24 h pour un appareil sans racine |
| Invitation volée/concurrente | Jeton aléatoire, empreinte, expiration et consommation transactionnelle | Premier consommateur gagne ; émission réservée au propriétaire/modérateur, exclusion persistante vérifiée à chaque adhésion |
| Fuite entre communautés | Adhésion à vérifier à chaque REST et chaque livraison WS | Contrôles REST et WS implémentés ; verrou communautaire pendant chaque livraison, fermeture après perte d’accès ; copies antérieures non effaçables |
| Réplica non autorisé | Politique signée par Alice ET Bob, origines et clés de serveur épinglées, requêtes et réponses interserveurs signées | Une proposition seule n'autorise aucun transfert ; un retrait n'efface pas les copies et ne peut pas être connu instantanément pendant une partition |
| Suppression/rétention/rejeu MP | Identifiants globaux, signatures contextualisées, parents causaux, curseurs persistants et déduplication exacte ; nonce de fédération consommé en base | Pas de garantie d'effacement, livraison totale ou anonymat ; dépendances manquantes et contenus contradictoires refusés |
| Client distribué par hébergeur hostile | Desktop signé distribué indépendamment | Packaging et chaîne de mise à jour à définir ; poste compromis hors protection E2EE |
| Déni de service | Limite de corps et quota auth en mémoire | Quotas persistants, stockage, réplication et connexions restent à définir |

Les révocations constituent un ensemble monotone signé : un journal ancien ne retire jamais une preuve connue. Le client vérifie les signatures puis transmet aux instances connues. Une instance inaccessible ou qui cache des événements peut accepter un appareil jusqu’à expiration de son autorisation v2 (durée totale ≤ 24 h). Les certificats futurs/expirés sont refusés ; un appareil associé ne possède pas la racine et doit demander son renouvellement. Une racine compromise n’est pas couverte par cette borne. Les politiques MP sont chaînées par empreinte et leur acceptation dépend des appareils du manifeste ; la fédération ne renouvelle pas les attestations.

Les horloges serveur servent aux délais locaux, jamais à un ordre global des MP. IP, volumes, participants et horaires restent des métadonnées visibles. Les copies déjà conservées par un réplica révoqué ne sont pas effaçables à distance.

## Limites du prototype Windows

La racine et l'appareil sont protégés par le coffre du compte Windows, mais un processus compromis du même utilisateur peut accéder aux identifiants selon les garanties du système. L'effacement mémoire explicite réduit la durée de présence des secrets détenus par nos structures, sans garantir l'absence de copies dans les bibliothèques ou le système. La récupération age/scrypt et l’association par fichiers publics sont implémentées. Une phrase secrète faible permet une attaque hors ligne ; conserver la sauvegarde et sa phrase séparément. Le champ de saisie est masqué et ne copie pas son contenu, mais les bibliothèques UI/OS peuvent conserver des copies en mémoire. L’association exige comparaison de l’empreinte et de la clé publique racine via un canal authentifié. Restaurer ne révoque pas les anciens appareils : transmettre leurs révocations aux instances concernées.

Le cache de chat public SQLite et les envois en attente sont en clair dans le profil local, séparés par origine/identité/salon. Ils ne contiennent pas de jeton de session. Une exclusion coupe les futures lectures et livraisons, sans garantir l’effacement des copies locales. Le flux interroge PostgreSQL toutes les 250 ms par connexion : quotas de connexions, rétention et montée en charge restent à traiter avant déploiement public.

## Stockage privé — jalon 5

Le client protège son historique, son état MLS et sa file d'envoi avec age, sous une clé propre au profil Windows. L'instance ne reçoit que les clés publiques, Welcome et messages MLS chiffrés. Les signatures de l'enveloppe et la vérification du roster empêchent la substitution d'un appareil sans détection par un client qui a vérifié les empreintes hors bande.

Le prototype ne garantit pas l'effacement des anciennes clés dans les journaux/snapshots locaux : la compromission de la clé de stockage peut révéler l'historique local conservé et des états archivés. Un attaquant contrôlant le disque peut restaurer un ancien état ; il n'existe pas de compteur anti-rollback matériel. Ne pas revendiquer une forward secrecy des sauvegardes locales ni une récupération post-compromission continue : le roster du jalon 5 est fixe et les commits de mise à jour ne sont pas implémentés. La révocation bloque les nouveaux dépôts sur une instance honnête ; une instance hostile peut cacher l'information jusqu'à l'expiration des preuves.


## Renouvellement des MP

Le [contrat du jalon 10](dm-renewal.md) distingue validité historique des signatures et admission de nouveaux messages. Une preuve renouvelée exige la racine et la possession de la même clé appareil ; elle ne restaure pas un appareil révoqué, ne change pas le roster MLS et ne transfère pas l’historique. Le manifeste conserve ses clés d’origine pour vérifier les anciens messages. Les événements v1 ne prouvent pas leur date de création ; une outbox reprise est admise sous les droits courants. Le contrôle fédéré converge après une partition, sans garantie de connaissance instantanée des révocations ni effacement des copies. Un appareil associé expiré doit obtenir l’intervention de la racine, sans élévation de privilège locale.
