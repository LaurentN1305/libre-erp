# Partager Libre ERP par SFTP — version 0.12.0 de test

Le partage SFTP est proposé dans la version installée ; la version portable reste locale pour éviter de transporter des identifiants liés à un autre compte Windows. Le mode local reste le fonctionnement par défaut. Le partage est facultatif : un seul poste peut modifier les données à la fois. Les autres consultent la dernière version envoyée. Ce mode ne fusionne pas deux travaux simultanés.

## Préparer l’hébergement

Demandez à votre hébergeur un compte **SFTP** (et non FTP/FTPS), son serveur, le port (souvent 22), un dossier privé déjà créé, son chemin absolu et l’empreinte SSH SHA256 du serveur. Le compte doit pouvoir lire, écrire, créer des dossiers et renommer les fichiers sans écrasement. Utilisez un dossier dédié, de préférence hors du dossier public de votre site web. Cette première version utilise un mot de passe SFTP ; l’authentification par clé privée SFTP n’est pas encore proposée.

Ne communiquez pas les mots de passe dans une discussion ou une capture d’écran. La clé SSH de votre hébergement SFTP n’est pas la clé qui signe les mises à jour du logiciel.

## Premier poste : créer le partage

1. Installez Libre ERP 0.12.0 sur le premier PC et faites une sauvegarde manuelle de vos données.
2. Ouvrez **Paramètres → Partage SFTP** avec un compte exploitant.
3. Renseignez le serveur, le port, le compte, le dossier et une **phrase commune de partage d’au moins 12 caractères**. Conservez cette phrase séparément du serveur : elle est nécessaire pour ouvrir les archives partagées.
4. Cliquez sur **Lire l’empreinte SSH**. Comparez-la avec celle fournie par votre hébergeur, puis cochez sa confirmation. Si elle change par la suite, la connexion sera refusée.
5. Cliquez sur **Tester la connexion**.
6. Choisissez **Créer le partage avec les données de ce PC**, puis **Créer l’espace partagé**. Aucun espace déjà rempli ne sera remplacé.
7. Après le transfert, l’écran se recharge. Reconnectez-vous. Le logiciel tente de réserver les modifications pour votre compte ; le bandeau confirme le résultat.

La copie complète comprend les bâtiments, les interventions, les registres, les contrats, les documents, les photos et les **comptes Libre ERP**. Le compte et la phrase SFTP restent propres au poste et ne sont pas inclus dans ces archives.

## Autres postes : rejoindre

1. Installez la même version de Libre ERP. Au premier lancement, créez l’espace local et un compte exploitant temporaire pour accéder aux paramètres.
2. Ouvrez **Paramètres → Partage SFTP**. Renseignez le même dossier et la même phrase commune ; vérifiez l’empreinte et testez la connexion.
3. Choisissez **Rejoindre les données déjà partagées**, puis confirmez le remplacement de la copie active de ce PC.
4. Après le transfert, reconnectez-vous avec **un compte de l’espace partagé**, créé sur le premier poste. Le compte temporaire fait partie de l’ancienne copie locale conservée pour récupération.

Les données précédentes ne sont pas fusionnées : elles sont conservées dans un sous-dossier `recovery/before-restore-...` du stockage local. Ne supprimez pas ces copies pendant les essais.

## Travailler au quotidien

- À l’ouverture, Libre ERP recherche la dernière version partagée avant l’écran de connexion. Sans connexion, la dernière copie locale reste consultable.
- Après connexion, le logiciel tente de réserver les modifications. Un compte commission reste en consultation.
- Si quelqu’un travaille déjà, un message indique son nom et la date de réservation. Vous consultez la dernière version envoyée, pas ses modifications en cours.
- Après **Envoyer et libérer**, le poste repasse en consultation. Cliquez sur **Réserver les modifications** pour reprendre le travail.
- Pour récupérer un travail publié depuis votre ouverture, utilisez **Récupérer la dernière version et se reconnecter** dans Partage SFTP. Cette opération ferme la session.
- À la fermeture, choisissez **Envoyer et fermer**. Une copie chiffrée complète est transférée et vérifiée avant publication ; la réservation est libérée ensuite.
- **Fermer en conservant le travail et la réservation** conserve tout sur ce PC. Les autres ne pourront pas modifier tant que ce travail n’aura pas été envoyé ou la réservation récupérée par l’administrateur.

