# Domaines et dépendances

```mermaid
flowchart TB
    UI[Prototype Windows GPUI] --> Core[Cœur Rust : coffre, SQLite et envois en attente]
    Core --> Protocol[kova-protocol : formats publics]
    Core --> Identity[kova-identity : vérification publique]
    Core --> Crypto[kova-crypto : OpenMLS, secrets côté client]
    Core --> A[Instance A : serveur modulaire]
    Core --> B[Instance B : serveur modulaire]
    A --> Protocol
    A --> Identity
    A --> Community[Communautés et salons locaux A]
    A --> DM[Enveloppes MP signées et chiffrées]
    B --> Replica[Réplica MP autorisé]
    DM <-->|Consentement bilatéral et synchronisation authentifiée| Replica
```

Identité : indépendante de l'hébergeur, racine privée côté client. Communauté : profils, adhésions et autorisations locales. MP : participants globaux, consentement et événements chiffrés indépendants des communautés. Média : domaine réservé, hors MVP.

Les bibliothèques publiques ne dépendent ni du serveur ni de son stockage. Le serveur n'importe aucun module de génération ou stockage de secrets E2EE. Sa clé de fédération authentifie uniquement les transferts entre réplicas. La réplication ne concerne que les conversations explicitement autorisées ; elle ne fédère pas les communautés. Le cœur client sélectionne un réplica disponible parmi ceux approuvés par les deux participants. Le desktop ordonnance la synchronisation et les réessais de la conversation sélectionnée ; les sessions restent isolées par origine.
