# Prototype GPUI

`src/input.rs` adapte le composant de saisie de `gpui 0.2.2/examples/input.rs`, publié par Zed Industries sous Apache-2.0. Modifications : enveloppe Kova, raccourcis Windows, champs secrets et intégration aux panneaux. `src/spike.rs` utilise ce composant, une liste virtualisée et la sauvegarde locale du brouillon. Licence reproduite dans GPUI-LICENSE-APACHE.

Source : https://docs.rs/crate/gpui/0.2.2/source/examples/input.rs

Les contributions propres au desktop Kova sont sous GPL-3.0-only ; le code adapté de GPUI conserve ses attributions et sa licence Apache-2.0. Voir la répartition dans `LICENSE.md` à la racine.

Les ZIP contiennent `THIRD-PARTY-LICENSES/`, les textes et attributions amont (y compris les sous-répertoires de code natif), ainsi que `dependency-inventory.json` : noms, versions, expressions de licence déclarées, dépôts, auteurs et empreintes des notices. L'inventaire couvre tout le workspace verrouillé, y compris les dépendances de test, de build et des autres plateformes ; il ne décrit pas les seuls objets liés au binaire.

Les sources complètes des dépendances Cargo sont fournies sans modification dans `corresponding-source.tar.gz`, avec les sources Kova et scripts de build. Les fichiers MPL restent disponibles sous MPL-2.0. Voir `SOURCE.md` pour l'accès et la reconstruction ; les SDK/outils système sont des prérequis externes. Les suppléments de licence absents des archives Cargo sont associés à leurs URL et révisions amont, ou à leur texte canonique de référence, dans leur fichier de provenance. L'inventaire signale les copyrights non fournis par les paquets ; aucun titulaire ni année n'est inventé.
