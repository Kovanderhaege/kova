# Distribution du MVP Windows et installation serveur

Le jalon 8 de `MVP_target.md` couvre le desktop packagé, l'installation serveur,
les sauvegardes et le parcours Alice/Bob sur deux instances indépendantes.
Le client reste un prototype Windows x86-64 MSVC. Le ZIP produit ici est une
distribution de développement non signée ; les sommes SHA-256 détectent une
altération accidentelle, elles ne prouvent pas l'identité de l'éditeur.

## Installer une instance

Prévoir Docker Engine avec Compose v2, un nom DNS pointant vers l'hôte et les
ports TCP 80/443 accessibles depuis Internet. Derrière CGNAT, configurer un
relais/tunnel explicitement ; aucune joignabilité publique n'est automatique.
Le fichier `compose.production.yaml` est autonome : ne pas le fusionner avec
`compose.yaml`, qui expose des ports de développement.

1. Copier `deployment/production.env.example` hors du dépôt, par exemple dans
   `/srv/kova/production.env`, et renseigner domaine, courriel ACME et chemin
   absolu du fichier de mot de passe PostgreSQL.
2. Sur un hôte Linux Docker classique (daemon root, sans remappage des UID),
   créer le secret en tant que root :

```sh
install -d -m 0700 /srv/kova/secrets
umask 077
openssl rand -hex 32 > /srv/kova/secrets/database-password
chown root:65534 /srv/kova/secrets/database-password
chmod 0640 /srv/kova/secrets/database-password
```

   Le répertoire root `0700` empêche les autres comptes hôte de traverser le
   chemin ; le groupe `65534` permet à l'API de lire le fichier monté. Les
   secrets Compose sont des fichiers montés, pas un coffre chiffré. Avec Docker
   rootless ou remappage d'UID, adapter explicitement propriétaire/groupe aux
   identités mappées et vérifier la sonde après démarrage ; ne pas rendre le
   secret lisible par tous pour contourner une erreur de permissions.
3. Depuis la racine du checkout, démarrer :

```sh
docker compose --env-file /srv/kova/production.env -p kova -f deployment/compose.production.yaml up -d --build --wait
curl --fail https://chat.example.org/health/ready
```

Le démarrage PostgreSQL sain ne remplace pas la sonde HTTPS : elle doit renvoyer
200 après les migrations. Caddy gère le certificat public et son renouvellement,
et conserve ses données dans les volumes `caddy_data` et `caddy_config`.
Seul Caddy publie des ports ; l'API et PostgreSQL restent sur le réseau Compose.
Le WebSocket public utilise la même origine HTTPS. Aucun jeton n'est placé dans
les URL et les journaux d'accès Caddy ne sont pas activés.

Créer une identité depuis le desktop puis copier sa clé publique racine :

```sh
docker compose --env-file /srv/kova/production.env -p kova -f deployment/compose.production.yaml exec api kova-server bootstrap-admin alice Alice CLE_PUBLIQUE_RACINE_HEX
```

La lecture du secret serveur passe par `KOVA_DATABASE_PASSWORD_FILE`. Un fichier
vide, illisible ou contradictoire avec `PGPASSWORD` bloque le démarrage ; un
LF/CRLF final est retiré. Changer ce fichier ne change pas le mot de passe d'un
volume PostgreSQL déjà initialisé : une rotation nécessite aussi `ALTER ROLE`
et le redémarrage de l'API.

## Sauvegarder et restaurer

Python 3.10+ suffit pour `deployment/backup.py`, qui utilise les outils PostgreSQL
17 du conteneur. Charger les trois variables du fichier d'environnement dans
le shell avant les commandes suivantes (ou définir `COMPOSE_ENV_FILES` avec son
chemin absolu). Le nom de projet doit être identique à celui du déploiement.

```sh
python deployment/backup.py --project kova backup /srv/backups/kova-20261115.dump
python deployment/backup.py --project kova restore /srv/backups/kova-20261115.dump --database kova_restored_20261115
```

