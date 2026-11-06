# Kova — Décisions de licence et modèle économique

Décisions consignées le 6 novembre 2026. Cette politique remplace les orientations proposées dans la [comparaison des licences](license-comparison.md).

## Vision

Kova est une alternative à Discord, entièrement open source, gratuite et auto-hébergeable, centrée sur la confidentialité et la souveraineté des données.

**Principes non négociables :**
- Code source intégralement public et auditable.
- Aucune infrastructure centrale obligatoire.
- Aucune collecte de données ou télémétrie obligatoire.
- Aucun financement par la publicité ou l'exploitation des données personnelles.
- Aucune fonctionnalité essentielle réservée aux utilisateurs payants.
- Gratuité du logiciel pour les particuliers, associations et entreprises.
- Identités portables, MP chiffrés de bout en bout et réplication contrôlée par les participants.

## Licences retenues

| Composant | Licence cible |
|---|---|
| Serveur Rust (Axum) | AGPL-3.0-only |
| Client desktop Rust (GPUI) | GPL-3.0-only ou AGPL-3.0-only — à arbitrer |
| Protocole public et bibliothèques indépendantes d'interopérabilité | Apache-2.0 |
| Autres crates partagées | À déterminer selon leurs dépendances et leur intégration |

**Objectif du copyleft :** garantir l'accès aux sources des versions modifiées distribuées et, pour le serveur AGPL, des versions modifiées utilisées à travers un réseau.

Les forks, l'usage commercial et l'auto-hébergement professionnel restent autorisés sans redevance obligatoire.

## Modèle économique

Le logiciel et toutes ses fonctionnalités restent gratuits.

Les revenus éventuels proviennent exclusivement de services facultatifs :
- Hébergement Kova géré.
- Installation, déploiement et migration.
- Maintenance, mises à jour et support professionnel.
- SLA et accompagnement opérationnel.
- Intégrations et développements spécifiques.

Aucune licence payante imposée selon le nombre d'utilisateurs, la taille de l'entreprise ou l'usage commercial.

## Contraintes techniques et juridiques

- Ne pas introduire de télémétrie centrale obligatoire, de DRM, de contrôle de licence ou de dépendance à un service commercial.
- Préserver la possibilité d'utiliser Kova intégralement hors des services officiels.
- Maintenir des protocoles documentés et interopérables.
- Distinguer confidentialité du contenu et confidentialité des métadonnées : l'E2EE ne masque pas automatiquement les métadonnées.
- Vérifier la compatibilité des dépendances et des licences des programmes combinés, notamment libsignal (AGPL).
- Préserver les notices et mentions de copyright.
- Définir une politique de contributions garantissant les droits nécessaires à la distribution.
- Traiter séparément les droits sur le nom et le logo Kova.

## Instructions pour le dépôt

1. Documenter ces décisions dans `docs/decisions/licensing.md`.
2. Utiliser les identifiants SPDX exacts dans les manifests Cargo concernés.
3. Ajouter les fichiers `LICENSE` appropriés uniquement lorsque la répartition définitive des licences est validée.
4. Ne pas ajouter de mécanisme de paiement, de limitation fonctionnelle ou de vérification de licence.
5. Ne pas confondre open source et gratuité des services d'hébergement.

## Application au workspace

| Crate | Champ Cargo `license` | État |
|---|---|---|
| `kova-server` | `AGPL-3.0-only` | Orientation retenue pour le serveur |
| `kova-protocol` | `Apache-2.0` | Orientation retenue pour le protocole indépendant |
| `kova-desktop` | Non renseigné | Arbitrage GPL-3.0-only / AGPL-3.0-only en attente |
| `kova-identity` | Non renseigné | Répartition à déterminer selon les dépendances et l'intégration |
| `kova-client-core` | Non renseigné | Répartition à déterminer selon les dépendances et l'intégration |

Les choix non tranchés ne sont pas encodés comme une expression SPDX `OR`, qui accorderait un choix de licence aux destinataires. Aucune licence commune n'est héritée au niveau du workspace. Les crates restent marquées `publish = false` ; les textes de licence Kova seront ajoutés après validation de la répartition définitive. Les notices de dépendances existantes sont conservées.

Avant publication, finaliser la licence desktop, la répartition des autres crates, la compatibilité des dépendances et programmes combinés, les droits sur le code existant, la politique de contributions et les droits sur le nom et le logo. Cette décision ne constitue pas un audit juridique des dépendances.

**État :** orientation AGPL serveur / Apache bibliothèques retenue ; licence desktop et compatibilité juridique à finaliser avant publication.
