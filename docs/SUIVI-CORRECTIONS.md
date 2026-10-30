# Corrections à reprendre après M8

Retours sur la version 0.10.0, enregistrés le 30 octobre 2026. Ces points sont constatés et restent ouverts ; les modules fonctionnels sont validés pour poursuivre la préparation de la publication.

## 1. Actions du tableau de bord
Les boutons Documents, Contrats et prestataires, Actualiser, Ajouter un bâtiment, Partager et Exports/import ne sont pas alignés. Regrouper les actions dans une barre cohérente, avec retour à la ligne propre aux petites largeurs. Supprimer les flottants indépendants responsables du décalage.

Critère : alignement stable aux largeurs 1024, 1280 et 1440 px, ordre clavier identique à l’ordre visuel, aucune action superposée ou isolée au-dessus des autres.

## 2. PDF récapitulatif des interventions
L’export actuel empile les champs en fiches longues. Préférer un tableau récapitulatif lisible : référence, bâtiment, intitulé, priorité, état, échéance, intervenant et, selon les droits, budget. Répéter les en-têtes, maîtriser les largeurs et les sauts de page. Réserver les longs commentaires à une annexe ou une fiche détaillée.

Critère : lecture rapide sur un exemple de plusieurs interventions, titres longs et budgets, avec contrôle visuel du PDF imprimé. Ne pas publier les exports réels d’une commune dans le dépôt.

## 3. Présentation du registre de sécurité
Revoir le PDF A à I pour évoquer un vrai registre : couverture rouge, identité de l’établissement et de la commune, sommaire, intercalaires, tableaux adaptés à chaque rubrique, pagination, espaces de signature lorsque pertinents et annexes identifiées.

Étape préalable : rechercher des modèles publics en ligne et citer les sources d’inspiration, sans reproduire de modèle commercial. Conserver tous les champs, mentions, horodatages et justificatifs existants. Faire valider une maquette avant de remplacer la présentation.

## 4. OpenStreetMap
Le fonctionnement reste défaillant chez l’utilisateur malgré les corrections précédentes. IGN constitue actuellement le recours fonctionnel. Reprendre le diagnostic des services, délais et messages d’erreur, avec un exemple public reproductible puis une recette sur le poste utilisateur. Conserver les plans déjà enregistrés et distinguer clairement la provenance OSM ou IGN.

Critère : géocodage et récupération du fond testés séparément ; échec explicite sans bloquer l’enregistrement de la fiche ; fonctionnement hors ligne d’un plan déjà conservé ; confirmation du résultat sur le poste concerné.
