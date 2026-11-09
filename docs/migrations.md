# Migrations proposées et compatibilité

Les migrations exécutables sont exclusivement dans `crates/kova-server/migrations`. `schema.sql` en donne une reproduction documentaire. Ne pas réécrire les migrations déjà déployées.

## Migration 0003 implémentée

`0003_desktop_sessions.sql` ajoute le transport des sessions et le mode des challenges, ainsi que les attestations et signatures de révocation. Les données historiques sont conservées ; les preuves absentes restent NULL. Les migrations 0001/0002 restent inchangées.

## Migration 0004 implémentée

`0004_identity_revocations.sql` invalide toutes les sessions, crée le journal monotone des révocations et reprend les preuves historiques présentes. Les preuves absentes ne sont pas inventées. Les autorisations v2 signent aussi leur date d’émission ; les anciennes autorisations doivent être renouvelées depuis la racine, même si leur échéance n’est pas atteinte. Sauvegarder la base avant mise à niveau. Les migrations 0001–0003 sont inchangées.

## Migration 0005 implémentée

La migration `0005_community_exclusions.sql` ajoute les exclusions persistantes par communauté et profil local, avec auteur et date. Elle ne modifie ni les sessions ni les données historiques. Les migrations 0001–0004 restent immuables. Les communautés historiques conservent leurs salons ; le salon `général` est garanti par la nouvelle transaction de création HTTP.

## Renommages futurs

Migrations futures à coordonner avec leurs écritures et lectures :

- `identities` → `global_identities`, avec version de format explicite ; `users` → `local_profiles`, `devices` → `device_authorizations`, `invites` → `invitations`, `messages` → `channel_messages`. Renommer en transaction, adapter toutes les requêtes/tests dans le même changement ; préserver UUID, clés et références.
- Les profils historiques non rattachés restent inauthentifiables ; conserver le rattachement hors bande.

## Jalons MP

La migration `0007_direct_messages.sql` crée `dm_key_packages`, `dm_conversations` et `dm_events`. Les deux participants sont des identités/appareils globaux dans le manifeste immuable. Le serveur stocke uniquement packages publics, Welcome et événements chiffrés signés ; les curseurs sont locaux. Les migrations 0001–0006 restent inchangées. Aucune autorisation de fédération n'est créée par cette migration.

La migration `0008_replica_policies.sql` ajoute les propositions et politiques signées, l'unicité de la politique active, les curseurs persistants par pair, les nonces anti-rejeu et les conflits de politique. Elle ne modifie ni les messages existants ni les migrations 0001–0007. Les conversations historiques restent locales jusqu'à approbation explicite d'une politique par les deux participants. Sauvegarder la base avant mise à niveau.

Chaque migration doit passer sur base vide et sur base historique peuplée, avec préservation des messages/adhésions, validation des contraintes et restauration d'une sauvegarde. Les tests de persistance existants couvrent la transition 0001→0005 ; le test séparé `backup_restore.py` vérifie désormais un pg_dump complet restauré dans une autre base.
