# Kova — Décisions de licence et modèle économique

Décisions initiales du 6 novembre 2026, répartition complétée le 15 novembre 2026. Cette politique remplace les orientations proposées dans la [comparaison des licences](license-comparison.md).

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
| Client desktop Rust (GPUI) et cœur applicatif client | GPL-3.0-only |
| Protocole public et bibliothèques indépendantes d'interopérabilité | Apache-2.0 |
| Bibliothèques indépendantes d'identité et de cryptographie | Apache-2.0 |

**Objectif du copyleft :** garantir l'accès aux sources des versions modifiées distribuées et, pour le serveur AGPL, des versions modifiées utilisées à travers un réseau.

Les forks, l'usage commercial et l'auto-hébergement professionnel restent autorisés sans redevance obligatoire.

### Motifs de la répartition

- **Desktop et `kova-client-core` : GPL-3.0-only.** Le copyleft porte sur le client distribué et son cœur applicatif (sessions, coffre, cache et synchronisation), y compris la CLI d'identité fournie par ce crate. Le cœur n'est pas une simple définition de protocole : le placer sous Apache permettrait sa réutilisation dans un client propriétaire. GPL correspond ici au modèle d'application installée ; AGPL reste réservée au serveur, où l'accès aux versions modifiées par le réseau est un objectif explicite. Un futur service hébergé réutilisant le cœur devra faire l'objet d'une décision distincte.
- **`kova-protocol`, `kova-identity`, `kova-crypto` : Apache-2.0.** Ces bibliothèques indépendantes définissent ou implémentent les formats, la vérification d'identité et l'intégration MLS, sans importer le serveur ou le cœur applicatif. Leur réutilisation, y compris dans des clients propriétaires, est intentionnellement permise pour favoriser l'interopérabilité. La licence Apache inclut des dispositions explicites sur les brevets. Ce choix s'applique au code Kova ; il ne change pas les licences d'OpenMLS, de ring ou des autres dépendances.
- **Serveur : AGPL-3.0-only, inchangée.** Conserver le copyleft adapté aux versions modifiées utilisées à travers un réseau. La licence du serveur ne s'applique pas automatiquement à un client indépendant du seul fait de ses échanges HTTP/WebSocket.
- **Versions précises :** conserver `-only`, en cohérence avec le serveur existant. Pas de double licence, de clause commerciale ou d'expression SPDX `OR` ajoutée implicitement. Les futures crates recevront une décision explicite plutôt qu'une licence héritée du workspace.

Les dépendances doivent suivre les frontières : les bibliothèques Apache ne doivent pas incorporer de code du cœur GPL ou du serveur AGPL en conservant une déclaration Apache inchangée. La licence de chaque crate ne suffit pas à déterminer les obligations du binaire combiné. La compatibilité Apache-2.0 vers GPLv3 est documentée par l'Apache Software Foundation ; cela ne constitue pas une validation de toutes les dépendances transitives de Kova.

### Sources et vérification

Références consultées le 15 novembre 2026 : [GPLv3](https://www.gnu.org/licenses/gpl-3.0.html), [AGPLv3, section 13](https://www.gnu.org/licenses/agpl-3.0.html), [FAQ GNU](https://www.gnu.org/licenses/gpl-faq.html), [compatibilité Apache-2.0/GPLv3](https://www.apache.org/licenses/GPL-compatibility), [texte Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0). Les versions utilisées de [GPUI 0.2.2](https://docs.rs/crate/gpui/0.2.2/source/Cargo.toml) (Apache-2.0) et d'OpenMLS 0.8.1 (MIT, voir la décision E2EE) ne motivent pas de changement de dépendance. Les textes sont conservés à la racine dans `LICENSE-GPL-3.0`, `LICENSE-AGPL-3.0` et `LICENSE-APACHE-2.0` ; `LICENSE.md` indique leur périmètre.

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
3. Conserver les textes de licence et la répartition par crate dans les distributions ; préserver également les licences et attributions tierces.
4. Ne pas ajouter de mécanisme de paiement, de limitation fonctionnelle ou de vérification de licence.
5. Ne pas confondre open source et gratuité des services d'hébergement.

## Application au workspace

| Crate | Champ Cargo `license` | État |
|---|---|---|
| `kova-server` | `AGPL-3.0-only` | Conservé |
| `kova-protocol` | `Apache-2.0` | Conservé |
| `kova-desktop` | `GPL-3.0-only` | Retenu le 15 novembre 2026 |
| `kova-identity` | `Apache-2.0` | Retenu le 15 novembre 2026 |
| `kova-client-core` | `GPL-3.0-only` | Retenu le 15 novembre 2026 |
| `kova-crypto` | `Apache-2.0` | Retenu le 15 novembre 2026 |

Aucune licence commune n'est héritée au niveau du workspace. Les six crates restent marquées `publish = false` ; leur licence ne constitue pas une autorisation de publication automatique. Les notices de dépendances existantes sont conservées.

Avant publication, vérifier la compatibilité des dépendances et programmes combinés, les droits sur le code existant, la politique de contributions, les notices complètes du paquet et les droits sur le nom et le logo. Prévoir l'accès au code source correspondant aux binaires distribués et l'offre de source requise pour le serveur AGPL modifié utilisé par le réseau. Cette décision ne constitue pas un audit juridique des dépendances.

**État :** répartition des six crates arrêtée et encodée dans Cargo ; vérifications de conformité des distributions encore nécessaires avant publication.
