# Communautés locales Windows

Ouvrir le desktop normalement et se connecter à l'instance dans **Connexion** avec le profil local voulu. Le premier profil de l'instance est initialisé par `bootstrap-admin`, comme décrit dans le README.

1. Dans **Communautés**, renseigner la même origine exacte, puis **Mes communautés**. **Créer la communauté** utilise le champ Nom et crée aussi le salon `général`.
2. Copier l'`id` de la communauté depuis le résultat dans le champ Identifiant. **Salons** et **Membres** affichent les ressources de cette communauté. Le champ Nom permet au propriétaire ou à un modérateur de **Créer le salon**.
3. **Inviter (24 h)** affiche un jeton à usage unique. Le copier et le transmettre uniquement au destinataire. Il reste en mémoire dans le résultat jusqu'à la prochaine action/navigation ; aucun jeton d'invitation n'est enregistré dans SQLite. Le presse-papiers n'est pas effacé automatiquement.
4. Le destinataire utilise **Connexion**, avec la même origine, le jeton, son identifiant local et son nom affiché. Il utilise son identité globale existante ; aucun mot de passe de compte n'est créé. Le parcours fonctionne aussi pour rejoindre une autre communauté d'une instance connue.
5. Dans **Modération**, saisir l'origine, l'identifiant de communauté et l'`id` du profil cible depuis **Membres**. Le propriétaire peut nommer un modérateur ou rétablir le rôle membre. **Exclure définitivement** retire l'accès et empêche cette identité de revenir avec une invitation. Un modérateur peut exclure un membre simple seulement. Vérifier la cible avant l'action.

Les erreurs de session, droits et accès sont affichées. Les opérations réseau/coffre s'exécutent en arrière-plan. La liste et les résultats sont relus depuis le serveur à chaque action : ce jalon ne propose pas encore de chat, cache communautaire hors ligne, synchronisation automatique ni sélecteur graphique de membres. Les identifiants se copient depuis les résultats du prototype.
