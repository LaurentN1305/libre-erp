-- Remarque ou anomalie relevée sur un rapport pour une fiche liée (un équipement, le plus souvent).
-- Table séparée : les liens d'un document sont réécrits à chaque enregistrement, les remarques non.
CREATE TABLE document_link_notes (
 document_id TEXT NOT NULL REFERENCES documents(id), entity_id TEXT NOT NULL REFERENCES entities(id),
 note TEXT NOT NULL CHECK(length(note) BETWEEN 1 AND 4000),
 PRIMARY KEY(document_id,entity_id)
) STRICT;
