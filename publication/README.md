# Clés publiques de signature

`cles-publiques.json` contient uniquement les deux clés publiques utilisées pour vérifier les versions Libre ERP : `tauri` pour la signature des paquets et `manifest` pour les manifestes Ed25519.

Les clés privées sont conservées chiffrées dans le coffre éditeur du compte Windows. Elles ne doivent jamais être copiées dans ce dossier ni dans le dépôt GitHub. Ne pas remplacer une clé publique sans préparer la migration des applications déjà installées.

La publication de ces clés ne signifie pas que les mises à jour sont déjà activées dans les anciennes versions installées. Une première version configurée devra être installée manuellement.
