# Identité sur Windows

Construire avec Rust MSVC, les outils C++ et le SDK Windows : `cargo run -p kova-desktop --locked`. La fenêtre d’identité est le démarrage normal ; `cargo run -p kova-desktop -- --spike` conserve la démonstration de liste et de brouillon. Ce client de développement n’est pas un installateur signé.

## Créer et sauvegarder

1. Choisir un profil local (`default` par défaut). Un nom différent crée un espace de coffre séparé ; changer ce champ ne supprime rien.
2. **Identité → Créer une identité**. La racine et la clé d’appareil restent dans Credential Manager. **Afficher l’identité** et **Copier le résultat public** donnent les identifiants et la clé publique racine, jamais les secrets.
3. **Sauvegarde** : fournir un chemin absolu nouveau et une phrase secrète longue, puis **Créer la sauvegarde**. Conserver le fichier `.age` et sa phrase séparément. Le serveur ne peut pas les récupérer.
4. Pour restaurer, choisir un profil vide, le fichier existant et sa phrase, puis **Restaurer dans ce profil vide**. L’identité globale est conservée mais une nouvelle clé d’appareil est créée. Révoquer les anciens appareils perdus séparément. Un profil existant ou un fichier existant à l’export n’est jamais écrasé.

## Rejoindre une instance

Dans **Connexion**, saisir l’origine exacte, par exemple `http://127.0.0.1:8080` en local ou une origine HTTPS. Une première adhésion exige une invitation, un identifiant local et un nom affiché. L’administrateur initial peut être rattaché hors bande avec `kova-server bootstrap-admin` et la clé publique racine affichée. Pour se reconnecter, laisser l’invitation vide.

Les sessions et profils sont propres à chaque instance. **Lister les appareils** montre les clés publiques et preuves connues de cette instance. **Se déconnecter** invalide la session sur cette origine et retire son jeton local. Les opérations réseau et de chiffrement tournent en arrière-plan.

## Associer un autre appareil ou renouveler

1. Sur le nouvel appareil/profil vide : **Demande**, choisir un nouveau fichier `.json`, créer la demande. Elle expire après 10 minutes. Sa clé privée reste dans son coffre Windows.
2. Transmettre le fichier public à l’appareil racine et comparer l’empreinte complète par un canal de confiance (par exemple en personne). Sur la racine : **Autoriser**, renseigner le fichier, l’empreinte comparée et un nouveau chemin de réponse, puis confirmer.
3. Transmettre la réponse publique au nouvel appareil. Dans **Associer**, renseigner ce fichier et la clé publique racine comparée à celle de l’appareil de confiance, puis confirmer. Une autre demande, racine ou signature est refusée.
4. L’appareil associé reçoit une autorisation de 24 h au maximum, jamais la racine. Pour la renouveler, reprendre les mêmes étapes dans le même profil associé ; sa clé d’appareil est conservée. Sans autorisation valide, il ne peut plus se connecter.

## Révoquer et transmettre

Sur le profil racine, copier la clé publique concernée depuis **Lister les appareils**, puis **Révocation → Révoquer définitivement**. La révocation est permanente pour cette clé. La preuve publique est conservée en SQLite et envoyée aux instances connues. Les échecs sont affichés ; utiliser **Réessayer la transmission** après retour du réseau. Une connexion réussie récupère aussi les révocations signées connues du serveur et retransmet l’union locale.

Un profil restauré doit se reconnecter aux instances concernées pour retrouver leurs adresses et journaux ; ils ne font pas partie de la sauvegarde de racine. Une instance non connue/inaccessible peut accepter l’appareil jusqu’à expiration de son certificat (24 h maximum). Il n’y a pas de propagation automatique serveur-à-serveur à ce stade.

## Mise à niveau

La migration 0004 invalide les sessions existantes. Les certificats d’appareil v1, dont ceux de 90 jours, sont refusés ; réautoriser depuis la racine avec le client actuel. Ne pas supprimer son coffre racine. Les données de profils, communautés et messages restent conservées. Une sauvegarde de base préalable reste nécessaire.