Il s’agit de versions de données, distinctes des mises à jour de l’application. Cette première version transfère une archive complète à chaque envoi, avec une limite de **2 Gio par version**. Prévoyez l’espace et le temps de transfert nécessaires. Les versions précédentes restent sur l’hébergement ; aucun effacement automatique n’est effectué. La limite de sécurité est de 10 000 entrées dans le dossier des versions.

## Si la connexion est coupée

Un poste qui a déjà réservé peut continuer son travail localement pendant la coupure. L’envoi échoue sans remplacer la version partagée et la fenêtre reste ouverte. Réessayez après rétablissement de la connexion ; le transfert complet est relancé.

Après un arrêt brutal, le travail et le jeton de réservation sont conservés. À la réouverture, la copie locale en attente n’est jamais remplacée. Reconnectez-vous, puis utilisez **Réserver les modifications** pour vérifier et reprendre votre réservation. Une connexion est nécessaire à cette vérification après redémarrage.

Si la réservation a été reprise par un autre poste ou si le serveur présente une version différente, l’envoi est bloqué. Exportez d’abord une sauvegarde manuelle de la copie locale avant toute récupération. Le logiciel ne fusionne pas automatiquement ces données.

## Changement du mot de passe SFTP

Dans Partage SFTP, renseignez le nouveau mot de passe, confirmez l’empreinte puis utilisez **Enregistrer les identifiants de connexion**. Laissez la phrase commune vide pour conserver celle déjà enregistrée. Le logiciel vérifie la connexion et l’historique distant avant d’enregistrer : le travail en attente et la réservation restent sur le PC. Si vous changez de compte ou de serveur, renseignez son mot de passe ; l’ancien mot de passe ne sera pas transmis automatiquement à un autre compte ou serveur.

## Réservation abandonnée

Un exploitant peut ouvrir **Réservation abandonnée · administration**, lire la réservation, vérifier auprès de la personne que Libre ERP est fermé, puis saisir **LIBERER**. L’ancienne réservation est archivée sur le serveur. Il faut ensuite récupérer la dernière version avant de réserver à nouveau.

Ne faites pas cette opération pour forcer deux personnes à travailler simultanément. Le travail non envoyé de l’ancien poste reste sur ce poste et doit être récupéré séparément. Les numéros de version et les fichiers sans écrasement empêchent sa publication sur une version plus récente.

## Sauvegardes, sécurité et retour au local

- Les sauvegardes manuelles restent disponibles. La conservation des versions SFTP ne remplace pas une sauvegarde indépendante de l’hébergement.
- Les sauvegardes planifiées Windows sont suspendues dans ce premier mode partagé. Leur dossier et leur programmation doivent être reconfigurés après un retour au local ; ils ne sont pas repris depuis un autre poste.
- Envoyez et libérez le travail avant une mise à jour de Libre ERP. La restauration d’une archive indépendante nécessite de revenir au mode local.
- **Revenir au mode local** est possible après envoi et libération. La copie du PC devient indépendante ; ses modifications ultérieures ne sont plus envoyées.
- Le mot de passe SFTP et la phrase commune sont chiffrés avec la protection Windows du compte local. Ils ne sont pas affichés lors d’une réouverture des paramètres, ni inclus dans les archives partagées.
- Avec la double authentification activée, utilisez le code temporaire de votre application d’authentification. Les codes de secours à usage unique ne sont pas acceptés en mode partagé pour éviter leur réutilisation après remplacement d’une copie locale.
- Les changements de comptes et de droits sont transmis lors de l’envoi ; les autres postes doivent récupérer la version récente. L’authentification et les journaux des consultations restent locaux. Ce mode n’est pas un serveur d’authentification central et ne révoque pas instantanément un poste déconnecté.

## Validation de cette livraison

Tests réalisés avec des données fictives : deux stockages locaux indépendants, serveur SFTP éphémère sur l’ordinateur, vérification de la clé SSH, compte partagé, SQLCipher, pièce jointe, exclusion d’un deuxième rédacteur, transmission d’une modification et restauration sur le deuxième poste. Les essais sur votre hébergement et sur deux PC physiques restent à réaliser avant un usage réel en équipe.
