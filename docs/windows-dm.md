# Messagerie privée E2EE — prototype Windows

Le panneau **MP chiffrés** du desktop Windows permet de publier un appareil destinataire, créer ou accepter une conversation, synchroniser son fil, envoyer et réessayer un message resté en attente. Les opérations de réseau et de chiffrement tournent en arrière-plan. L'état MLS et les messages en attente sont conservés dans la base locale du profil et protégés par son secret stocké dans le coffre Windows.

## Établir une conversation

Alice et Bob se connectent à la même instance et publient chacun leur appareil dans le panneau. Ils s'échangent par un canal indépendant l'identité globale et l'empreinte complète de l'appareil, puis les vérifient avant de poursuivre. Le créateur fournit les empreintes de l'appareil distant vérifiées. L'autre participant sélectionne l'identifiant de conversation transmis, ressaisit et vérifie les empreintes du créateur, puis accepte explicitement l'invitation cryptographique. La création seule n'ouvre pas l'historique au correspondant.

Les messages sont chiffrés dans le client avec OpenMLS. L'instance peut stocker et relayer les événements opaques, mais ne détient pas l'état de groupe ni la clé de déchiffrement. Le client synchronise les événements après une coupure et conserve localement un envoi incertain jusqu'à son réessai. Si le résultat réseau d'un envoi est incertain, le champ de saisie reste affiché ; utilisez **Réessayer** pour reprendre le même événement au lieu de le recréer. L'identifiant de conversation est globalement stable ; les curseurs de transport restent locaux à l'instance.

## Limites du jalon

Le prototype prend en charge une conversation 1:1 établie avec un appareil par identité. L'ajout d'un appareil n'importe pas automatiquement l'historique. Le jalon 6 ajoute le consentement et la réplication décrits ci-dessous ; le jalon 7 ajoute la bascule automatique et la reprise de fond. Les clés privées de session dépendent du coffre du profil local : la sauvegarde de la racine d'identité ne sauvegarde pas automatiquement l'état MLS. Perdre ce profil peut rendre la conversation irrécupérable. Le protocole de récupération de conversation n'est pas livré.

La vérification visuelle de l'interface n'atteste pas à elle seule les garanties cryptographiques. Les limites d'audit et de persistance OpenMLS figurent dans [la décision E2EE](decisions/e2ee.md) ; les contrats wire et résultats de test sont consignés dans [le bilan du jalon](migration-plan.md).

Les attestations d'appareil du manifeste expirent au plus 24 heures après leur émission. Le prototype arrête alors les nouveaux envois : reconnectez les appareils, republiez le package du destinataire et créez une nouvelle conversation après vérification. L'ancien historique reste consultable. Un package destinataire est consommé à la création : republiez après acceptation avant de créer une autre conversation entrante. Les données locales sont chiffrées, mais aucune suppression physique des anciennes clés dans le disque, les journaux SQLite ou les sauvegardes n'est garantie. Ne restaurez pas un ancien snapshot MP pour reprendre un envoi : le rollback du ratchet n'est pas protégé par ce prototype.

**Hors ligne**, avec l'identifiant de conversation vide, affiche la liste locale globale sans connexion ni session. Avec un identifiant, il affiche l'historique et les envois en attente. **Synchroniser** récupère les messages reçus ; aucune notification temps réel MP n'est fournie dans ce jalon.

## Autoriser et synchroniser un réplica — jalon 6

Les exploitants configurent d'abord les identités de fédération et les clés de pairs selon [le guide de réplication](replication.md). Les deux participants doivent avoir accepté la conversation sur leur appareil. L'adresse seule ne prouve pas l'identité d'un serveur : comparer également sa clé publique avec celle communiquée par son exploitant via un canal authentifié.

1. Dans **MP chiffrés**, renseigner la conversation et l'origine exacte de B, puis préparer une proposition de réplica.
2. Consulter les origines, clés, version et empreinte de la proposition. L'approbation est une action séparée et signe la proposition conservée localement après cette consultation.
3. Le premier participant approuve ; le second consulte la proposition sur A, vérifie les mêmes informations et l'approuve à son tour. Une seule signature n'autorise aucun transfert.
4. Déclencher **Synchroniser avec le réplica approuvé**. A et B échangent directement leurs enveloppes chiffrées ; les erreurs restent visibles et la même action reprend le transfert.
5. Pour utiliser manuellement B, sélectionner son origine et une session propre à B avec la même identité et le même appareil. Le cache garde un curseur distinct pour chaque origine et un seul historique par conversation.

Retirer un réplica demande également une nouvelle politique approuvée par les deux participants. La nouvelle politique doit être transmise aux instances concernées ; une instance isolée ne peut pas apprendre instantanément son retrait. Le retrait n'efface pas les copies déjà conservées. Les conflits de politique ne sont jamais résolus par union automatique des autorisations.

## Bascule et reprise automatique — jalon 7

Connectez auparavant le même appareil à chaque réplica souhaité, puis synchronisez la conversation et sa politique sur ces instances. Gardez l'origine préférée et l'identifiant de conversation dans le panneau MP. **Envoyer**, **Réessayer** et **Synchroniser** sélectionnent automatiquement un réplica autorisé si cette origine est indisponible ; le résultat indique l'origine utilisée. Une session absente ou expirée nécessite une reconnexion dans **Connexion**. Aucun mot de passe ni jeton n'est partagé entre instances.

Le panneau tente aussi de synchroniser et reprendre les envois toutes les 30 secondes, avec un délai croissant jusqu'à cinq minutes après échec. **Hors ligne** et les actions de consultation/approbation des politiques mettent ce travail en pause ; **Synchroniser**, **Envoyer** ou **Réessayer** le réactivent. Changer de panneau ou fermer la fenêtre arrête les nouvelles tentatives. Les textes saisis ne sont jamais envoyés automatiquement : seuls les événements déjà préparés par **Envoyer** sont repris.

Après une panne, ne recréez pas le message : la file locale conserve le même événement chiffré, y compris après redémarrage. Si le réplica manque d'un message antérieur du correspondant, la reprise peut attendre le retour d'une source qui le possède. Les refus d'accès et conflits de sécurité restent affichés et ne sont pas contournés par une bascule. Les détails et limites figurent dans [le contrat de réplication](replication.md#sélection-automatique--jalon-7).
