ALTER TABLE documents ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;
ALTER TABLE documents ADD COLUMN notes TEXT NOT NULL DEFAULT '';
ALTER TABLE documents ADD COLUMN restricted INTEGER NOT NULL DEFAULT 0 CHECK(restricted IN(0,1));
UPDATE documents SET restricted=1 WHERE category_code IN('quote','invoice');
UPDATE documents SET revision=coalesce((SELECT max(version) FROM entity_versions WHERE entity_id=documents.id),0);
INSERT INTO permissions VALUES('documents.financial.read');
INSERT INTO role_permissions SELECT code,'documents.financial.read' FROM roles WHERE code IN('exploitant','securite','technique');
CREATE TRIGGER document_scope_locked BEFORE UPDATE OF building_id,organisation_id ON entities WHEN OLD.kind='document' AND (NEW.building_id IS NOT OLD.building_id OR NEW.organisation_id<>OLD.organisation_id) BEGIN SELECT RAISE(ABORT,'Document scope is immutable'); END;
CREATE TRIGGER document_versions_no_delete BEFORE DELETE ON document_versions BEGIN SELECT RAISE(ABORT,'Document versions are immutable'); END;
