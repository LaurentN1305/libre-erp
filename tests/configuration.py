"""Contrôles reproductibles SQL/configuration sans ouverture des données utilisateur."""
from pathlib import Path
import json
import sqlite3
import tomllib

root = Path(__file__).resolve().parents[1]
db = sqlite3.connect(':memory:')
db.execute('PRAGMA foreign_keys=ON')
for migration in sorted((root / 'src-tauri/migrations').glob('*.sql')):
    db.executescript('BEGIN;\n' + migration.read_text(encoding='utf-8') + '\nCOMMIT;')
assert db.execute('PRAGMA foreign_key_check').fetchall() == []
assert db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
commission = set(x[0] for x in db.execute("SELECT permission_code FROM role_permissions WHERE role_code='commission'"))
assert commission == {'settings.read', 'register.read', 'documents.read', 'buildings.read', 'work.read', 'inventory.read'}
admin = set(x[0] for x in db.execute("SELECT permission_code FROM role_permissions WHERE role_code='exploitant'"))
assert {'users.manage', 'backup.manage', 'updates.manage', 'audit.read'} <= admin
cargo = tomllib.loads((root / 'src-tauri/Cargo.toml').read_text(encoding='utf-8'))
assert 'bundled-sqlcipher-vendored-openssl' in cargo['dependencies']['libsqlite3-sys']['features']
config = json.loads((root / 'src-tauri/tauri.conf.json').read_text(encoding='utf-8-sig'))
assert config['bundle']['targets'] == ['nsis', 'msi']
assert config['bundle']['windows']['webviewInstallMode']['type'] == 'offlineInstaller'
cap = json.loads((root / 'src-tauri/capabilities/main.json').read_text(encoding='utf-8'))
assert not any(p.startswith(('sql:', 'fs:', 'http:', 'updater:')) for p in cap['permissions'])
try:
    import jsonschema
    schema = json.loads((root / 'architecture/tauri/config.schema.json').read_text(encoding='utf-8'))
    jsonschema.validate(config, schema)
    schema_status = 'passed'
except ImportError:
    schema_status = 'not executed: jsonschema unavailable'
result = {'sqlite_migrations': 'passed', 'rbac_commission': 'passed', 'rbac_admin': 'passed', 'sqlcipher_feature': 'passed', 'tauri_json_schema': schema_status, 'frontend_permissions': 'passed'}
print(json.dumps(result, indent=2))
