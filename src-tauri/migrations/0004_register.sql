CREATE INDEX register_navigation ON register_entries(building_id,block,effective_on);
CREATE TABLE register_observations (
 entry_id TEXT PRIMARY KEY REFERENCES register_entries(id),
 observation_id TEXT NOT NULL UNIQUE REFERENCES observations(id)
) STRICT;
