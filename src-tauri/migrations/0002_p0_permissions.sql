INSERT INTO permissions(code) VALUES
 ('settings.read'),('users.manage'),('backup.manage'),('audit.read'),('updates.manage'),
 ('register.read'),('register.write'),('work.read'),('work.write'),('inventory.read'),('inventory.write'),
 ('contracts.read'),('contracts.write'),('documents.read'),('documents.write'),('alerts.read'),('exports.create');
INSERT INTO role_permissions SELECT 'exploitant',code FROM permissions;
INSERT INTO role_permissions SELECT 'securite',code FROM permissions WHERE code IN
 ('settings.read','audit.read','register.read','register.write','work.read','inventory.read','contracts.read','documents.read','documents.write','alerts.read','exports.create');
INSERT INTO role_permissions SELECT 'technique',code FROM permissions WHERE code IN
 ('settings.read','work.read','work.write','inventory.read','inventory.write','contracts.read','contracts.write','documents.read','documents.write','alerts.read','exports.create');
INSERT INTO role_permissions SELECT 'agent',code FROM permissions WHERE code IN ('settings.read','register.read','work.read','inventory.read','alerts.read');
INSERT INTO role_permissions SELECT 'prestataire',code FROM permissions WHERE code IN ('settings.read','work.read','contracts.read','documents.read');
INSERT INTO role_permissions SELECT 'commission',code FROM permissions WHERE code IN ('settings.read','register.read','documents.read');
INSERT INTO update_state(singleton,channel,installed_version,phase) VALUES(1,'stable','0.1.0','idle');
-- Garde-fous d'identité : la portée d'une identité ne change pas silencieusement.
CREATE TRIGGER entity_identity_immutable BEFORE UPDATE OF id,organisation_id,building_id,kind ON entities
WHEN NEW.id<>OLD.id OR NEW.organisation_id<>OLD.organisation_id OR NEW.building_id IS NOT OLD.building_id OR NEW.kind<>OLD.kind
BEGIN SELECT RAISE(ABORT,'Entity scope is immutable'); END;
CREATE TRIGGER users_org_immutable BEFORE UPDATE OF organisation_id ON users
WHEN NEW.organisation_id<>OLD.organisation_id BEGIN SELECT RAISE(ABORT,'User organisation is immutable'); END;