La sauvegarde custom PostgreSQL n'écrase jamais un fichier existant. Elle contient
les profils, messages publics, enveloppes MP, politiques, révocations, sessions
et checksums des migrations. L'archive est créée en mode `0600` sur POSIX ; sur
Windows, contrôler les ACL du dossier de destination. Elle contient des données sensibles : limiter
les accès, chiffrer les copies exportées et définir une rétention et une copie
hors hôte. Le script ne chiffre pas les archives et ne les efface pas à échéance.

La restauration crée une **nouvelle base** à partir de `template0`, puis restaure
en une transaction. Une base existante est refusée ; la base d'origine n'est pas
modifiée. Après échec, inspecter la nouvelle base conservée. Vérifier comptes,
communautés, messages et `_sqlx_migrations`, puis arrêter l'API et modifier son
`PGDATABASE` dans un overlay explicite pour basculer. Conserver la base d'origine
tant que la recette n'est pas validée. Ne jamais utiliser `down --volumes` sur
une installation à conserver.

Sauvegarder séparément le secret PostgreSQL, le fichier d'environnement, les
volumes Caddy et la clé privée de fédération. La base seule ne restaure pas
l'identité cryptographique de l'instance. Les clés E2EE et l'état MLS restent
sur les appareils : une sauvegarde serveur ne récupère pas un historique privé
après perte du coffre client. Le guide [identité](windows-identity.md) décrit la
sauvegarde de racine et ses limites.

## Préparer A et B

Installer sur deux hôtes avec des domaines, projets, volumes et secrets distincts.
Générer une clé de fédération par instance avec la commande
`generate-federation-key` décrite dans [replication.md](replication.md).
Utiliser l'overlay `deployment/compose.federation.yaml` en plus du Compose de
production pour monter les clés et épingler les descripteurs de l'autre instance.
Les deux origines HTTPS doivent être joignables depuis les serveurs et clients.
La clé privée doit être lisible par l'UID 65534 du conteneur API.

## Paquet Windows

Sur Windows avec PowerShell 7 (`pwsh`, comme en CI), Rust MSVC, outils C++ et SDK Windows, Git et Python 3.11+ :

```powershell
./deployment/package-windows.ps1 -Configuration Release
```

`Debug` permet un paquet local plus rapide. Le script respecte
`CARGO_TARGET_DIR` et affiche le chemin d'un ZIP dans `dist/`, contenant desktop,
CLI d'identité, guides, notices, inventaire des dépendances et `SHA256SUMS`.
Les sommes couvrent récursivement tous les fichiers du paquet. Le ZIP contient
aussi `THIRD-PARTY-LICENSES/`, `SOURCE.md` et `corresponding-source.tar.gz`.
L'assemblage vérifie les suppléments de licences et refuse les textes manquants.
Il utilise Cargo vendor et peut télécharger les sources des autres plateformes.
Prévoir plusieurs Gio libres et quelques minutes supplémentaires. Ajouter à Git
les nouveaux fichiers du travail avant de produire un paquet de développement ;
la révision et les modifications locales sont déclarées dans l'inventaire.
Extraire le ZIP dans un dossier accessible puis lancer `kova-desktop.exe`.
Vérifier les sommes avant utilisation avec `Get-FileHash -Algorithm SHA256`.
Une mise à jour est manuelle : fermer Kova et extraire le nouveau ZIP ailleurs.
Le profil et le coffre restent séparés du dossier d'exécutables.

La signature Authenticode, un installeur, la mise à jour automatique et l'essai
sur un Windows vierge restent à valider avant publication destinée au grand
public. Aucun certificat d'éditeur n'est embarqué ou généré silencieusement.

## Matériel de source serveur et publication

Depuis le checkout exact utilisé pour construire le serveur, avec Git, Cargo et
Python 3.11+, produire un dossier neuf (un dossier existant est refusé) :

```sh
python deployment/compliance.py --output dist/server-source
```

La CI Linux conserve ce même matériel comme artefact `server-source-and-notices`.
Publier les sources et notices à côté de chaque binaire/image, à accès équivalent,
et conserver les versions déployées : les artefacts CI expirent. Le dossier
contient les sources Cargo verrouillées et les scripts Kova, avec instructions
de reconstruction hors ligne ; les outils et SDK restent des prérequis externes.
Ne pas distribuer une image modifiée sous la seule référence de la branche
principale. Les images actuelles sont construites par l'exploitant depuis ses
sources ; aucune image serveur Kova publique précompilée n'est livrée.

