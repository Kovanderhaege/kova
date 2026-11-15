# Interface du MVP — jalon 11

## Maquette et parcours retenus

La maquette fonctionnelle est relue avant intégration selon les parcours ci-dessous.
Elle réutilise GPUI 0.2.2, les sessions du coffre et les opérations du cœur client.

```text
┌ Kova · profil · instance ─ Actualiser ─ Paramètres ┐
│ Communautés │ Salons / Messages privés │ Fil      │
│ par nom     │ sélection par nom         │ auteur   │
│             │                          │ date     │
│             │ Membres par nom          │ état     │
│             │                          │ texte    │
│             │                          │ Plus tôt │
│             │                          │ brouillon│
│             │                          │ Envoyer  │
└ état de connexion / erreur et action de reprise ──┘
```

- Première utilisation : Paramètres → créer ou restaurer une identité →
  Connexion avec invitation → Actualiser → choisir communauté et salon.
- Discussion : choisir un salon ou un MP ; rédiger ; changer de fil ; revenir
  au brouillon. Après échec, conserver le texte et proposer Réessayer.
- Administration : choisir communauté et membre par leur nom ; ouvrir les
  paramètres pour inviter, créer un salon ou modérer sans recopier d'UUID.
- MP : sélectionner une conversation de la liste globale, puis ouvrir ses
  paramètres. La création/acceptation exige toujours une identité et une
  empreinte vérifiées hors bande. Consulter les réplicas et approuver sont
  deux actions distinctes ; une sélection ne donne aucun consentement.
- Clavier : Tab/Maj+Tab parcourent les contrôles, Entrée active les boutons ;
  le focus est visible. Les colonnes défilent indépendamment et le fil charge
  progressivement l'historique local. Les brouillons restent en mémoire dans
  la fenêtre ; leur conservation après fermeture n'est pas annoncée.

La recette exécutée et ses limites sont consignées dans `migration-plan.md`.

## Utilisation

Lancer `cargo run -p kova-desktop --locked`. Le champ de profil sélectionne
le coffre local ; `default` conserve le profil habituel. Saisir l'origine de
l'instance et utiliser **Actualiser** pour appliquer un changement de profil
ou d'instance. Pour une première utilisation, ouvrir **Paramètres / Bienvenue** :
créer une identité ou utiliser **Sauvegarde et restauration**, puis **Instance**
pour se connecter avec l'invitation et le nom local choisis. Une connexion
réussie renseigne l'instance du bandeau au retour dans les discussions.

Choisir une communauté affiche ses salons et ses membres. Cliquer un membre,
puis ouvrir **Membres et rôles** dans les paramètres pour agir sur cette
sélection ; son nom et la communauté restent affichés au-dessus du formulaire.
**Communautés et salons** crée une communauté, un salon ou une invitation.
Les droits restent contrôlés par le serveur. Une exclusion reste définitive.

La liste des MP regroupe les conversations du stockage chiffré de cet appareil
et les propositions reçues sur les instances connues ayant une session.
L'empreinte courte sert au repérage en l'absence de carnet de contacts ; elle
ne remplace jamais la comparaison de l'empreinte complète dans les paramètres.
Une proposition distante ouvre son instance d'origine pour la vérification.
La création et l'acceptation demandent l'identité et l'appareil vérifiés hors
bande. Proposer, consulter et approuver les réplicas restent des opérations
distinctes, avec les deux consentements requis.

**Afficher plus tôt** élargit par 50 messages la portion affichée du cache
local. Les dates du chat public viennent du serveur ; les MP v1 n'attestent
aucune date de création, ce que le fil indique. **Réessayer l'envoi en attente**
réutilise l'envoi déjà préparé ; après un échec, éviter d'en créer un second.
Les brouillons ne sont effacés après succès que s'ils correspondent toujours
au texte envoyé.

Le salon sélectionné conserve son flux WebSocket et sa reprise réseau.
Le MP sélectionné reprend sa synchronisation et son outbox toutes les 30 secondes.
La navigation ou l'ouverture des paramètres arrête le suivi du fil quitté.
La réception globale, les non-lus et les notifications relèvent du jalon 12.
Les brouillons ne sont pas sauvegardés après fermeture de la fenêtre.

`--diagnostics` ouvre les anciens formulaires avec identifiants techniques ;
`--spike` conserve le prototype de liste. Les variables `KOVA_UI_PROFILE` et
`KOVA_UI_ORIGIN` préremplissent le bandeau pour une recette isolée, sans nouvelle
connexion ni approbation automatique.
