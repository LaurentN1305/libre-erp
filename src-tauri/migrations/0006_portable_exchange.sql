ALTER TABLE users ADD COLUMN is_external INTEGER NOT NULL DEFAULT 0 CHECK(is_external IN(0,1) AND (is_external=0 OR active=0));
CREATE TRIGGER external_identity_locked BEFORE UPDATE ON users WHEN OLD.is_external=1 AND (NEW.active<>0 OR NEW.is_external<>1 OR NEW.login<>OLD.login) BEGIN SELECT RAISE(ABORT,'Imported attribution identity cannot authenticate'); END;
CREATE TABLE exchange_origins(lineage TEXT PRIMARY KEY,building_id TEXT NOT NULL UNIQUE REFERENCES buildings(id),last_received_hash TEXT,local_fingerprint TEXT,updated_at TEXT NOT NULL) STRICT;
CREATE TABLE exchange_receipts(id TEXT PRIMARY KEY,lineage TEXT NOT NULL,building_id TEXT NOT NULL REFERENCES buildings(id),previous_building_id TEXT REFERENCES buildings(id),received_hash TEXT NOT NULL,source_created_at TEXT NOT NULL,imported_at TEXT NOT NULL,imported_by TEXT NOT NULL REFERENCES users(id),summary_json TEXT NOT NULL CHECK(json_valid(summary_json))) STRICT;
CREATE TRIGGER exchange_receipts_immutable_update BEFORE UPDATE ON exchange_receipts BEGIN SELECT RAISE(ABORT,'Import history is append-only'); END;
CREATE TRIGGER exchange_receipts_immutable_delete BEFORE DELETE ON exchange_receipts BEGIN SELECT RAISE(ABORT,'Import history is append-only'); END;

DROP TRIGGER work_status_transition;
CREATE TRIGGER work_status_transition BEFORE UPDATE OF status ON works
WHEN NEW.status<>OLD.status AND NOT (
 (OLD.status='identified' AND NEW.status IN('scheduled','in_progress')) OR
 (OLD.status='scheduled' AND NEW.status IN('identified','in_progress')) OR
 (OLD.status='in_progress' AND NEW.status IN('pending_validation','closed')) OR
 (OLD.status='pending_validation' AND NEW.status IN('in_progress','closed')) OR
 (OLD.status='closed' AND NEW.status IN('in_progress','archived')) OR
 (OLD.status='archived' AND OLD.abandoned_from IS NULL AND NEW.status='closed') OR
 (OLD.status IN('identified','scheduled','in_progress','pending_validation') AND NEW.status='archived' AND NEW.abandoned_reason IS NOT NULL AND NEW.abandoned_from=OLD.status) OR
 (OLD.status='archived' AND OLD.abandoned_from=NEW.status AND NEW.abandoned_reason IS NULL))
BEGIN SELECT RAISE(ABORT,'Invalid work status transition'); END;