Pour un serveur AGPL modifié accessible par le réseau, afficher clairement aux
utilisateurs une offre d'accès aux sources de cette version. Le proxy peut servir
le dossier ci-dessus sur une URL publique dédiée, ou pointer vers un téléchargement
durable. Vérifier réellement ce lien avant ouverture. Conserver aussi les notices
et sources requises des images tierces (Debian/Caddy/PostgreSQL) si elles sont
redistribuées. Voir [la revue des licences](license-review.md) et
[le périmètre de publication](release-scope.md).

## Recette Alice/Bob

Sur deux sessions Windows distinctes, lancer le desktop packagé. Suivre les guides
[communautés](windows-communities.md), [chat](windows-chat.md) et
[MP](windows-dm.md) :

1. Alice crée son identité, se connecte à A et crée « Atelier ». Bob utilise une
   invitation avec sa propre identité. Envoyer des messages publics dans les deux
   sens, déconnecter/reconnecter Bob et vérifier l'historique sans doublon.
2. Ouvrir un MP, vérifier les empreintes des appareils par un canal authentifié,
   puis envoyer et déchiffrer les messages sur les deux desktops.
3. Les mêmes identités se connectent à B. Alice propose B, Bob examine et approuve
   la politique ; synchroniser A/B. Les salons d'Atelier restent locaux à A.
4. Arrêter A sans toucher ses volumes. Continuer le MP via B ; rouvrir le client
   avec un envoi en attente et vérifier son réessai unique. Relancer A et vérifier
   la convergence et le déchiffrement sur les deux clients.
5. Une instance C sans consentement ne reçoit pas le MP. Sauvegarder A, restaurer
   dans une nouvelle base et vérifier les données et migrations indépendamment.

Les preuves automatisées et les observations graphiques réellement exécutées
sont consignées dans [migration-plan.md](migration-plan.md). Une exécution du
cœur Rust ne remplace pas cette recette graphique connectée sur deux desktops.

### Fixture locale pour une recette reproductible

Sous Windows, avec Docker actif, Python et `cryptography` installés, construire
le serveur avec `cargo build -p kova-server --locked`, puis lancer :

```powershell
python crates/kova-server/tests/desktop_recipe.py
```

Le script crée deux PostgreSQL isolés, démarre deux serveurs natifs et prépare
deux profils synthétiques dans Credential Manager. Le test Rust ignoré
`desktop_recipe` crée sessions, communauté, salon et MP MLS avec consentement
bilatéral pour B. Le script affiche un chemin `target/desktop-recipe-…/state.json`
contenant uniquement les paramètres publics à saisir dans deux fenêtres du
desktop packagé. Le profil `default` reste séparé. La préparation par le test
n'est pas une observation graphique du parcours de création d'identité ou
d'approbation de réplica.

Créer un fichier vide `stop-a` dans ce dossier arrête A (`stopped-a` confirme).
Créer `start-a` le relance (`started-a` confirme). Créer `stop` termine la recette
et supprime ses conteneurs et volumes temporaires. Une échéance de 45 minutes
arrête également la fixture. Les profils synthétiques nommés dans `state.json`
restent locaux pour inspection ; ils ne servent pas à une installation réelle.

## Limites d'exploitation

Le Compose borne mémoire, nombre de processus et journaux (3 × 10 Mo par
service). Caddy borne les corps à 2 Mo, les limites plus fines restent appliquées
par l'API. Ces paramètres sont des valeurs initiales à mesurer, pas une preuve
de capacité. Les volumes PostgreSQL et les sauvegardes n'ont pas de quota disque
portable fourni par Compose : utiliser une partition/volume avec quota hôte,
surveiller `pg_database_size`, espace libre et WAL, et réserver de l'espace pour
la restauration. Aucun nettoyage automatique des messages n'est effectué.

Les limitations MP existantes demeurent : attestations de 24 heures, roster fixe,
sessions préalables sur B, réplication initiale avant panne, reprise limitée au
panneau ouvert. ARM64, autres desktops et accès HTTPS public doivent être testés
sur leur environnement réel avant d'être annoncés compatibles.
