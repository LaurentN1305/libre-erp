-- Autoriser les plans vectoriels OSM sans modifier les plans existants.
DROP TRIGGER building_map_source_insert;
DROP TRIGGER building_map_source_update;
CREATE TRIGGER building_map_source_insert BEFORE INSERT ON buildings
WHEN json_extract(NEW.location_json,'$.plan.source') IS NOT NULL AND json_extract(NEW.location_json,'$.plan.source') NOT IN('osm','ign','osm-vector')
BEGIN SELECT RAISE(ABORT,'Unknown map provenance'); END;
CREATE TRIGGER building_map_source_update BEFORE UPDATE OF location_json ON buildings
WHEN json_extract(NEW.location_json,'$.plan.source') IS NOT NULL AND json_extract(NEW.location_json,'$.plan.source') NOT IN('osm','ign','osm-vector')
BEGIN SELECT RAISE(ABORT,'Unknown map provenance'); END;
