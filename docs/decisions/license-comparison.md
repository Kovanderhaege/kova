# Choisir la licence de Kova

Comparaison préparée le 6 novembre 2026, conservée comme contexte historique. **La [politique de licence et de modèle économique](licensing.md) fait désormais référence** : orientation AGPL-3.0-only serveur / Apache-2.0 protocole retenue, licence desktop et autres crates à finaliser. Les propositions ci-dessous ne remplacent pas cette décision. Les licences des dépendances restent applicables indépendamment de ce choix.

## Comparaison pratique

| Licence | Fork propriétaire distribué | Modifications exploitées uniquement sur un serveur | Obligation principale | Intérêt pour Kova |
|---|---|---|---|---|
| MIT | Possible | Pas de partage imposé | Conserver copyright et licence | Très simple, mais peu de réciprocité |
| Apache-2.0 | Possible | Pas de partage imposé | Licence, notices applicables, mention des modifications ; concession explicite de brevets | Favoriser adoption et réutilisation |
| MPL-2.0 | Possible autour des fichiers couverts | Pas de partage imposé du seul fait de l'hébergement | Rendre disponibles les sources des fichiers MPL modifiés lorsqu'ils sont distribués | Compromis au niveau fichier |
| GPL-3.0 | Pas pour l'œuvre dérivée couverte | L'accès réseau seul ne déclenche pas la distribution | Fournir les sources correspondantes selon les modalités de distribution | Protéger les versions distribuées du client |
| AGPL-3.0 | Pas pour l'œuvre dérivée couverte | Une version modifiée accessible par réseau doit proposer ses sources correspondantes aux utilisateurs distants | Obligations GPL + clause réseau | Préserver le partage des améliorations chez les hébergeurs |

Sources primaires : [MIT — OSI](https://opensource.org/license/mit), [Apache, sections 3–4](https://www.apache.org/licenses/LICENSE-2.0), [FAQ MPL — Mozilla](https://www.mozilla.org/en-US/MPL/2.0/FAQ/), [GPLv3 — texte OSI](https://opensource.org/license/gpl-3.0), [AGPLv3 — texte OSI, section 13](https://opensource.org/license/agpl-3.0).

Toutes ces licences permettent l'usage commercial et la vente de services. « Gratuit pour les utilisateurs de Kova » peut être ton choix de distribution ; aucune de ces licences n'interdit à un tiers de facturer son hébergement. Le copyleft ne donne pas accès aux conversations, secrets ou données des utilisateurs : il porte sur le logiciel couvert.

## Trois choix cohérents

1. **AGPL-3.0 pour le projet** si ta priorité est que les utilisateurs des hébergeurs bénéficient des sources des améliorations. C'est ma recommandation conditionnelle pour une plateforme communautaire auto-hébergée qui veut cette réciprocité. Elle n'empêche pas les forks concurrents ni les offres payantes.
2. **Apache-2.0 pour le projet** si tu veux maximiser la réutilisation, y compris dans des offres propriétaires. La concession de brevets est un avantage par rapport à MIT, au prix de notices plus structurées.
3. **MPL-2.0 pour le projet** si tu veux conserver l'ouverture des fichiers modifiés distribués tout en permettant des modules propriétaires. Ce compromis ne protège pas contre les améliorations gardées privées par un hébergeur qui ne distribue pas son serveur.

Ce classement est une proposition de politique produit, pas une conclusion imposée par la technique. GPL seule répond moins bien au cas précis des versions serveur modifiées proposées comme service. Une répartition AGPL serveur / Apache bibliothèques et client peut être étudiée, mais demande des frontières et notices précises ; la séparation en crates ne suffit pas à elle seule à déterminer les obligations d'un programme combiné.

## Impact des dépendances pressenties

| Composant | Licence constatée | Conséquence avant engagement |
|---|---|---|
| GPUI 0.2.2 | Apache-2.0 | Prototype possible sans fixer la licence globale ; préserver notices et texte Apache |
| libsignal | AGPLv3 | Son intégration peut soumettre le programme combiné aux obligations AGPL ; ne pas supposer qu'un client propriétaire/permissif reste distribuable tel quel |
| OpenMLS | MIT | Choix généralement plus souple pour la licence de l'application ; ne tranche pas l'adéquation du protocole, l'audit ou le multi-appareils |

Sources : [manifeste GPUI](https://raw.githubusercontent.com/zed-industries/zed/main/crates/gpui/Cargo.toml), [libsignal officiel](https://github.com/signalapp/libsignal), [OpenMLS officiel](https://github.com/openmls/openmls). Le prototype utilise exactement la version publiée GPUI 0.2.2 ; libsignal/OpenMLS ne sont pas ajoutés au projet. Leurs versions et dépendances devront être figées et auditées lors du choix E2EE. Aucun audit de sécurité n'est déduit d'une licence.

## Après ton choix

Ajouter le texte de licence, les identifiants SPDX exacts (notamment `-only` ou `-or-later` pour GPL/AGPL), les notices par crate et les conditions des contributions. Vérifier les droits sur le code existant et les dépendances transitives du paquet livré. Un changement de licence futur sur des contributions externes peut nécessiter l'accord de leurs auteurs ; ne pas promettre une double licence commerciale sans ces droits.
