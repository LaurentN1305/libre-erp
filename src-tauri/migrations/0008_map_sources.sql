-- Provenance du fond, également utilisée pour la compatibilité des échanges.
CREATE TRIGGER building_map_source_insert BEFORE INSERT ON buildings
WHEN json_extract(NEW.location_json,'$.plan.source') IS NOT NULL AND json_extract(NEW.location_json,'$.plan.source') NOT IN('osm','ign')
BEGIN SELECT RAISE(ABORT,'Unknown map provenance'); END;
CREATE TRIGGER building_map_source_update BEFORE UPDATE OF location_json ON buildings
WHEN json_extract(NEW.location_json,'$.plan.source') IS NOT NULL AND json_extract(NEW.location_json,'$.plan.source') NOT IN('osm','ign')
BEGIN SELECT RAISE(ABORT,'Unknown map provenance'); END;
