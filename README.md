# Kova

Kova est un projet d’alternative auto-hébergeable à Discord. L’idée est de permettre à chaque communauté d’héberger ses discussions sur son propre ordinateur ou serveur et de garder la maîtrise de ses données.

## Première version prévue

Depuis un navigateur, on pourra créer une communauté, inviter ses amis et discuter en temps réel dans des salons textuels. L’historique sera conservé sur la machine qui héberge la communauté.

## État du projet

Le socle Rust/Axum démarre avec PostgreSQL et applique la [migration initiale](backend/migrations/0001_initial.sql). Le deuxième jalon ajoute le premier compte local et les sessions HTTP.

## Démarrage local

Avec Docker et la variable d’environnement `POSTGRES_PASSWORD` définie, lancer depuis la racine du dépôt :

```sh
docker compose -f deployment/compose.yaml up -d --build --wait
```

[La sonde de disponibilité](http://127.0.0.1:8080/health/ready) renvoie HTTP 200 lorsque le serveur peut interroger la base. Pour arrêter les services en conservant les données, utiliser `docker compose -f deployment/compose.yaml down`.

Le test de persistance se lance avec `python backend/tests/persistence.py`. Il crée une instance temporaire, écrit un message, recrée les conteneurs et relit le message.

## Premier compte et sessions

Une fois les services démarrés, créer le premier compte depuis un terminal local avec la commande suivante. Saisir l’identifiant, le nom affiché puis le mot de passe au prompt masqué. La commande refuse de créer un autre compte dès que la base contient un utilisateur.

```sh
docker compose -f deployment/compose.yaml exec api kova-server create-first-user
```

Pour ouvrir une session, envoyer à `POST /api/v1/auth/login` un corps JSON contenant `handle` et `password`. Le client conserve le cookie renvoyé et le présente à `GET /api/v1/me` pour lire son profil. `POST /api/v1/auth/logout` prend un corps vide et révoque uniquement la session présentée. Chaque connexion expire au bout de sept jours.

Les deux requêtes POST exigent un en-tête `Origin` égal à l’origine publique de l’API. Compose utilise `http://127.0.0.1:8080` par défaut et adapte cette valeur à `KOVA_API_PORT`. Pour un service derrière un proxy HTTPS, définir `KOVA_ORIGIN` sur l’origine HTTPS exacte du navigateur. Le cookie porte alors aussi l’attribut `Secure`.

Exécuter `python backend/tests/sessions.py` pour tester le premier compte et le parcours de deux clients HTTP avec des cookies indépendants. Le test utilise une base temporaire et avance l’échéance de la session restante pour vérifier le refus immédiat après expiration.
