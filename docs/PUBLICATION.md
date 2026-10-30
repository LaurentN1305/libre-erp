# Publication GitHub et mises à jour

Dépôt : https://github.com/LaurentN1305/libre-erp — public, licence GPL-3.0-only.

## Première étape
Publication des sources de la version d’essai 0.10.0 et de la liste des corrections connues. La chaîne de contrôle GitHub Actions compile le frontend et exécute les tests Node et Rust sur Windows. Elle dispose uniquement du droit de lecture du dépôt et ne reçoit aucune clé privée.

Les dossiers de livraisons, archives, caches et outils locaux sont exclus. Les fichiers d’exemple des tests ne contiennent pas de données opérationnelles d’une commune. Aucun export PDF utilisateur n’est publié.

## Étapes avant la première mise à jour publique
1. Corriger le parcours de publication qui impose encore un certificat Authenticode, et adapter la récupération d’une ancienne version pour vérifier une preuve cryptographique sans certificat Windows.
2. Préparer une clé de signature Tauri et une clé Ed25519 du manifeste, conserver les clés privées hors du dépôt et prévoir une sauvegarde éditeur. N’afficher ni clés privées ni mots de passe dans un chat ou un journal.
3. Choisir et configurer le stockage des secrets de publication. Les clés publiques seront intégrées au logiciel ; les clés privées resteront réservées à la fabrication des versions.
4. Adapter le téléchargement aux redirections contrôlées des pièces jointes GitHub Releases. Le code actuel exige un même hôte et refuse les redirections : il ne suffit donc pas de remplacer l’URL du serveur.
5. Préparer deux canaux distincts stable et bêta, avec version, séquence, expiration et signatures vérifiées. Une empreinte SHA-256 non signée ne remplace pas une preuve d’authenticité.
6. Tester le cycle complet sur une installation de test : téléchargement, vérification, sauvegarde avant migration, installation et reprise avec sauvegarde compatible. Ne pas tester sur les données réelles de la commune.
7. Publier une version optimisée et les premiers manifestes signés après la recette.

Le dépôt public et les contrôles automatiques ne signifient pas que les mises à jour sont actives dans les versions installées. Les livraisons actuelles restent des essais non signés Authenticode ; aucune clé de production n’a été créée ou déposée pendant cette préparation.

## Références techniques
- [Updater Tauri 2](https://v2.tauri.app/plugin/updater/)
- [Signature Windows Tauri](https://v2.tauri.app/distribute/sign/windows/)
- [Permissions minimales des workflows GitHub](https://docs.github.com/en/actions/tutorials/authenticate-with-github_token)


## Préparation de la signature sans certificat Windows

La description About est harmonisée avec le README. Les outils suivants sont prêts :

- `Initialiser-Signatures.ps1` : crée les deux clés dans un nouveau coffre DPAPI du compte Windows, hors projet, sans remplacer un coffre existant. Les fichiers temporaires de génération sont retirés. Les clés publiques sont séparées.
- `Fabriquer-Version-Signee.ps1` : charge les clés uniquement pendant la fabrication, prépare la configuration publique Tauri et utilise le profil release. Aucun certificat Authenticode n’est imposé ; les signatures updater restent obligatoires.
- `Preparer-Version-Signee.ps1 -Canal stable -Sequence N -Sortie DOSSIER_NOUVEAU` : prépare les installeurs, signatures et preuves à partir de sources validées dans Git. Le dossier de sortie est nouveau ; aucune ancienne publication n’est remplacée.
- `Retour-Version.ps1 -Installateur CHEMIN -Preuve PREUVE_JSON` : fait contrôler l’empreinte signée Ed25519 par l’outil de récupération avant de proposer le retour manuel. Une preuve historique expirée reste utilisable pour un retour volontaire ; une recherche de mise à jour refuse un manifeste expiré.

Les manifestes sont distincts par canal et par format : `stable/windows-x86_64-nsis.json`, `stable/windows-x86_64-msi.json`, et les équivalents sous `beta`. Ils sont destinés aux Releases de canaux `stable` et `beta`. Les installeurs versionnés sont destinés à une Release `vVERSION`. Le client autorise les redirections HTTPS des pièces jointes GitHub vers ses hôtes de téléchargement connus, avec un nombre de sauts borné, puis vérifie les signatures et l’empreinte.

La préparation ne téléverse rien sur GitHub. Publier d’abord les paquets et leurs preuves dans la Release de version, puis les deux manifestes dans la Release du canal. Ne jamais publier `signatures.dpapi`, une clé privée, un mot de passe, ou un fichier temporaire du coffre. Chaque publication utilise une séquence strictement croissante pour son canal.

Le coffre DPAPI est lié au compte Windows : une simple copie vers une clé USB ne garantit pas la récupération sur un autre PC. Prévoir une sauvegarde éditeur transportable protégée avant d’activer la première mise à jour publique. Ne pas régénérer une clé déjà intégrée à une application installée.

État de cette préparation : validation sur des clés jetables (coffre, relecture et signature Tauri liée à une version), 34 tests Rust réussis et 11 tests Node réussis. Les deux tests réseau facultatifs restent ignorés. Les clés de production ont été créées après accord explicite de l’éditeur pour leur conservation sur son PC. Leur coffre chiffré a été relu et une signature Tauri de contrôle liée à une version a réussi. Les clés privées ne sont pas publiées. La sauvegarde transportable du coffre et la première publication signée restent à effectuer. L’ancienne version installée ne possède pas encore les clés publiques : une première installation manuelle de la version configurée sera nécessaire.


## Sauvegarder les clés avant la première publication

Sur le PC éditeur, double-cliquer sur `Sauvegarder-Cles-Signature.cmd`. Choisir un nouveau fichier `.erp-cles`, puis saisir deux fois une phrase d’au moins 12 caractères dans la console locale (saisie masquée). Ne jamais communiquer cette phrase dans un chat ou sur GitHub.

Le fichier obtenu est chiffré et relu immédiatement pour vérifier sa cohérence. Le copier sur un support séparé du PC et conserver sa phrase à part. Ce fichier ne doit pas être publié ; il est ignoré par Git.

Sur un autre PC ou compte Windows, `Restaurer-Cles-Signature.cmd` permet de choisir ce fichier et de saisir la phrase. La restauration recrée un coffre Windows à partir des mêmes clés ; elle refuse de remplacer un coffre existant. Les logiciels déjà distribués continuent ainsi de reconnaître les signatures.

Le test automatique du format de sauvegarde vérifie la relecture, le rejet d’une mauvaise phrase et le rejet d’une modification du contenu. Aucune sauvegarde des clés de production n’a été créée automatiquement : sa phrase doit être choisie et conservée par l’éditeur.
