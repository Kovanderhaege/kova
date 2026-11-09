# Réplication MP — jalon 6

La réplication concerne les enveloppes MLS opaques d'une conversation 1:1. Les clés privées d'identité et MLS restent sur les clients. Les communautés ne donnent aucun droit sur les MP. La synchronisation est déclenchée explicitement ; la bascule automatique et l'ordonnancement de fond relèvent du jalon 7.

## Consentement

Une `ReplicaPolicy` contient `conversation_id`, `version`, `previous_hash`, `replicas` et `approvals`. Le contenu signé est le tuple JSON UTF-8 compact `["kova-dm-replica-policy-v1", conversation_id, version, previous_hash, replicas]`. Les descripteurs `{origin, public_key}` sont triés par origine, uniques et limités à huit. La première version inclut l'origine du manifeste et n'a pas de prédécesseur ; chaque version suivante référence l'empreinte SHA-256 du contenu canonique précédent. Les approbations ne font pas partie de cette empreinte.

Les deux appareils inscrits dans le manifeste signent exactement la même proposition. Une approbation porte `identity`, `device`, `signature`. Une seule signature n'autorise aucun transfert. Le client conserve la proposition consultée avant de permettre sa signature ; une nouvelle lecture distante ne peut pas la remplacer au moment de l'approbation. Les signatures et versions sont vérifiées à nouveau par le serveur. Une politique concurrente n'est jamais fusionnée par union des réplicas.

Les politiques retirant un réplica coupent les nouveaux accès autorisés après leur application. Pendant une partition, un serveur peut ignorer une nouvelle politique : aucune révocation instantanée globale n'est revendiquée. Le retrait ne peut pas effacer les enveloppes et métadonnées qu'un serveur a déjà conservées.

Un serveur conservé peut transmettre la nouvelle chaîne à un ancien réplica, même s'il a manqué plusieurs versions. Ce transfert de retrait ne contient aucun événement et ne renvoie aucun historique. Répéter cet avis après une réponse perdue est idempotent. Une ancienne politique reçue est refusée sans être confondue avec une branche contradictoire de même version.

## Configuration des exploitants

Chaque serveur utilise une clé Ed25519 de fédération distincte des clés des utilisateurs. Générer son fichier PKCS#8 v2 avant de lancer le service :

```sh
kova-server generate-federation-key /chemin/prive/federation.pk8
```

La commande refuse d'écraser un fichier existant et affiche la clé publique à échanger. Conserver le fichier hors du dépôt, limiter sa lecture au compte du service et sauvegarder sa clé. Une rotation change l'identité du serveur : mettre à jour les épinglages et faire approuver les nouvelles politiques ; aucune rotation automatique n'est fournie.

Définir `KOVA_FEDERATION_KEY_FILE` vers ce fichier et `KOVA_FEDERATION_PEERS` avec les origines et clés publiques vérifiées hors bande, par exemple :

```json
[{"origin":"https://b.example","public_key":"CLE_PUBLIQUE_ED25519_HEX_64_CARACTERES"}]
```

La clé locale est dérivée du fichier privé ; un descripteur approuvé doit correspondre exactement à cette configuration. Les origines doivent être HTTPS, sans chemin ni barre finale ; HTTP est accepté uniquement sur boucle locale. Aucun pair découvert n'est automatiquement approuvé. Les redirections HTTP sont interdites. Sans configuration de fédération, les MP locaux restent utilisables.

Pour Compose, l'overlay facultatif `deployment/compose.federation.yaml` monte le fichier en secret en lecture seule. Définir `KOVA_FEDERATION_HOST_KEY_FILE` avec son chemin hôte et `KOVA_FEDERATION_PEERS`, puis utiliser les deux fichiers `-f deployment/compose.yaml -f deployment/compose.federation.yaml`. Le compte du conteneur doit pouvoir lire le fichier monté. L'overlay n'ajoute pas de reverse proxy HTTPS ; l'exposition publique et les certificats restent à déployer séparément. Deux conteneurs ne partagent pas leur boucle locale : les tests de fédération démarrent des serveurs natifs sur boucle locale avec des bases PostgreSQL isolées dans Docker.

## Routes

Toutes les routes ci-dessous sont préfixées par `/api/v1`.

| Route | Usage |
|---|---|
| `GET /federation/v1/identity` | Descripteur public à comparer avec la clé communiquée par l'exploitant |
| `GET /dm/conversations/{id}/replicas` | Politique active, après authentification du participant |
| `GET /dm/conversations/{id}/replicas/history` | Chaîne des politiques approuvées pour vérifier une reprise après plusieurs versions |
| `GET /dm/conversations/{id}/replicas/proposal` | Proposition en attente, consultable par les deux participants |
| `POST /dm/conversations/{id}/replicas/approve` | `{policy, approval}` ; politique sans approbations, signature séparée du participant courant |
| `POST /dm/conversations/{id}/replicas/sync` | `{origin}` ; échange avec un pair configuré et autorisé |
| `POST /federation/v1/dm/sync` | Transport serveur à serveur authentifié, sans session d'utilisateur |

Les événements sont dédupliqués par identifiant global ; le même identifiant avec des octets différents est un conflit. Les parents doivent être présents avant l'insertion. Les curseurs locaux et accusés de transfert sont persistants, sans compteur distribué. Après une coupure ou un résultat incertain, relancer la synchronisation : les événements déjà committés sont reconnus et les autres sont repris. Le client conserve un curseur par origine et affiche l'ordre topologique déterministe avec départage par UUID.

La requête interserveurs lie par signature Ed25519 les origines source et destination, la méthode POST, le chemin exact, l'empreinte SHA-256 du corps, un horodatage et un nonce aléatoire. La fenêtre d'acceptation est de 60 secondes ; les nonces acceptés sont conservés en PostgreSQL, y compris après redémarrage. La réponse signe à son tour les origines, le nonce et l'empreinte de la requête, ainsi que l'empreinte de son propre contenu. Les réponses ne peuvent pas être réutilisées pour une autre requête. Ces signatures complètent HTTPS ; elles ne chiffrent pas le contenu.

## Limites

Les attestations du manifeste restent bornées à 24 heures et le roster MLS reste fixe, comme au jalon 5. La réplication ne renouvelle pas les appareils et ne récupère pas un état MLS perdu. Un serveur autorisé peut retenir des données ou mentir sur leur disponibilité ; aucune garantie de livraison totale, d'effacement ou d'anonymat n'est fournie. Participants, origines, volumes et horaires restent des métadonnées visibles. Les quotas globaux d'exploitation et la distribution signée sont hors de ce jalon.

Bornes du prototype : huit réplicas, 64 versions de politique par conversation, dix événements par échange interserveurs et corps limités à 2 Mio. Le client parcourt au plus 64 lots par action ; au-delà, il indique une synchronisation incomplète à relancer. Le serveur refuse une version supplémentaire plutôt que d'activer une politique qu'il ne pourrait plus synchroniser. Une proposition concurrente au même numéro est refusée ; aucun arbitrage automatique ni fusion des autorisations n'est fourni.

Les preuves effectivement exécutées sont consignées dans [le bilan de migration](migration-plan.md) ; la présence de ces routes ne remplace pas leur validation.
