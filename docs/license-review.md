# Revue de distribution — jalon 9

Revue technique du 15 novembre 2026, sans changement de la répartition des six
crates décidée dans [licensing.md](decisions/licensing.md). Les déclarations
Cargo concordent avec `LICENSE.md` et les textes GPLv3, AGPLv3 et Apache-2.0
présents à la racine. La documentation et les outils propres au dépôt ont aussi
une licence explicite. La politique de [contribution](../CONTRIBUTING.md) exige
les droits nécessaires et la provenance du code tiers.

## Dépendances et notices

Le verrouillage contient 780 paquets tiers. `deployment/compliance.py` inventorie
tout le workspace avec les expressions de licence déclarées ; ce sur-ensemble
inclut les builds, tests et plateformes étrangères. Il copie sans modification
les fichiers LICENSE/LICENCE/COPYING/NOTICE/COPYRIGHT, y compris ceux du code
natif embarqué (ring, SQLite, etc.), et les `license_file` explicites. Les
textes omis des archives Cargo sont complétés depuis leur dépôt officiel à la
révision indiquée par `.cargo_vcs_info.json` lorsqu'ils y sont disponibles, avec
provenance et SHA-256. Certains dépôts n'ont ajouté leurs textes que plus tard :
le supplément indique alors explicitement cette révision ultérieure et la
concordance de la licence déclarée, sans l'annoncer comme celle du paquet.
Lorsque seule une déclaration de licence est fournie, le texte canonique de
référence et la déclaration originale sont conservés avec cette distinction.
Aucun titulaire ni année de copyright n'est fabriqué ; les notices absentes
sont signalées dans l'inventaire pour revue avant publication.
Les 82 suppléments se répartissent en 66 révisions exactes, sept révisions
amont ultérieures et neuf archives crates.io vérifiées contre `Cargo.lock`
accompagnées de références canoniques. Douze dossiers n'ont pas de mention
concrète de titulaire de copyright ; ce constat n'est pas transformé en notice
inventée et reste visible via `copyright_notice_missing`.
Une déclaration ou des textes manquants bloquent l'assemblage.

Les familles recensées sont MIT, Apache-2.0 (dont LLVM-exception), BSD, ISC,
0BSD, BSL-1.0, CC0, MIT-0, Unlicense, Zlib, Unicode-3.0, NCSA,
CDLA-Permissive-2.0 et MPL-2.0. Pour les alternatives, une branche permissive
est disponible : notamment Apache-2.0 pour `Apache-2.0 OR GPL-2.0-only`, et
MIT/Apache pour les expressions proposant LGPL. Les anciennes notations `/`
sont conservées dans l'inventaire, sans les réécrire en SPDX moderne.
Les obligations cumulatives `AND` restent cumulatives ; les textes sont tous
conservés. Cette revue n'ajoute pas de dépendance ni de parseur de licences.

Les fichiers MPL (`cbindgen`, `hpke-rs` et ses fournisseurs) sont livrés avec
leurs sources non modifiées et leurs textes MPL. Les codes sous MPL restent
sous leurs termes ; l'archive fournit leur forme source, même lorsqu'ils ne
sont pas liés au programme livré. Les racines de confiance `webpki-roots` ont
leur texte CDLA. Le code GPUI adapté est attribué dans `src/input.rs` et dans
la notice desktop corrigée ; son texte Apache est conservé.

Le scan de métadonnées ne prouve pas à lui seul les droits de chaque auteur,
la compatibilité de chaque objet lié ou la conformité d'un futur asset. Les
sources complètes conservent également les mentions présentes dans les fichiers
eux-mêmes. Tout changement du verrouillage exige une nouvelle revue.

## Sources et publication

Le ZIP Windows embarque les notices et `corresponding-source.tar.gz` : fichiers
Kova suivis par Git dans leur état au build, lockfile, scripts et dépendances
fournies par `cargo vendor --locked`. L'inventaire lie la révision de base,
l'état modifié et les empreintes des fichiers réellement fournis ; un répertoire
de travail modifié ne doit pas être annoncé comme le commit seul. Les nouveaux
fichiers doivent être ajoutés à l'index avant génération. `SOURCE.md` décrit
la reconstruction hors ligne et les outils/SDK externes nécessaires. Aucun
build reproductible octet pour octet n'est revendiqué.

La CI Linux produit aussi `server-source-and-notices` pour la même révision que
les tests. Les images serveur actuelles sont construites par l'exploitant depuis
son checkout ; aucune image Kova publique précompilée n'est publiée ici. Toute
distribution ultérieure d'une image doit conserver les licences de son système
de base et joindre les sources correspondantes. Caddy, PostgreSQL et Debian
restent des distributions tierces avec leurs propres sources et licences.
Les outils Python de recette (`cryptography`, `websockets`) sont des prérequis
de test, sans inclusion dans les exécutables Windows.

Les artefacts CI sont temporaires, pas un hébergement permanent de publication.
Publier sources, notices et binaires ensemble au même emplacement avec accès
équivalent, et conserver les versions déployées. Un exploitant d'un serveur
AGPL modifié doit proposer clairement ses sources aux utilisateurs distants
(section 13), par exemple par un lien public servi via son proxy. Un lien vers
la branche courante ne remplace pas les sources de sa version modifiée.

Avant la publication du jalon 14, le responsable vérifie les droits des
contributions historiques et des noms/assets, les notices du paquet final,
l'accès public durable aux sources et les bibliothèques système de l'image
réellement distribuée. Aucun registre de marque, contrat de travail ou attestation
d'auteur n'est disponible dans ce dépôt : la présente revue ne les invente pas.

## Références

Sources primaires consultées le 15 novembre 2026 :
[Cargo vendor](https://doc.rust-lang.org/cargo/commands/cargo-vendor.html),
[Apache-2.0, section 4](https://www.apache.org/licenses/LICENSE-2.0),
[FAQ MPL, distribution et sources](https://www.mozilla.org/en-US/MPL/2.0/FAQ/).
Les textes GPL/AGPL locaux (`LICENSE-GPL-3.0`, sections 1 et 6 ;
`LICENSE-AGPL-3.0`, section 13) ont été relus ; leurs pages GNU n'ont pas pu
être rechargées dans le navigateur pendant cette passe. Les sources exactes
des compléments tiers sont consignées avec les textes, et non réinventées ici.
