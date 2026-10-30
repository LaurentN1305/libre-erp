-- Proposition initiale. Identifiants UUID créés en Rust ; UTC RFC3339 ; montants en centimes.
-- Ouvrir avec SQLCipher et appliquer la clé AVANT toute lecture ou migration.


CREATE TABLE organisations (
 id TEXT PRIMARY KEY, name TEXT NOT NULL, address TEXT, timezone TEXT NOT NULL DEFAULT 'Europe/Paris',
 created_at TEXT NOT NULL, archived_at TEXT
) STRICT;
CREATE TABLE users (
 id TEXT PRIMARY KEY, organisation_id TEXT NOT NULL REFERENCES organisations(id), login TEXT NOT NULL COLLATE NOCASE,
 display_name TEXT NOT NULL, password_hash TEXT NOT NULL, totp_secret_encrypted BLOB,
 totp_last_step INTEGER, failed_attempts INTEGER NOT NULL DEFAULT 0 CHECK(failed_attempts>=0), locked_until TEXT,
 active INTEGER NOT NULL DEFAULT 1 CHECK(active IN(0,1)), created_at TEXT NOT NULL, anonymised_at TEXT,
 UNIQUE(organisation_id,login), UNIQUE(id,organisation_id)
) STRICT;
CREATE TABLE roles (code TEXT PRIMARY KEY, label TEXT NOT NULL) STRICT;
INSERT INTO roles VALUES ('exploitant','Exploitant'),('securite','Responsable sécurité'),
 ('technique','Responsable technique'),('agent','Agent'),('prestataire','Prestataire'),('commission','Commission lecture seule');
