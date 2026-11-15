# Périmètre de publication — état après les jalons 0 à 8

Ce document consolide les preuves consignées dans [migration-plan.md](migration-plan.md), [windows-spike.md](windows-spike.md) et [distribution.md](distribution.md). Il distingue les observations locales, les validations rapportées et les preuves encore attendues. Une limite explicitement reportée ne vaut pas validation.

## Plateformes et forme de livraison

- **Client officiellement visé par le prototype :** Windows x86-64 avec Rust MSVC. Le code GPUI 0.2.2 a été compilé, les tests du cœur client et le coffre Credential Manager ont été exercés sous Windows, et le parcours distribué du jalon 8 a été observé avec deux processus desktop sur une session Windows.
- **Paquet :** ZIP de développement Windows non signé, avec guides, licences, inventaire des dépendances et sommes SHA-256. Un ZIP Debug extrait a été exécuté au jalon 8. Au jalon 9, un nouveau ZIP Debug inclut aussi les sources et notices tierces : sommes, correspondance du binaire et résolution Cargo hors ligne depuis l'archive source vérifiées. La signature Authenticode, un installeur, la mise à jour automatique et une recette sur Windows vierge ne sont pas validés.
- **Serveur :** conteneurs Linux x86-64 avec PostgreSQL 17 et Docker Compose ; suites Linux distantes sur `b828466` et essais Docker locaux. Compose de production avec Caddy/HTTPS a été testé localement avec des certificats de test, deux instances PostgreSQL indépendantes et restauration. Aucun certificat ACME public ni accès depuis un réseau externe n'a été vérifié. La préparation guidée d'un hôte Linux propre reste au jalon 13.
- **Non annoncés compatibles :** macOS, Linux desktop et ARM64 (client ou hôte serveur). Les constructions croisées ou images multi-architecture ne remplacent pas une exécution sur matériel et OS cibles.

## Preuves consolidées

Les jalons 2 à 7 ont des bilans de clôture dans [migration-plan.md](migration-plan.md). Le jalon 8 a aussi un bilan local détaillé, comprenant tests Windows MSVC, scénarios Docker et recette graphique Alice/Bob packagée. Le bilan du jalon 0 atteste les essais manuels rapportés par le propriétaire (défilement, saisie, copier-coller et persistance du brouillon). Il ne couvre pas les notifications Windows.

Le jalon 1 possède des preuves locales historiques de workspace, migrations, persistance et restauration dans les suites consignées aux jalons ultérieurs. Les preuves restent attachées à leurs dates et révisions ; elles ne prouvent pas une exécution CI sur la révision de clôture du jalon 9.

| Jalon résiduel | Statut consolidé | Preuve ou reste affecté |
|---|---|---|
| 0 — spike GPUI | Socle Windows observé et retours manuels consignés | Notifications au 12 ; essai du paquet sur Windows propre et publication au 14 ; autres desktops hors support annoncé. |
| 1 — workspace et stockage | Preuves locales et job Linux distant réussis sur `b828466` | CI Windows complète de la révision retenue au 9 ; ARM64 demande une validation dédiée avant extension du support. |
| 8 — distribution du prototype | Recette locale clôturée, limites conservées | Deux processus Windows packagés, instances/bases indépendantes, panne/reprise ; deux machines physiques, accès public et publication finale aux 13/14. |

## Écarts affectés aux jalons suivants

| Écart | Affectation | Portée |
|---|---|---|
| Notifications Windows, non-lus et réception indépendante du panneau | Jalon 12 | Aucun comportement de notification n'est revendiqué avant validation de ce jalon. |
| Signature d'éditeur, validation de publication, mise à jour et recette sur Windows propre | Jalon 14 | Le ZIP actuel reste une distribution de développement non signée. |
| Vérification HTTPS depuis Internet et certificat ACME public | Jalon 13 puis recette intégrée du jalon 14 | Les tests HTTPS locaux ne prouvent pas la joignabilité externe. |
| CI distante réussie sur la révision retenue | Jalon 9 | Sur `b828466`, le job Linux a réussi ; le job Windows échoue après son test de packaging (code d'erreur simulé conservé). Voir [l'exécution](https://github.com/Kovanderhaege/kova/actions/runs/43735416464). Le correctif et les nouveaux artefacts nécessitent un nouveau passage. |
| Conformité complète des licences et notices, droits sur contributions/marque et accès aux sources | Jalon 9 pour l'inventaire et la revue ; jalon 14 avant publication | La répartition de licences est décidée. La décision documentée ne constitue pas un audit juridique ni une preuve d'accès aux sources correspondant aux binaires. |
| Exécution du desktop sur macOS/Linux et hôtes ARM64 | Hors périmètre de support MVP actuel | Exiger des validations dédiées avant toute annonce de compatibilité. |

## Avertissements connus

Les tests et Clippy exécutés pendant le jalon 9 sous Windows MSVC/Rust 1.99.1 réussissent. Clippy conserve six avertissements de style (emprunt inutile, itération sur les valeurs d'une map, deux conditions imbriquées et deux tests manuels de divisibilité), sans échec de compilation. L'import inutilisé d'un ancien bilan n'est pas signalé par cette exécution.

`cargo report future-incompatibilities --id 1` identifie précisément `proc-macro-error2 2.0.1` : réexport public de l'extern crate privé `proc_macro` (E0365, [suivi Rust](https://github.com/rust-lang/rust/issues/127909)). La dépendance transitive compile aujourd'hui mais pourra bloquer une future version de Rust. Suivre son correctif amont avant changement de toolchain ; aucune mise à jour générale ou correction locale du code tiers n'est introduite ici. Ce rapport ne démontre pas une vulnérabilité exploitable du client.

## État de sortie du jalon 9

Le jalon 9 reste **en cours** tant que la CI distante n'a pas réussi sur la révision retenue et que l'inventaire de distribution n'a pas été vérifié sur cette même révision. Les preuves locales historiques des jalons 0 à 8 sont consolidées, mais ne doivent pas être présentées comme passage de CI ou conformité juridique. Le jalon 9 peut être clôturé lorsque le commit de clôture indique le résultat et le lien de la CI distante, que les six crates et les paquets sont rapprochés des licences/textes/notices/source, et que les écarts résiduels ci-dessus restent affectés à leurs jalons.
