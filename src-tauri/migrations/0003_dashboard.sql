INSERT INTO permissions(code) VALUES ('buildings.read'),('buildings.write');
INSERT INTO role_permissions SELECT code,'buildings.read' FROM roles WHERE code<>'prestataire';
INSERT INTO role_permissions VALUES ('exploitant','buildings.write'),('securite','buildings.write'),('technique','buildings.write');
CREATE INDEX dashboard_deadlines ON deadlines(building_id,due_on) WHERE completed_at IS NULL;
CREATE INDEX dashboard_compliance ON compliance_requirements(building_id,applicable,status);
