ALTER TABLE users ADD COLUMN is_system INTEGER NOT NULL DEFAULT 0 CHECK(is_system IN(0,1) AND (is_system=0 OR active=0));
CREATE UNIQUE INDEX one_system_identity ON users(organisation_id) WHERE is_system=1;
CREATE TRIGGER system_identity_immutable BEFORE UPDATE ON users
WHEN OLD.is_system=1 AND (NEW.is_system<>1 OR NEW.active<>0 OR NEW.login<>OLD.login OR NEW.display_name<>OLD.display_name)
BEGIN SELECT RAISE(ABORT,'System identity is immutable and disabled'); END;
INSERT INTO permissions VALUES('work.validate'),('work.budget.read');
INSERT INTO role_permissions SELECT code,'work.validate' FROM roles WHERE code IN('exploitant','securite','technique');
INSERT INTO role_permissions SELECT code,'work.budget.read' FROM roles WHERE code IN('exploitant','securite','technique');
INSERT INTO role_permissions VALUES('commission','work.read');
ALTER TABLE works ADD COLUMN revision INTEGER NOT NULL DEFAULT 0 CHECK(revision>=0);
ALTER TABLE works ADD COLUMN target_due_on TEXT;
UPDATE works SET revision=coalesce((SELECT max(version) FROM entity_versions v WHERE v.entity_id=works.id),0);
ALTER TABLE work_costs ADD COLUMN receipt_file_id TEXT REFERENCES files(id);
CREATE TABLE work_cost_reversals(cost_id TEXT PRIMARY KEY REFERENCES work_costs(id),counter_id TEXT NOT NULL UNIQUE REFERENCES work_costs(id)) STRICT;
CREATE TRIGGER costs_append_only_update BEFORE UPDATE ON work_costs BEGIN SELECT RAISE(ABORT,'Cost ledger is append-only'); END;
CREATE TRIGGER costs_append_only_delete BEFORE DELETE ON work_costs BEGIN SELECT RAISE(ABORT,'Cost ledger is append-only'); END;
CREATE TABLE work_contacts(id TEXT PRIMARY KEY,work_id TEXT NOT NULL REFERENCES works(id),name TEXT NOT NULL,role TEXT NOT NULL,contact TEXT NOT NULL) STRICT;
CREATE TABLE work_documents(work_id TEXT NOT NULL REFERENCES works(id),file_id TEXT NOT NULL REFERENCES files(id),purpose TEXT NOT NULL CHECK(purpose IN('quote','invoice','report','photo','other')),active INTEGER NOT NULL DEFAULT 1 CHECK(active IN(0,1)),PRIMARY KEY(work_id,file_id)) STRICT;
CREATE TRIGGER work_document_identity BEFORE UPDATE OF work_id,file_id,purpose ON work_documents BEGIN SELECT RAISE(ABORT,'Document identity is immutable'); END;
CREATE TABLE work_observation_sync(observation_id TEXT PRIMARY KEY REFERENCES observations(id),work_id TEXT NOT NULL UNIQUE REFERENCES works(id)) STRICT;
CREATE INDEX work_planning ON works(building_id,status,planned_start,planned_end);
CREATE INDEX work_observation ON works(observation_id);
DROP TRIGGER work_status_transition;
CREATE TRIGGER work_status_transition BEFORE UPDATE OF status ON works
WHEN NEW.status<>OLD.status AND NOT (
 (OLD.status='identified' AND NEW.status='scheduled') OR
 (OLD.status='scheduled' AND NEW.status IN('identified','in_progress')) OR
 (OLD.status='in_progress' AND NEW.status='pending_validation') OR
 (OLD.status='pending_validation' AND NEW.status IN('in_progress','closed')) OR
 (OLD.status='closed' AND NEW.status IN('in_progress','archived')) OR
 (OLD.status='archived' AND NEW.status='closed'))
BEGIN SELECT RAISE(ABORT,'Invalid work status transition'); END;
ALTER TABLE works ADD COLUMN abandoned_reason TEXT;
ALTER TABLE works ADD COLUMN abandoned_from TEXT CHECK(abandoned_from IS NULL OR abandoned_from IN('identified','scheduled','in_progress','pending_validation'));
DROP TRIGGER work_status_transition;
CREATE TRIGGER work_status_transition BEFORE UPDATE OF status ON works
WHEN NEW.status<>OLD.status AND NOT (
 (OLD.status='identified' AND NEW.status='scheduled') OR
 (OLD.status='scheduled' AND NEW.status IN('identified','in_progress')) OR
 (OLD.status='in_progress' AND NEW.status='pending_validation') OR
 (OLD.status='pending_validation' AND NEW.status IN('in_progress','closed')) OR
 (OLD.status='closed' AND NEW.status IN('in_progress','archived')) OR
 (OLD.status='archived' AND OLD.abandoned_from IS NULL AND NEW.status='closed') OR
 (OLD.status IN('identified','scheduled','in_progress','pending_validation') AND NEW.status='archived' AND NEW.abandoned_reason IS NOT NULL AND NEW.abandoned_from=OLD.status) OR
 (OLD.status='archived' AND OLD.abandoned_from=NEW.status AND NEW.abandoned_reason IS NULL))
BEGIN SELECT RAISE(ABORT,'Invalid work status transition'); END;
CREATE TRIGGER work_auto_link_immutable BEFORE UPDATE OF observation_id,origin ON works
WHEN EXISTS(SELECT 1 FROM work_observation_sync s WHERE s.work_id=OLD.id) AND (NEW.observation_id IS NOT OLD.observation_id OR NEW.origin<>OLD.origin)
BEGIN SELECT RAISE(ABORT,'Automatic observation link is immutable'); END;