CREATE TABLE permissions (code TEXT PRIMARY KEY) STRICT;
CREATE TABLE role_permissions (
 role_code TEXT NOT NULL REFERENCES roles(code), permission_code TEXT NOT NULL REFERENCES permissions(code),
 PRIMARY KEY(role_code,permission_code)
) STRICT;
CREATE TABLE sessions (
 id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), token_hash BLOB NOT NULL UNIQUE,
 created_at TEXT NOT NULL, expires_at TEXT NOT NULL, last_activity_at TEXT NOT NULL,
 mfa_verified INTEGER NOT NULL CHECK(mfa_verified IN(0,1)), revoked_at TEXT
) STRICT;
CREATE TABLE recovery_codes (
 user_id TEXT NOT NULL REFERENCES users(id), code_hash TEXT NOT NULL, used_at TEXT,
 PRIMARY KEY(user_id,code_hash)
) STRICT;
-- Une entité générique typée donne de vraies FK aux liens, versions et échéances.
CREATE TABLE entities (
 id TEXT PRIMARY KEY, organisation_id TEXT NOT NULL REFERENCES organisations(id), building_id TEXT,
 kind TEXT NOT NULL CHECK(kind IN('building','register_entry','equipment','work','provider','contract','document','observation','inspection','exercise','see','staff','stock')),
 created_at TEXT NOT NULL, created_by TEXT REFERENCES users(id), archived_at TEXT,
 UNIQUE(id,organisation_id), UNIQUE(id,building_id),
 FOREIGN KEY(building_id,organisation_id) REFERENCES buildings(id,organisation_id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE buildings (
 id TEXT PRIMARY KEY REFERENCES entities(id), organisation_id TEXT NOT NULL REFERENCES organisations(id),
 name TEXT NOT NULL, address TEXT NOT NULL, erp_category INTEGER NOT NULL CHECK(erp_category BETWEEN 1 AND 5),
 capacity INTEGER NOT NULL CHECK(capacity>=0), safety_manager_id TEXT, photo_file_id TEXT REFERENCES files(id),
 opened_on TEXT, closed_on TEXT, UNIQUE(id,organisation_id),
 FOREIGN KEY(safety_manager_id,organisation_id) REFERENCES users(id,organisation_id)
) STRICT;
CREATE TABLE erp_types (code TEXT PRIMARY KEY, label TEXT NOT NULL) STRICT;
INSERT INTO erp_types VALUES ('J','Accueil personnes âgées ou handicapées'),('L','Salles de réunion et spectacles'),
 ('M','Magasins'),('N','Restaurants'),('O','Hôtels'),('P','Danse et jeux'),('R','Enseignement'),
 ('S','Bibliothèques'),('T','Expositions'),('U','Soins'),('V','Culte'),('W','Administrations'),
 ('X','Sport couvert'),('Y','Musées'),('PA','Plein air'),('CTS','Chapiteaux'),('SG','Structures gonflables'),
 ('PS','Stationnement'),('GA','Gares'),('OA','Altitude'),('EF','Établissements flottants'),('REF','Refuges');
CREATE TABLE building_erp_types (
 building_id TEXT NOT NULL REFERENCES buildings(id), type_code TEXT NOT NULL REFERENCES erp_types(code),
 is_primary INTEGER NOT NULL CHECK(is_primary IN(0,1)), PRIMARY KEY(building_id,type_code)
) STRICT;
CREATE UNIQUE INDEX one_primary_erp_type ON building_erp_types(building_id) WHERE is_primary=1;
CREATE TABLE role_grants (
 id TEXT PRIMARY KEY, user_id TEXT NOT NULL, organisation_id TEXT NOT NULL, building_id TEXT,
 role_code TEXT NOT NULL REFERENCES roles(code), expires_at TEXT,
 FOREIGN KEY(user_id,organisation_id) REFERENCES users(id,organisation_id),
 FOREIGN KEY(building_id,organisation_id) REFERENCES buildings(id,organisation_id)
) STRICT;
CREATE UNIQUE INDEX unique_org_grant ON role_grants(user_id,organisation_id,role_code) WHERE building_id IS NULL;
CREATE UNIQUE INDEX unique_building_grant ON role_grants(user_id,building_id,role_code) WHERE building_id IS NOT NULL;
CREATE TABLE files (
 id TEXT PRIMARY KEY, organisation_id TEXT NOT NULL REFERENCES organisations(id), storage_key TEXT NOT NULL UNIQUE,
 original_name TEXT NOT NULL, mime_type TEXT NOT NULL, plaintext_sha256 TEXT NOT NULL CHECK(length(plaintext_sha256)=64),
 byte_size INTEGER NOT NULL CHECK(byte_size>=0), encryption_version INTEGER NOT NULL, nonce BLOB NOT NULL,
 key_envelope BLOB NOT NULL, created_at TEXT NOT NULL, created_by TEXT REFERENCES users(id)
) STRICT;
CREATE TABLE entity_files (
 entity_id TEXT NOT NULL, file_id TEXT NOT NULL, organisation_id TEXT NOT NULL REFERENCES organisations(id),
 purpose TEXT NOT NULL, position INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(entity_id,file_id,purpose),
 FOREIGN KEY(entity_id,organisation_id) REFERENCES entities(id,organisation_id),
 FOREIGN KEY(file_id) REFERENCES files(id)
) STRICT;
CREATE TABLE entity_versions (
 entity_id TEXT NOT NULL REFERENCES entities(id), version INTEGER NOT NULL CHECK(version>0),
 payload_json TEXT NOT NULL CHECK(json_valid(payload_json)), recorded_at TEXT NOT NULL,
 recorded_by TEXT NOT NULL REFERENCES users(id), reason TEXT NOT NULL,
 PRIMARY KEY(entity_id,version)
) STRICT;
CREATE TABLE locations (
 id TEXT PRIMARY KEY, building_id TEXT NOT NULL REFERENCES buildings(id), parent_id TEXT,
 kind TEXT NOT NULL CHECK(kind IN('level','zone','room')), name TEXT NOT NULL, sort_order INTEGER NOT NULL DEFAULT 0,
 UNIQUE(id,building_id), FOREIGN KEY(parent_id,building_id) REFERENCES locations(id,building_id), CHECK(parent_id IS NULL OR parent_id<>id)
) STRICT;
CREATE TABLE reference_counters (
 organisation_id TEXT NOT NULL REFERENCES organisations(id), prefix TEXT NOT NULL, year INTEGER NOT NULL,
 last_value INTEGER NOT NULL CHECK(last_value>=0), PRIMARY KEY(organisation_id,prefix,year)
) STRICT;
CREATE TABLE registers (
 building_id TEXT PRIMARY KEY REFERENCES buildings(id), opened_on TEXT NOT NULL, title TEXT NOT NULL, reference TEXT NOT NULL
) STRICT;
CREATE TABLE register_entries (
 id TEXT PRIMARY KEY REFERENCES entities(id), building_id TEXT NOT NULL REFERENCES registers(building_id),
 block TEXT NOT NULL CHECK(block IN('A','B','C','D','E','F','G','H','I')), title TEXT NOT NULL,
 current_version INTEGER NOT NULL CHECK(current_version>0), effective_on TEXT NOT NULL,
 applicability TEXT NOT NULL DEFAULT 'applicable' CHECK(applicability IN('applicable','not_applicable','to_confirm')),
 justification TEXT, UNIQUE(id,building_id),
 FOREIGN KEY(id,current_version) REFERENCES entity_versions(entity_id,version) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE safety_staff (
 id TEXT PRIMARY KEY REFERENCES entities(id), building_id TEXT NOT NULL REFERENCES buildings(id), user_id TEXT REFERENCES users(id),
 first_name TEXT NOT NULL, last_name TEXT NOT NULL, job_title TEXT NOT NULL, hierarchy_rank INTEGER NOT NULL CHECK(hierarchy_rank>=0),
 supervisor_id TEXT, qualification_level TEXT, authorised_on TEXT, valid_until TEXT,
 status TEXT NOT NULL CHECK(status IN('active','suspended','left')), UNIQUE(id,building_id),
 FOREIGN KEY(supervisor_id,building_id) REFERENCES safety_staff(id,building_id), CHECK(supervisor_id IS NULL OR supervisor_id<>id)
) STRICT;
CREATE TABLE staff_training (
 id TEXT PRIMARY KEY, staff_id TEXT NOT NULL REFERENCES safety_staff(id), name TEXT NOT NULL,
 training_on TEXT NOT NULL, valid_until TEXT, certificate_document_id TEXT REFERENCES documents(id)
) STRICT;
CREATE TABLE instructions (
 entry_id TEXT PRIMARY KEY REFERENCES register_entries(id), scope TEXT NOT NULL,
 fire_instructions TEXT NOT NULL, disability_evacuation TEXT NOT NULL, assembly_point TEXT
) STRICT;
CREATE TABLE commission_visits (
 id TEXT PRIMARY KEY, building_id TEXT NOT NULL REFERENCES buildings(id), visited_on TEXT NOT NULL,
 commission_name TEXT NOT NULL, opinion TEXT NOT NULL, next_visit_on TEXT, report_document_id TEXT REFERENCES documents(id), UNIQUE(id,building_id)
) STRICT;
CREATE TABLE inspections (
 id TEXT PRIMARY KEY REFERENCES entities(id), building_id TEXT NOT NULL REFERENCES buildings(id),
 entry_id TEXT REFERENCES register_entries(id), inspection_type TEXT NOT NULL, inspected_on TEXT NOT NULL,
 inspector TEXT NOT NULL, result TEXT NOT NULL, remarks TEXT, next_due_on TEXT,
 report_document_id TEXT REFERENCES documents(id), UNIQUE(id,building_id)
) STRICT;
CREATE TABLE observations (
 id TEXT PRIMARY KEY REFERENCES entities(id), building_id TEXT NOT NULL REFERENCES buildings(id), visit_id TEXT, inspection_id TEXT,
 reference TEXT NOT NULL, description TEXT NOT NULL, severity INTEGER NOT NULL CHECK(severity BETWEEN 1 AND 4),
 issued_on TEXT NOT NULL, due_on TEXT, status TEXT NOT NULL CHECK(status IN('open','in_progress','resolved','accepted')),
 resolved_at TEXT, validated_by TEXT REFERENCES users(id), evidence_document_id TEXT REFERENCES documents(id),
 UNIQUE(id,building_id), UNIQUE(building_id,reference),
 FOREIGN KEY(visit_id,building_id) REFERENCES commission_visits(id,building_id),
 FOREIGN KEY(inspection_id,building_id) REFERENCES inspections(id,building_id)
) STRICT;
CREATE TABLE fire_exercises (
 id TEXT PRIMARY KEY REFERENCES entities(id), building_id TEXT NOT NULL REFERENCES buildings(id), entry_id TEXT REFERENCES register_entries(id),
 started_at TEXT NOT NULL, duration_seconds INTEGER CHECK(duration_seconds>=0), participant_count INTEGER CHECK(participant_count>=0),
 scenario TEXT NOT NULL, disability_measures TEXT NOT NULL, results TEXT NOT NULL, improvements TEXT, next_due_on TEXT
) STRICT;
CREATE TABLE see_solutions (
 id TEXT PRIMARY KEY REFERENCES entities(id), building_id TEXT NOT NULL REFERENCES buildings(id), entry_id TEXT REFERENCES register_entries(id),
 title TEXT NOT NULL, design TEXT NOT NULL, implementation TEXT NOT NULL, operating_conditions TEXT NOT NULL,
 maintenance_conditions TEXT NOT NULL, objectives_attestation_id TEXT REFERENCES documents(id),
 implementation_attestation_id TEXT REFERENCES documents(id), modifications TEXT NOT NULL DEFAULT '', approved_on TEXT
) STRICT;
CREATE TABLE equipment_categories (
 id TEXT PRIMARY KEY, parent_id TEXT REFERENCES equipment_categories(id), code TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
 CHECK(parent_id IS NULL OR parent_id<>id)
) STRICT;
CREATE TABLE equipment (
 id TEXT PRIMARY KEY REFERENCES entities(id), building_id TEXT NOT NULL REFERENCES buildings(id), reference TEXT NOT NULL,
 designation TEXT NOT NULL, category_id TEXT NOT NULL REFERENCES equipment_categories(id), location_id TEXT,
 manufacturer TEXT, model TEXT, serial_number TEXT, installed_on TEXT, commissioned_on TEXT,
 lifespan_months INTEGER CHECK(lifespan_months>0), end_of_life_on TEXT, purchase_cost_cents INTEGER CHECK(purchase_cost_cents>=0),
 rated_power_w INTEGER CHECK(rated_power_w>=0), condition TEXT NOT NULL CHECK(condition IN('new','good','worn','faulty','out_of_service')),
 maintenance_interval_months INTEGER CHECK(maintenance_interval_months>0), last_maintenance_on TEXT, next_maintenance_on TEXT,
 supplier_id TEXT REFERENCES providers(id), notes TEXT, qr_token TEXT NOT NULL UNIQUE, UNIQUE(id,building_id), UNIQUE(building_id,reference),
 FOREIGN KEY(location_id,building_id) REFERENCES locations(id,building_id)
) STRICT;
CREATE TABLE equipment_events (
 id TEXT PRIMARY KEY, equipment_id TEXT NOT NULL REFERENCES equipment(id), occurred_at TEXT NOT NULL,
 recorded_at TEXT NOT NULL, recorded_by TEXT NOT NULL REFERENCES users(id), event_type TEXT NOT NULL,
 description TEXT NOT NULL, work_id TEXT REFERENCES works(id)
) STRICT;
CREATE TABLE consumption_readings (
 id TEXT PRIMARY KEY, equipment_id TEXT NOT NULL REFERENCES equipment(id), measured_at TEXT NOT NULL,
 quantity_milliunits INTEGER NOT NULL CHECK(quantity_milliunits>=0), unit TEXT NOT NULL,
 mode TEXT NOT NULL CHECK(mode IN('cumulative','interval')), period_start TEXT, recorded_by TEXT NOT NULL REFERENCES users(id),
 UNIQUE(equipment_id,measured_at,unit,mode)
) STRICT;
CREATE TABLE works (
 id TEXT PRIMARY KEY REFERENCES entities(id), building_id TEXT NOT NULL REFERENCES buildings(id), reference TEXT NOT NULL, title TEXT NOT NULL,
 description TEXT NOT NULL, type TEXT NOT NULL CHECK(type IN('preventive','corrective','compliance')),
 origin TEXT NOT NULL CHECK(origin IN('commission_observation','failure','regulation','maintenance','other')),
 observation_id TEXT, priority INTEGER NOT NULL CHECK(priority BETWEEN 1 AND 4),
 status TEXT NOT NULL DEFAULT 'identified' CHECK(status IN('identified','scheduled','in_progress','pending_validation','closed','archived')),
 planned_start TEXT, planned_end TEXT, actual_start TEXT, actual_end TEXT,
 estimated_budget_cents INTEGER NOT NULL DEFAULT 0 CHECK(estimated_budget_cents>=0),
 progress_percent INTEGER NOT NULL DEFAULT 0 CHECK(progress_percent BETWEEN 0 AND 100),
 project_manager TEXT, architect TEXT, commission_authorisation TEXT, safety_impact TEXT, compensatory_measures TEXT,
 validated_by TEXT REFERENCES users(id), validated_at TEXT, UNIQUE(id,building_id), UNIQUE(building_id,reference),
 FOREIGN KEY(observation_id,building_id) REFERENCES observations(id,building_id),
 CHECK(origin<>'commission_observation' OR observation_id IS NOT NULL),
 CHECK(planned_start IS NULL OR planned_end IS NULL OR planned_end>=planned_start),
 CHECK(actual_start IS NULL OR actual_end IS NULL OR actual_end>=actual_start),
 CHECK(status NOT IN('closed','archived') OR (validated_by IS NOT NULL AND validated_at IS NOT NULL))
) STRICT;
CREATE TABLE work_equipment (
 work_id TEXT NOT NULL, equipment_id TEXT NOT NULL, building_id TEXT NOT NULL,
 PRIMARY KEY(work_id,equipment_id), FOREIGN KEY(work_id,building_id) REFERENCES works(id,building_id),
 FOREIGN KEY(equipment_id,building_id) REFERENCES equipment(id,building_id)
) STRICT;
CREATE TABLE work_dependencies (
 work_id TEXT NOT NULL REFERENCES works(id), predecessor_id TEXT NOT NULL REFERENCES works(id),
 lag_days INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(work_id,predecessor_id), CHECK(work_id<>predecessor_id)
) STRICT;
CREATE TABLE work_status_events (
 id TEXT PRIMARY KEY, work_id TEXT NOT NULL REFERENCES works(id), from_status TEXT, to_status TEXT NOT NULL,
 occurred_at TEXT NOT NULL, actor_id TEXT NOT NULL REFERENCES users(id), reason TEXT NOT NULL
) STRICT;
CREATE TABLE work_costs (
 id TEXT PRIMARY KEY, work_id TEXT NOT NULL REFERENCES works(id), description TEXT NOT NULL,
 amount_cents INTEGER NOT NULL, incurred_on TEXT NOT NULL, invoice_document_id TEXT REFERENCES documents(id)
) STRICT;
CREATE VIEW work_budgets AS SELECT w.id, w.estimated_budget_cents,
 coalesce((SELECT sum(c.amount_cents) FROM work_costs c WHERE c.work_id=w.id),0) AS actual_budget_cents FROM works w;
CREATE TABLE providers (
 id TEXT PRIMARY KEY REFERENCES entities(id), name TEXT NOT NULL, registration_number TEXT, address TEXT, email TEXT, phone TEXT
) STRICT;
CREATE TABLE provider_credentials (
 id TEXT PRIMARY KEY, provider_id TEXT NOT NULL REFERENCES providers(id), kind TEXT NOT NULL CHECK(kind IN('insurance','qualification','authorisation')),
 title TEXT NOT NULL, issued_on TEXT, expires_on TEXT NOT NULL, document_id TEXT REFERENCES documents(id)
) STRICT;
CREATE TABLE provider_users (
 provider_id TEXT NOT NULL REFERENCES providers(id), user_id TEXT NOT NULL REFERENCES users(id), PRIMARY KEY(provider_id,user_id)
) STRICT;
CREATE TABLE work_participants (
 id TEXT PRIMARY KEY, work_id TEXT NOT NULL REFERENCES works(id), user_id TEXT REFERENCES users(id), provider_id TEXT REFERENCES providers(id),
 role TEXT NOT NULL, CHECK((user_id IS NOT NULL)+(provider_id IS NOT NULL)=1)
) STRICT;
CREATE TABLE contracts (
 id TEXT PRIMARY KEY REFERENCES entities(id), provider_id TEXT NOT NULL REFERENCES providers(id), reference TEXT NOT NULL,
 subject TEXT NOT NULL, starts_on TEXT NOT NULL, ends_on TEXT, notice_days INTEGER NOT NULL DEFAULT 0 CHECK(notice_days>=0),
 cost_cents INTEGER NOT NULL CHECK(cost_cents>=0), cost_period TEXT NOT NULL CHECK(cost_period IN('one_off','monthly','annual','per_visit')),
 intervention_interval_months INTEGER CHECK(intervention_interval_months>0), terms TEXT, CHECK(ends_on IS NULL OR ends_on>=starts_on)
) STRICT;
CREATE TABLE contract_buildings (
 contract_id TEXT NOT NULL REFERENCES contracts(id), building_id TEXT NOT NULL REFERENCES buildings(id), PRIMARY KEY(contract_id,building_id)
) STRICT;
CREATE TABLE contract_equipment (
 contract_id TEXT NOT NULL, equipment_id TEXT NOT NULL, building_id TEXT NOT NULL,
 PRIMARY KEY(contract_id,equipment_id), FOREIGN KEY(contract_id,building_id) REFERENCES contract_buildings(contract_id,building_id),
 FOREIGN KEY(equipment_id,building_id) REFERENCES equipment(id,building_id)
) STRICT;
CREATE TABLE contract_visits (
 id TEXT PRIMARY KEY, contract_id TEXT NOT NULL, building_id TEXT NOT NULL, planned_at TEXT NOT NULL,
 completed_at TEXT, status TEXT NOT NULL CHECK(status IN('planned','completed','cancelled','missed')),
 report_document_id TEXT REFERENCES documents(id), notes TEXT,
 FOREIGN KEY(contract_id,building_id) REFERENCES contract_buildings(contract_id,building_id)
) STRICT;
CREATE TABLE document_categories (code TEXT PRIMARY KEY, label TEXT NOT NULL) STRICT;
INSERT INTO document_categories VALUES ('doe','DOE'),('plan','Plan'),('material_certificate','PV matériaux'),('quote','Devis'),
 ('invoice','Facture'),('instruction','Consigne'),('photo','Photo'),('report','Rapport'),('authorisation','Autorisation'),
 ('notice','Notice'),('attestation','Attestation'),('other','Autre');
CREATE TABLE documents (
 id TEXT PRIMARY KEY REFERENCES entities(id), category_code TEXT NOT NULL REFERENCES document_categories(code),
 title TEXT NOT NULL, current_version INTEGER NOT NULL CHECK(current_version>0), expires_on TEXT, retention_until TEXT,
 FOREIGN KEY(id,current_version) REFERENCES document_versions(document_id,version) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE document_versions (
 document_id TEXT NOT NULL REFERENCES documents(id), version INTEGER NOT NULL CHECK(version>0), file_id TEXT NOT NULL REFERENCES files(id),
 created_at TEXT NOT NULL, created_by TEXT NOT NULL REFERENCES users(id), change_note TEXT NOT NULL,
 PRIMARY KEY(document_id,version)
) STRICT;
CREATE TABLE document_links (
 document_id TEXT NOT NULL REFERENCES documents(id), entity_id TEXT NOT NULL REFERENCES entities(id),
 purpose TEXT NOT NULL, PRIMARY KEY(document_id,entity_id,purpose)
) STRICT;
CREATE TABLE inspection_equipment (
 inspection_id TEXT NOT NULL, equipment_id TEXT NOT NULL, building_id TEXT NOT NULL,
 PRIMARY KEY(inspection_id,equipment_id), FOREIGN KEY(inspection_id,building_id) REFERENCES inspections(id,building_id),
 FOREIGN KEY(equipment_id,building_id) REFERENCES equipment(id,building_id)
) STRICT;
CREATE TABLE stock_items (
 id TEXT PRIMARY KEY REFERENCES entities(id), building_id TEXT NOT NULL REFERENCES buildings(id), location_id TEXT,
 designation TEXT NOT NULL, unit TEXT NOT NULL, minimum_milliunits INTEGER NOT NULL CHECK(minimum_milliunits>=0),
 FOREIGN KEY(location_id,building_id) REFERENCES locations(id,building_id)
) STRICT;
CREATE TABLE stock_movements (
 id TEXT PRIMARY KEY, stock_item_id TEXT NOT NULL REFERENCES stock_items(id), delta_milliunits INTEGER NOT NULL CHECK(delta_milliunits<>0),
 recorded_at TEXT NOT NULL, recorded_by TEXT NOT NULL REFERENCES users(id), reason TEXT NOT NULL
) STRICT;
CREATE TABLE regulatory_references (
 id TEXT PRIMARY KEY, code TEXT NOT NULL, article TEXT NOT NULL, source_url TEXT NOT NULL,
 effective_from TEXT NOT NULL, effective_until TEXT, verified_on TEXT, text_snapshot TEXT NOT NULL,
 UNIQUE(code,article,effective_from)
) STRICT;
CREATE TABLE compliance_requirements (
 id TEXT PRIMARY KEY, building_id TEXT NOT NULL REFERENCES buildings(id), regulatory_reference_id TEXT REFERENCES regulatory_references(id),
 title TEXT NOT NULL, applicable INTEGER NOT NULL CHECK(applicable IN(0,1)), justification TEXT,
 interval_months INTEGER CHECK(interval_months>0), evidence_entity_id TEXT REFERENCES entities(id),
 status TEXT NOT NULL CHECK(status IN('unknown','compliant','non_compliant','not_applicable')), assessed_at TEXT, assessed_by TEXT REFERENCES users(id)
) STRICT;
CREATE TABLE deadlines (
 id TEXT PRIMARY KEY, entity_id TEXT NOT NULL REFERENCES entities(id), building_id TEXT REFERENCES buildings(id),
 kind TEXT NOT NULL, due_on TEXT NOT NULL, recurrence_months INTEGER CHECK(recurrence_months>0), completed_at TEXT,
 responsible_user_id TEXT REFERENCES users(id), UNIQUE(entity_id,kind,due_on)
) STRICT;
CREATE TABLE alert_rules (
 id TEXT PRIMARY KEY, organisation_id TEXT NOT NULL REFERENCES organisations(id), building_id TEXT REFERENCES buildings(id),
 deadline_kind TEXT NOT NULL, enabled INTEGER NOT NULL CHECK(enabled IN(0,1)), severity INTEGER NOT NULL CHECK(severity BETWEEN 1 AND 4)
) STRICT;
CREATE TABLE alert_rule_steps (
 rule_id TEXT NOT NULL REFERENCES alert_rules(id), days_before INTEGER NOT NULL,
 escalation_level INTEGER NOT NULL CHECK(escalation_level>=0), recipient_role TEXT NOT NULL REFERENCES roles(code),
 PRIMARY KEY(rule_id,days_before,escalation_level)
) STRICT;
CREATE TABLE alerts (
 id TEXT PRIMARY KEY, deadline_id TEXT NOT NULL REFERENCES deadlines(id), rule_id TEXT NOT NULL,
 days_before INTEGER NOT NULL, escalation_level INTEGER NOT NULL, created_at TEXT NOT NULL,
 acknowledged_at TEXT, acknowledged_by TEXT REFERENCES users(id), resolved_at TEXT,
 FOREIGN KEY(rule_id,days_before,escalation_level) REFERENCES alert_rule_steps(rule_id,days_before,escalation_level),
 UNIQUE(deadline_id,rule_id,days_before,escalation_level)
) STRICT;
CREATE TABLE notifications (
 id TEXT PRIMARY KEY, alert_id TEXT NOT NULL REFERENCES alerts(id), user_id TEXT NOT NULL REFERENCES users(id),
 state TEXT NOT NULL CHECK(state IN('pending','delivered','failed')), attempts INTEGER NOT NULL DEFAULT 0,
 next_attempt_at TEXT, delivered_at TEXT, last_error TEXT, UNIQUE(alert_id,user_id)
) STRICT;
CREATE TABLE audit_events (
 sequence INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE, occurred_at TEXT NOT NULL,
 actor_id TEXT REFERENCES users(id), organisation_id TEXT REFERENCES organisations(id), building_id TEXT REFERENCES buildings(id),
 action TEXT NOT NULL, entity_id TEXT REFERENCES entities(id), payload_json TEXT NOT NULL CHECK(json_valid(payload_json)),
 previous_hash BLOB NOT NULL, event_hash BLOB NOT NULL UNIQUE, hmac BLOB NOT NULL
) STRICT;
CREATE TABLE backup_schedules (
 id TEXT PRIMARY KEY, organisation_id TEXT NOT NULL REFERENCES organisations(id), destination TEXT NOT NULL,
 local_time TEXT NOT NULL, timezone TEXT NOT NULL DEFAULT 'Europe/Paris', enabled INTEGER NOT NULL CHECK(enabled IN(0,1)),
 retention_count INTEGER NOT NULL CHECK(retention_count>0), last_success_at TEXT, next_run_at TEXT NOT NULL
) STRICT;
CREATE TABLE backup_runs (
 id TEXT PRIMARY KEY, schedule_id TEXT REFERENCES backup_schedules(id), trigger TEXT NOT NULL CHECK(trigger IN('manual','scheduled','pre_update','pre_restore')),
 started_at TEXT NOT NULL, completed_at TEXT, state TEXT NOT NULL CHECK(state IN('running','succeeded','failed')),
 archive_path TEXT, archive_sha256 TEXT, schema_version INTEGER, error TEXT
) STRICT;
CREATE TABLE update_state (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1), channel TEXT NOT NULL CHECK(channel IN('stable','beta')),
 installed_version TEXT NOT NULL, last_checked_at TEXT, candidate_version TEXT, signed_manifest_json TEXT,
 downloaded_path TEXT, artifact_sha256 TEXT, manifest_verified_at TEXT, artifact_verified_at TEXT,
 phase TEXT NOT NULL CHECK(phase IN('idle','checking','downloading','ready','installing','health_check','failed')),
 previous_installer_path TEXT, pre_update_backup_id TEXT REFERENCES backup_runs(id), last_error TEXT
) STRICT;
CREATE TABLE settings (
 key TEXT PRIMARY KEY, value_json TEXT NOT NULL CHECK(json_valid(value_json)), modified_at TEXT NOT NULL, modified_by TEXT REFERENCES users(id)
) STRICT;
CREATE TABLE privacy_requests (
 id TEXT PRIMARY KEY, subject_user_id TEXT REFERENCES users(id), subject_staff_id TEXT REFERENCES safety_staff(id),
 kind TEXT NOT NULL CHECK(kind IN('access','rectification','erasure')), requested_at TEXT NOT NULL, completed_at TEXT,
 outcome TEXT, legal_retention_reason TEXT
) STRICT;
CREATE TABLE processing_records (
 id TEXT PRIMARY KEY, purpose TEXT NOT NULL, legal_basis TEXT NOT NULL, data_categories TEXT NOT NULL,
 retention_policy TEXT NOT NULL, access_roles TEXT NOT NULL, modified_at TEXT NOT NULL
) STRICT;
CREATE TABLE export_runs (
 id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), building_id TEXT REFERENCES buildings(id),
 format TEXT NOT NULL CHECK(format IN('pdf','xlsx','csv','ics','print')), started_at TEXT NOT NULL, completed_at TEXT,
 parameters_json TEXT NOT NULL CHECK(json_valid(parameters_json)), output_sha256 TEXT
) STRICT;
CREATE TABLE import_runs (
 id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), building_id TEXT NOT NULL REFERENCES buildings(id),
 source_file_id TEXT NOT NULL REFERENCES files(id), mapping_json TEXT NOT NULL CHECK(json_valid(mapping_json)),
 started_at TEXT NOT NULL, completed_at TEXT, accepted_rows INTEGER NOT NULL DEFAULT 0, rejected_rows INTEGER NOT NULL DEFAULT 0,
 errors_json TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(errors_json))
) STRICT;
CREATE VIRTUAL TABLE search_index USING fts5(entity_id UNINDEXED, building_id UNINDEXED, title, body, tokenize='unicode61 remove_diacritics 2');
CREATE INDEX idx_entities_scope ON entities(organisation_id,building_id,kind);
CREATE INDEX idx_equipment_location ON equipment(building_id,location_id,category_id);
CREATE INDEX idx_equipment_maintenance ON equipment(next_maintenance_on);
CREATE INDEX idx_equipment_life ON equipment(end_of_life_on);
CREATE INDEX idx_equipment_events ON equipment_events(equipment_id,occurred_at DESC);
CREATE INDEX idx_consumption ON consumption_readings(equipment_id,measured_at);
CREATE INDEX idx_works_scope ON works(building_id,status,planned_start);
CREATE INDEX idx_observations_scope ON observations(building_id,status,due_on);
CREATE INDEX idx_register_scope ON register_entries(building_id,block,effective_on);
CREATE INDEX idx_deadlines_pending ON deadlines(due_on,building_id) WHERE completed_at IS NULL;
CREATE INDEX idx_alerts_open ON alerts(deadline_id,created_at) WHERE resolved_at IS NULL;
CREATE INDEX idx_audit_scope ON audit_events(building_id,occurred_at);
CREATE INDEX idx_document_expiry ON documents(expires_on);
CREATE INDEX idx_contract_visits ON contract_visits(building_id,planned_at);
CREATE INDEX idx_notifications_pending ON notifications(state,next_attempt_at);
CREATE TRIGGER work_status_transition BEFORE UPDATE OF status ON works
WHEN NEW.status<>OLD.status AND NOT (
 (OLD.status='identified' AND NEW.status='scheduled') OR
 (OLD.status='scheduled' AND NEW.status='in_progress') OR
 (OLD.status='in_progress' AND NEW.status='pending_validation') OR
 (OLD.status='pending_validation' AND NEW.status IN('in_progress','closed')) OR
 (OLD.status='closed' AND NEW.status='archived'))
