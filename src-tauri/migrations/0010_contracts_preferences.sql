ALTER TABLE providers ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;
ALTER TABLE providers ADD COLUMN notes TEXT NOT NULL DEFAULT '';
ALTER TABLE contracts ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;
ALTER TABLE contracts ADD COLUMN intervention_interval_days INTEGER CHECK(intervention_interval_days IS NULL OR (intervention_interval_days>0 AND intervention_interval_months IS NULL));
ALTER TABLE provider_credentials ADD COLUMN file_id TEXT REFERENCES files(id);
ALTER TABLE provider_credentials ADD COLUMN active INTEGER NOT NULL DEFAULT 1 CHECK(active IN(0,1));
ALTER TABLE contract_visits ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;
CREATE TABLE contract_files(contract_id TEXT NOT NULL REFERENCES contracts(id),file_id TEXT NOT NULL REFERENCES files(id),visit_id TEXT REFERENCES contract_visits(id),title TEXT NOT NULL,created_at TEXT NOT NULL,PRIMARY KEY(contract_id,file_id)) STRICT;
INSERT INTO permissions VALUES('contracts.cost.read');
INSERT INTO role_permissions SELECT code,'contracts.cost.read' FROM roles WHERE code IN('exploitant','technique','securite');
CREATE TRIGGER provider_identity_locked BEFORE UPDATE OF id ON providers BEGIN SELECT RAISE(ABORT,'Identity is immutable'); END;
CREATE TRIGGER contract_identity_locked BEFORE UPDATE OF id ON contracts BEGIN SELECT RAISE(ABORT,'Identity is immutable'); END;

UPDATE providers SET revision=coalesce((SELECT max(version) FROM entity_versions WHERE entity_id=providers.id),0);
UPDATE contracts SET revision=coalesce((SELECT max(version) FROM entity_versions WHERE entity_id=contracts.id),0);
