# Chat public Windows

Connectez le profil à l’instance depuis **Connexion**. Dans **Communautés**, consultez les salons et copiez l’identifiant du salon voulu. Dans **Chat**, renseignez l’origine exacte et cet identifiant.

- **Recevoir en direct** ouvre le flux et rattrape l’historique depuis le cache. Une coupure provoque une reprise automatique avec délai croissant jusqu’à huit secondes. Une session expirée ou un accès retiré exige une intervention ; reconnectez-vous si nécessaire.
- **Arrêter le direct** ferme la réception. Changer de panneau l’arrête également.
- **Synchroniser** rattrape les messages par HTTP. Les 100 derniers messages conservés sont affichés, avec numéro, auteur local et date.
- **Envoyer** publie le texte dans le salon. Il doit contenir de 1 à 4000 caractères, ne pas être uniquement blanc et ne pas contenir NUL. Il est visible par les membres et l’exploitant de l’instance.
- **Réessayer l’envoi** réutilise l’identifiant du message en attente : même si le serveur avait reçu le premier essai, il ne crée pas de doublon. Tant qu’un envoi reste en attente, terminez ce réessai avant de rédiger un autre message. Le texte en attente survit au redémarrage ; le champ de saisie non envoyé n’est pas un brouillon sauvegardé.
- **Hors ligne** affiche la copie locale, sans vérifier les droits actuels auprès du serveur. Cette copie peut rester disponible après exclusion.

Le cache est dans `%LOCALAPPDATA%\Kova\profiles\<profil>\cache.sqlite`, séparé par origine, identité et salon. Il contient du texte public en clair, sans jeton de session. Une exclusion n’efface pas ce qui a déjà été reçu. La réception et le réseau s’exécutent hors du thread graphique.

Ce panneau reste une interface de prototype : identifiants copiés manuellement, un salon suivi à la fois, affichage des 100 derniers messages. Pas de pièces jointes, édition, suppression, MP ou chiffrement de bout en bout. La synchronisation conserve l’historique reçu sans politique automatique de rétention ; surveillez l’espace disque.