BEGIN SELECT RAISE(ABORT,'Invalid work status transition'); END;
CREATE TRIGGER entity_versions_update_blocked BEFORE UPDATE ON entity_versions BEGIN SELECT RAISE(ABORT, 'Append-only table'); END;
CREATE TRIGGER entity_versions_delete_blocked BEFORE DELETE ON entity_versions BEGIN SELECT RAISE(ABORT, 'Append-only table'); END;
CREATE TRIGGER document_versions_update_blocked BEFORE UPDATE ON document_versions BEGIN SELECT RAISE(ABORT, 'Append-only table'); END;
CREATE TRIGGER document_versions_delete_blocked BEFORE DELETE ON document_versions BEGIN SELECT RAISE(ABORT, 'Append-only table'); END;
CREATE TRIGGER audit_events_update_blocked BEFORE UPDATE ON audit_events BEGIN SELECT RAISE(ABORT, 'Append-only table'); END;
CREATE TRIGGER audit_events_delete_blocked BEFORE DELETE ON audit_events BEGIN SELECT RAISE(ABORT, 'Append-only table'); END;
CREATE TRIGGER equipment_events_update_blocked BEFORE UPDATE ON equipment_events BEGIN SELECT RAISE(ABORT, 'Append-only table'); END;
CREATE TRIGGER equipment_events_delete_blocked BEFORE DELETE ON equipment_events BEGIN SELECT RAISE(ABORT, 'Append-only table'); END;
CREATE TRIGGER work_status_events_update_blocked BEFORE UPDATE ON work_status_events BEGIN SELECT RAISE(ABORT, 'Append-only table'); END;
CREATE TRIGGER work_status_events_delete_blocked BEFORE DELETE ON work_status_events BEGIN SELECT RAISE(ABORT, 'Append-only table'); END;
CREATE TRIGGER stock_movements_update_blocked BEFORE UPDATE ON stock_movements BEGIN SELECT RAISE(ABORT, 'Append-only table'); END;
CREATE TRIGGER stock_movements_delete_blocked BEFORE DELETE ON stock_movements BEGIN SELECT RAISE(ABORT, 'Append-only table'); END;

