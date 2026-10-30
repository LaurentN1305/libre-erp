CREATE TABLE inventory_import_rows (
 organisation_id TEXT NOT NULL REFERENCES organisations(id),
 building_id TEXT NOT NULL REFERENCES buildings(id),
 fingerprint TEXT NOT NULL,
 equipment_id TEXT NOT NULL REFERENCES equipment(id),
 imported_at TEXT NOT NULL,
 imported_by TEXT NOT NULL REFERENCES users(id),
 PRIMARY KEY(organisation_id,building_id,fingerprint)
);
