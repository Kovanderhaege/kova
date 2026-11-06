# Prototype Windows avant jalon 3

## Périmètre

Cible initiale : Windows x86-64, Rust MSVC, GPUI publié 0.2.2. `apps/desktop` contient une fenêtre, 10 000 lignes virtualisées, un éditeur adapté de l'exemple officiel GPUI avec sélection/Unicode/presse-papiers et sauvegarde explicite d'un brouillon SQLite. C'est un prototype local ; il ne présente aucun faux chat connecté.

`kova-client-core` contient une identité racine/appareil générée localement, les signatures compatibles serveur, la validation du contexte des challenges, une API HTTP sans redirections, des sessions liées à l'origine et un cache SQLite sans secrets. Sur Windows, `keyring` utilise le gestionnaire d'identifiants natif pour l'identité et les sessions. Il n'y a aucun repli vers un fichier en clair.

## Blocage constaté sur cette machine

Rust MSVC 1.99.1 et rustfmt sont installés pour l'utilisateur, sans modifier le PATH système. Visual Studio Community est présent mais les outils C++/SDK requis sont absents. `cargo check -p kova-desktop --locked` échoue faute de `link.exe`. L'installateur Visual Studio a refusé l'installation silencieuse avec le code 5007 : cette commande exige un lancement administrateur. Aucune élévation n'a été déclenchée.

Dans Visual Studio Installer, installer **Développement Desktop en C++**, avec MSVC x64/x86 et Windows SDK. Après cela, dans un terminal disposant de Cargo :

```powershell
cargo run -p kova-desktop --locked
cargo test -p kova-client-core --locked
cargo build -p kova-desktop --release --locked
```

## Validation manuelle à effectuer sur Windows

- Fenêtre native, redimensionnement et fermeture, GPU/DPI, liste de 10 000 lignes fluide.
- Saisie française, caractères composés, emoji et IME ; sélection au clavier et à la souris.
- Ctrl+A/C/V/X, presse-papiers entre applications, collage multiligne : consigner les limites de l'éditeur monoligne.
- Enregistrer un brouillon, fermer puis relancer : contenu conservé dans `%LOCALAPPDATA%/Kova/spike/cache.sqlite`.
- Notifications Windows : **non implémentées/non validées** ; reste du jalon 0.
- ZIP de développement : script fourni, exécution sur une machine sans outils de compilation à prouver. Signature, installeur et mises à jour restent au jalon distribution.

## Client d'identité Windows de diagnostic

```powershell
cargo run -p kova-client-core --bin kova-identity -- init
# La commande affiche uniquement la clé publique racine pour bootstrap-admin.
cargo run -p kova-client-core --bin kova-identity -- login http://127.0.0.1:8080
cargo run -p kova-client-core --bin kova-identity -- me http://127.0.0.1:8080
cargo run -p kova-client-core --bin kova-identity -- logout http://127.0.0.1:8080
```

`init` refuse de remplacer une identité déjà présente. Ce coffre est destiné au prototype : restauration/export chiffré et appairage ne sont pas encore disponibles. Ne pas l'utiliser pour une identité irremplaçable. Le binaire affiche le profil public, jamais les jetons. Les clés et sessions sont dans le coffre Windows ; la CLI Linux échoue explicitement. L'UI de connexion n'est pas encore reliée à ce client de diagnostic.

La preuve du jalon 0 reste ouverte tant que les essais natifs, notifications et packaging ne sont pas réalisés. Une compilation croisée ou une CI qui compile ne remplace pas ces essais.

## Résultats obtenus pendant cette passe

- `cargo check -p kova-desktop --target x86_64-pc-windows-gnu --locked` : succès dans un conteneur Rust avec MinGW.
- `cargo build -p kova-desktop --target x86_64-pc-windows-gnu --locked` : succès ; exécutable local dans `target/x86_64-pc-windows-gnu/debug/kova-desktop.exe`. Cette chaîne GNU est un moyen de vérifier le prototype, pas un remplacement décidé de MSVC pour la distribution.
- L'inspection des imports PE n'a montré que des DLL système Windows ; ce contrôle statique ne prouve pas l'exécution sur un Windows vierge.
- Contrôle graphique impossible : le moteur Computer Use échoue à initialiser ses fichiers (`failed to write kernel assets`, chemin introuvable), y compris après réinitialisation et via le second point d'entrée. Aucun essai visuel, IME, presse-papiers, notifications ni coffre natif n'est donc déclaré réussi.
- Le compilateur signale une méthode `reset` inutilisée dans l'éditeur adapté et un avertissement de compatibilité future dans la dépendance `proc-macro-error2`. Ces avertissements ne bloquent pas la construction testée.

Le job CI Windows/MSVC est préparé mais n'a pas été exécuté à distance. Le ZIP MSVC n'a pas été produit localement. Ne pas confondre l'exécutable de diagnostic GNU avec une distribution signée du MVP.
