ALTER TABLE buildings ADD COLUMN location_json TEXT CHECK(location_json IS NULL OR json_valid(location_json));
CREATE TABLE map_geocache(organisation_id TEXT NOT NULL REFERENCES organisations(id),query TEXT NOT NULL,results_json TEXT NOT NULL CHECK(json_valid(results_json)),fetched_at TEXT NOT NULL,PRIMARY KEY(organisation_id,query)) STRICT;
