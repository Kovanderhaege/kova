# Kova

Kova est un projet d’alternative auto-hébergeable à Discord. L’idée est de permettre à chaque communauté d’héberger ses discussions sur son propre ordinateur ou serveur et de garder la maîtrise de ses données.

## Première version prévue

Depuis un navigateur, on pourra créer une communauté, inviter ses amis et discuter en temps réel dans des salons textuels. L’historique sera conservé sur la machine qui héberge la communauté.

## État du projet

Kova est en phase de conception. Le dépôt contient un [premier schéma PostgreSQL](docs/schema.sql) et une [configuration Docker Compose](deployment/compose.yaml) pour lancer la base en local. Le développement du serveur Rust/Axum et de l’interface Vue 3 est la prochaine étape.
