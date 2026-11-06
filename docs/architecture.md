# Domaines et dépendances

```mermaid
flowchart TB
    UI[Desktop GPUI — à créer] --> Core[Cœur Rust : coffre, SQLite, outbox — à créer]
    Core --> Protocol[kova-protocol : formats publics]
    Core --> Identity[kova-identity : vérification publique]
    Core --> Crypto[E2EE client — bibliothèque à sélectionner]
    Core --> A[Instance A : serveur modulaire]
    Core --> B[Instance B : serveur modulaire]
    A --> Protocol
    A --> Identity
    A --> Community[Communautés et salons locaux A]
    A --> DM[Enveloppes MP autorisées — à créer]
    B --> Replica[Réplica MP autorisé — à créer]
    DM <--> Replica
```

Identité : indépendante de l'hébergeur, racine privée côté client. Communauté : profils, adhésions et autorisations locales. MP : participants globaux, consentement et événements chiffrés indépendants des communautés. Média : domaine réservé, hors MVP.

Les bibliothèques publiques ne dépendent ni du serveur ni de son stockage. Le serveur n'importe aucun module de génération ou stockage de secrets E2EE. La réplication future ne concerne que les conversations explicitement autorisées ; elle ne fédère pas les communautés.
