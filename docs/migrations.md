# Migrations proposées et compatibilité

Les migrations exécutables sont exclusivement dans `crates/kova-server/migrations`. `schema.sql` en donne une reproduction documentaire. Ne pas réécrire 0001/0002 déjà déployées.

## Migration 0003 implémentée

`0003_desktop_sessions.sql` ajoute le transport des sessions et le mode des challenges, ainsi que les attestations et signatures de révocation. Les données historiques sont conservées ; les preuves absentes restent NULL. Les migrations 0001/0002 restent inchangées.

## Étapes suivantes du jalon identité

Migrations futures à coordonner avec leurs écritures et lectures :

- `identities` → `global_identities`, avec version de format explicite ; `users` → `local_profiles`, `devices` → `device_authorizations`, `invites` → `invitations`, `messages` → `channel_messages`. Renommer en transaction, adapter toutes les requêtes/tests dans le même changement ; préserver UUID, clés et références.
- Ajouter le journal signé versionné d'identité après définition de sa politique de fraîcheur. Les profils historiques non rattachés restent inauthentifiables ; conserver le rattachement hors bande.

## Jalons MP

Créer séparément dm_conversations, dm_participants, dm_events (event_id unique, enveloppe/signature/dépendances), dm_replica_policies, dm_replica_approvals et les tables de jobs/curseurs. Attendre la décision E2EE, les formats canoniques et la politique de concurrence avant de fixer colonnes et index. Ne pas stocker de clé privée ni utiliser users.id comme identité de participant global.

Chaque migration doit passer sur base vide et sur base historique peuplée, avec préservation des messages/adhésions, validation des contraintes et restauration d'une sauvegarde. Les tests de persistance existants couvrent la transition 0001→0003 ; le test séparé `backup_restore.py` vérifie désormais un pg_dump complet restauré dans une autre base.
