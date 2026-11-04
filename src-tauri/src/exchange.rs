//! Échange de clichés complets d’un bâtiment, chiffrés et intégrés sans écrasement.
use crate::{audit, auth, crypto, db, vault, Core};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{Column, Row, TypeInfo};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
const LIMIT: usize = 256 * 1024 * 1024;
const TABLES: &[&str] = &[
    "equipment_categories",
    "locations",
    "equipment",
    "equipment_events",
    "equipment_photos",
    "consumption_readings",
    "consumption_cancellations",
    "work_equipment",
    "inspection_equipment",
    "entities",
    "buildings",
    "building_erp_types",
    "registers",
    "register_entries",
    "entity_versions",
    "entity_files",
    "instructions",
    "safety_staff",
    "staff_training",
    "commission_visits",
    "inspections",
    "observations",
    "fire_exercises",
    "see_solutions",
    "register_observations",
    "works",
    "work_dependencies",
    "work_status_events",
    "work_costs",
    "work_cost_reversals",
    "work_contacts",
    "work_documents",
    "work_observation_sync",
    "work_participants",
    "providers",
    "provider_credentials",
    "contracts",
    "contract_buildings",
    "contract_equipment",
    "contract_visits",
    "contract_files",
    "documents",
    "document_versions",
    "document_links",
    "compliance_requirements",
    "regulatory_references",
    "deadlines",
    "manual_deadlines",
];
type Tables = BTreeMap<String, Vec<BTreeMap<String, Value>>>;
#[derive(Serialize, Deserialize, Clone)]
pub struct Attachment {
    id: String,
    name: String,
    mime: String,
    sha256: String,
    bytes: String,
}
#[derive(Serialize, Deserialize, Clone)]
pub struct Package {
    format: u32,
    schema: i64,
    lineage: String,
    building: String,
    name: String,
    created_at: String,
    tables: Tables,
    authors: BTreeMap<String, String>,
    files: Vec<Attachment>,
}
#[derive(Serialize)]
pub struct Preview {
    pub path: String,
    pub digest: String,
    pub name: String,
    pub created_at: String,
    pub records: usize,
    pub documents: usize,
    pub existing: Option<String>,
    pub existing_name: Option<String>,
    pub local_fingerprint: Option<String>,
    pub already_received: bool,
    pub local_changed: bool,
}
fn hash(v: &[u8]) -> String {
    hex::encode(Sha256::digest(v))
}
fn content_hash(p: &Package) -> Result<String> {
    Ok(hash(&serde_json::to_vec(&(
        &p.lineage,
        &p.building,
        &p.tables,
        &p.authors,
        &p.files,
    ))?))
}
fn strings(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::String(s) => {
            out.insert(s.clone());
            if let Ok(v) = serde_json::from_str::<Value>(s) {
                if !v.is_string() {
                    strings(&v, out)
                }
            }
        }
        Value::Array(a) => {
            for v in a {
                strings(v, out)
            }
        }
        Value::Object(o) => {
            for v in o.values() {
                strings(v, out)
            }
        }
        _ => {}
    }
}
async fn rows(c: &Core, table: &str) -> Result<Vec<BTreeMap<String, Value>>> {
    let mut out = vec![];
    for row in sqlx::query(&format!("SELECT * FROM {table}"))
        .fetch_all(&c.pool)
        .await?
    {
        let mut obj = BTreeMap::new();
        for col in row.columns() {
            let name = col.name();
            let value = match col.type_info().name() {
                "INTEGER" => row.try_get::<Option<i64>, _>(name)?.map(Value::from),
                "REAL" => row.try_get::<Option<f64>, _>(name)?.map(Value::from),
                "BLOB" => bail!("Colonne binaire non prévue : {table}.{name}"),
                _ => row.try_get::<Option<String>, _>(name)?.map(Value::String),
            };
            obj.insert(name.to_owned(), value.unwrap_or(Value::Null));
        }
        out.push(obj);
    }
    out.sort_by_key(|r| serde_json::to_string(r).unwrap_or_default());
    Ok(out)
}
pub async fn access(c: &Core, token: &str) -> Result<String> {
    let actor = auth::require(c, token, Some("backup.manage")).await?;
    let n:i64=sqlx::query_scalar("SELECT count(*) FROM role_grants WHERE user_id=? AND role_code='exploitant' AND building_id IS NULL AND (expires_at IS NULL OR expires_at>?)").bind(&actor).bind(db::now()).fetch_one(&c.pool).await?;
    if n == 0 {
        bail!("Le partage complet est réservé à l’exploitant de l’organisation.")
    }
    Ok(actor)
}
async fn collect(c: &Core, building: &str) -> Result<Package> {
    let b = sqlx::query("SELECT organisation_id,name FROM buildings WHERE id=?")
        .bind(building)
        .fetch_one(&c.pool)
        .await?;
    let org: String = b.get(0);
    let name: String = b.get(1);
    let lineage: Option<String> =
        sqlx::query_scalar("SELECT lineage FROM exchange_origins WHERE building_id=?")
            .bind(building)
            .fetch_optional(&c.pool)
            .await?;
    let mut all = BTreeMap::new();
    for t in TABLES {
        all.insert(*t, rows(c, t).await?);
    }
    let mut ids = BTreeSet::from([building.to_owned()]);
    let mut selected: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
    // Racines locales, puis fermeture sur les relations et les identifiants référencés.
    for _ in 0..40 {
        let old = (ids.len(), selected.values().map(|s| s.len()).sum::<usize>());
        for (t, rs) in &all {
            for (i, row) in rs.iter().enumerate() {
                let belongs = row.get("building_id").and_then(Value::as_str) == Some(building);
                let referenced = row
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| ids.contains(id));
                let child = [
                    "entity_id",
                    "entry_id",
                    "work_id",
                    "staff_id",
                    "observation_id",
                    "cost_id",
                    "equipment_id",
                    "reading_id",
                    "inspection_id",
                    "contract_id",
                    "document_id",
                    "deadline_id",
                ]
                .iter()
                .any(|k| {
                    row.get(*k)
                        .and_then(Value::as_str)
                        .is_some_and(|id| ids.contains(id))
                });
                let child = child
                    || (*t == "provider_credentials"
                        && row
                            .get("provider_id")
                            .and_then(Value::as_str)
                            .is_some_and(|id| ids.contains(id)));
                if (*t == "buildings" && row.get("id").and_then(Value::as_str) != Some(building))
                    || (!belongs && !referenced && !child)
                {
                    continue;
                }
                if let Some(other) = row.get("building_id").and_then(Value::as_str) {
                    if other != building {
                        bail!("Une dépendance externe au bâtiment empêche cet export.")
                    }
                }
                if let Some(other) = row.get("organisation_id").and_then(Value::as_str) {
                    if other != org {
                        bail!("Relation inter-organisation interdite.")
                    }
                }
                selected.entry(t).or_default().insert(i);
                for (k, v) in row {
                    if k == "id" || k.ends_with("_id") || k.ends_with("_by") || k.ends_with("_json")
                    {
                        strings(v, &mut ids)
                    }
                }
            }
        }
        let new = (ids.len(), selected.values().map(|s| s.len()).sum::<usize>());
        if old == new {
            break;
        }
    }
    let mut tables = Tables::new();
    for (t, set) in selected {
        tables.insert(
            t.to_owned(),
            set.into_iter().map(|i| all[t][i].clone()).collect(),
        );
    }
    let entities = tables.get("entities").context("Bâtiment absent")?;
    if entities.iter().any(|e| {
        !matches!(
            e.get("kind").and_then(Value::as_str),
            Some(
                "equipment"
                    | "building"
                    | "register_entry"
                    | "work"
                    | "observation"
                    | "inspection"
                    | "staff"
                    | "exercise"
                    | "see"
                    | "provider"
                    | "contract"
                    | "document"
            )
        )
    }) {
        bail!("Ce bâtiment contient un module non encore pris en charge par le partage. Aucun export incomplet n’a été produit.")
    }
    // Transformer les affectations de comptes en contacts, sans transmettre les accès.
    let users = sqlx::query("SELECT id,display_name FROM users WHERE organisation_id=?")
        .bind(&org)
        .fetch_all(&c.pool)
        .await?;
    let authors: BTreeMap<String, String> = users
        .into_iter()
        .filter_map(|r| {
            let id: String = r.get(0);
            ids.contains(&id).then(|| (id, r.get(1)))
        })
        .collect();
    let mut files = vec![];
    let mut total = 0usize;
    for r in sqlx::query(
        "SELECT id,original_name,mime_type,plaintext_sha256 FROM files WHERE organisation_id=?",
    )
    .bind(&org)
    .fetch_all(&c.pool)
    .await?
    {
        let id: String = r.get(0);
        if !ids.contains(&id) {
            continue;
        }
        let bytes = vault::read(c, &id, &org).await?;
        total += bytes.len();
        if total > 120 * 1024 * 1024 {
            bail!("Le partage dépasse 120 Mio de pièces jointes. Utilisez une sauvegarde complète.")
        }
        files.push(Attachment {
            id,
            name: r.get(1),
            mime: r.get(2),
            sha256: r.get(3),
            bytes: B64.encode(&bytes),
        });
    }
    files.sort_by(|a, b| a.id.cmp(&b.id));
    let p = Package {
        format: 1,
        schema: db::SCHEMA_VERSION,
        lineage: lineage.unwrap_or_else(|| building.into()),
        building: building.into(),
        name,
        created_at: db::now(),
        tables,
        authors,
        files,
    };
    validate(&p)?;
    Ok(p)
}
fn validate(p: &Package) -> Result<()> {
    if p.format != 1 || p.schema != db::SCHEMA_VERSION {
        bail!("Version d’échange incompatible. Mettez les deux applications à la même version.")
    }
    uuid::Uuid::parse_str(&p.lineage)?;
    uuid::Uuid::parse_str(&p.building)?;
    if p.tables.values().map(Vec::len).sum::<usize>() > 50000
        || p.files.len() > 2000
        || p.authors.len() > 500
    {
        bail!("Échange trop volumineux.")
    }
    if p.tables.keys().any(|t| !TABLES.contains(&t.as_str())) {
        bail!("Table interdite dans l’échange.")
    }
    let b = p
        .tables
        .get("buildings")
        .context("Fiche bâtiment absente")?;
    if b.len() != 1 || b[0].get("id").and_then(Value::as_str) != Some(&p.building) {
        bail!("L’archive doit contenir un seul bâtiment.")
    }
    if let Some(text) = b[0].get("location_json").and_then(Value::as_str) {
        let location: crate::maps::Location = serde_json::from_str(text)?;
        crate::maps::validate(&Some(location))?;
    }
    for (table, max_depth) in [("locations", 3usize), ("equipment_categories", 2usize)] {
        if let Some(rows) = p.tables.get(table) {
            let index: std::collections::BTreeMap<&str, _> = rows
                .iter()
                .filter_map(|r| r.get("id").and_then(Value::as_str).map(|id| (id, r)))
                .collect();
            for row in rows {
                let mut cursor = Some(row);
                let mut seen = std::collections::BTreeSet::new();
                while let Some(current) = cursor {
                    let id = current["id"]
                        .as_str()
                        .context("Emplacement ou catégorie invalide")?;
                    if !seen.insert(id) || seen.len() > max_depth {
                        bail!("Hiérarchie d’inventaire invalide ou circulaire.")
                    }
                    cursor = if let Some(parent) = current["parent_id"].as_str() {
                        Some(*index.get(parent).context("Parent d’inventaire absent")?)
                    } else {
                        None
                    };
                    if table == "locations" {
                        if let Some(parent) = cursor {
                            let rank = |v: &Value| match v.as_str() {
                                Some("level") => 0,
                                Some("zone") => 1,
                                Some("room") => 2,
                                _ => 9,
                            };
                            if rank(&parent["kind"]) >= rank(&current["kind"]) {
                                bail!("Ordre des niveaux/zones/pièces invalide.")
                            }
                        }
                    }
                }
            }
        }
    }
    let org = b[0]
        .get("organisation_id")
        .and_then(Value::as_str)
        .context("Organisation absente")?;
    let ids: BTreeSet<&str> = p
        .tables
        .values()
        .flatten()
        .filter_map(|r| r.get("id").and_then(Value::as_str))
        .collect();
    for rs in p.tables.values() {
        for r in rs {
            if r.get("building_id")
                .and_then(Value::as_str)
                .is_some_and(|s| s != p.building)
                || r.get("organisation_id")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s != org)
            {
                bail!("Périmètre de bâtiment invalide.")
            }
            if r.get("entity_id")
                .and_then(Value::as_str)
                .is_some_and(|id| !ids.contains(id))
            {
                bail!("Entité externe interdite.")
            }
        }
    }
    if p.authors
        .keys()
        .any(|id| ids.contains(id.as_str()) || id == org)
        || p.files
            .iter()
            .any(|f| ids.contains(f.id.as_str()) || p.authors.contains_key(&f.id) || f.id == org)
    {
        bail!("Identifiants de types incompatibles.")
    }
    let mut file_ids = BTreeSet::new();
    for f in &p.files {
        uuid::Uuid::parse_str(&f.id)?;
        if !file_ids.insert(&f.id) || f.name.len() > 255 || f.name.contains(['\\', '/', '\0']) {
            bail!("Pièce jointe invalide.")
        }
        let b = B64.decode(&f.bytes)?;
        if hash(&b) != f.sha256 {
            bail!("Pièce jointe altérée.")
        }
    }
    Ok(())
}
pub async fn export(c: &Core, token: &str, id: &str, password: &str, path: &Path) -> Result<()> {
    let actor = access(c, token).await?;
    if !crate::dashboard::allowed(c, &actor, id, "buildings.read").await? {
        bail!("Bâtiment non autorisé")
    }
    let p = collect(c, id).await?;
    let bytes = serde_json::to_vec(&p)?;
    if bytes.len() > LIMIT {
        bail!("Échange trop volumineux")
    }
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let k = crypto::derive_backup_key(password, &salt)?;
    let mut out = b"ERP-X001".to_vec();
    out.extend(salt);
    out.extend(crypto::seal(&k, &bytes, b"erp.exchange.v1")?);
    if path.starts_with(&c.root) {
        bail!("Choisissez un dossier d’échange extérieur au stockage interne.")
    }
    db::atomic_write(path, &out)?;
    let mut tx = c.pool.begin().await?;
    sqlx::query("INSERT INTO exchange_origins(lineage,building_id,updated_at) VALUES(?,?,?) ON CONFLICT(lineage) DO NOTHING").bind(&p.lineage).bind(id).bind(db::now()).execute(&mut *tx).await?;
    audit::append(
        &mut tx,
        &c.key,
        Some(&actor),
        "building.export",
        json!({"building":id,"digest":content_hash(&p)?}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}
fn read(path: &Path, password: &str) -> Result<Package> {
    if std::fs::metadata(path)?.len() > LIMIT as u64 {
        bail!("Échange trop volumineux")
    }
    let b = std::fs::read(path)?;
    if b.len() < 52 || &b[..8] != b"ERP-X001" {
        bail!("Ce fichier n’est pas un partage de bâtiment.")
    }
    let k = crypto::derive_backup_key(password, &b[8..24])?;
    let plain = crypto::unseal(&k, &b[24..], b"erp.exchange.v1")?;
    let p = serde_json::from_slice(&plain)?;
    validate(&p)?;
    Ok(p)
}
pub async fn preview(c: &Core, token: &str, path: &Path, password: &str) -> Result<Preview> {
    access(c, token).await?;
    let p = read(path, password)?;
    preview_package(c, path, &p).await
}
async fn preview_package(c: &Core, path: &Path, p: &Package) -> Result<Preview> {
    let digest = content_hash(p)?;
    let row=sqlx::query("SELECT x.building_id,b.name,x.last_received_hash,x.local_fingerprint FROM exchange_origins x JOIN buildings b ON b.id=x.building_id WHERE lineage=?").bind(&p.lineage).fetch_optional(&c.pool).await?;
    let mut v = Preview {
        path: path.to_string_lossy().into_owned(),
        digest: digest.clone(),
        name: p.name.clone(),
        created_at: p.created_at.clone(),
        records: p.tables.get("register_entries").map_or(0, Vec::len),
        documents: p.files.len(),
        existing: None,
        existing_name: None,
        local_fingerprint: None,
        already_received: false,
        local_changed: false,
    };
    if let Some(r) = row {
        let id: String = r.get(0);
        let current = content_hash(&collect(c, &id).await?)?;
        v.local_changed = r.get::<Option<String>, _>(3).is_some_and(|s| s != current);
        v.already_received = r.get::<Option<String>, _>(2).as_deref() == Some(&digest);
        v.existing = Some(id);
        v.existing_name = Some(r.get(1));
        v.local_fingerprint = Some(current);
    }
    Ok(v)
}
fn remap(v: &Value, map: &BTreeMap<String, String>) -> Value {
    match v {
        Value::String(s) => {
            if let Some(id) = map.get(s) {
                return Value::String(id.clone());
            }
            if let Ok(j) = serde_json::from_str::<Value>(s) {
                if j.is_object() || j.is_array() {
                    return Value::String(remap(&j, map).to_string());
                }
            }
            v.clone()
        }
        Value::Array(a) => Value::Array(a.iter().map(|v| remap(v, map)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        if k == "asset_key" {
                            v.clone()
                        } else {
                            remap(v, map)
                        },
                    )
                })
                .collect(),
        ),
        _ => v.clone(),
    }
}
pub async fn import(
    c: &Core,
    token: &str,
    path: &Path,
    password: &str,
    digest: &str,
    expected: Option<&str>,
    replace: bool,
) -> Result<String> {
    crate::shared_workspace::require_write(c)?;
    let actor = access(c, token).await?;
    let p = read(path, password)?;
    let preview = preview_package(c, path, &p).await?;
    if preview.digest != digest || preview.local_fingerprint.as_deref() != expected {
        bail!("Le fichier ou les données locales ont changé. Recommencez l’aperçu.")
    }
    if preview.already_received {
        bail!("Ce contenu a déjà été reçu. Aucun doublon créé.")
    }
    if preview.existing.is_some() && !replace {
        bail!("Conservez votre version ou confirmez l’utilisation de la version reçue.")
    }
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
        .bind(&actor)
        .fetch_one(&c.pool)
        .await?;
    let source_org = p.tables["buildings"][0]["organisation_id"]
        .as_str()
        .context("Organisation absente")?;
    let mut map = BTreeMap::from([(source_org.to_owned(), org.clone())]);
    for row in p.tables.values().flatten() {
        if let Some(id) = row.get("id").and_then(Value::as_str) {
            uuid::Uuid::parse_str(id)?;
            map.insert(id.into(), db::id());
        }
    }
    for f in &p.files {
        map.insert(f.id.clone(), db::id());
    }
    for id in p.authors.keys() {
        map.insert(id.clone(), db::id());
    }
    let mut reused = std::collections::BTreeSet::new();
    if let Some(categories) = p.tables.get("equipment_categories") {
        for row in categories {
            let code = row["code"].as_str().context("Code de catégorie absent")?;
            if let Some(r) = sqlx::query("SELECT id,name FROM equipment_categories WHERE code=?")
                .bind(code)
                .fetch_optional(&c.pool)
                .await?
            {
                if r.get::<String, _>(1)
                    != row["name"].as_str().context("Nom de catégorie absent")?
                {
                    bail!("Une catégorie existante porte un autre nom. Import interrompu sans modification.")
                }
                let source = row["id"].as_str().context("Catégorie invalide")?;
                map.insert(source.into(), r.get(0));
                reused.insert(source.to_string());
            }
        }
        for row in categories {
            let source = row["id"].as_str().unwrap();
            if reused.contains(source) {
                let parent: Option<String> =
                    sqlx::query_scalar("SELECT parent_id FROM equipment_categories WHERE id=?")
                        .bind(&map[source])
                        .fetch_one(&c.pool)
                        .await?;
                let incoming = row["parent_id"]
                    .as_str()
                    .map(|id| map.get(id).cloned().context("Parent de catégorie absent"))
                    .transpose()?;
                if parent != incoming {
                    bail!("Arborescence de catégories incompatible.")
                }
            }
        }
    }
    if let Some(equipment) = p.tables.get("equipment") {
        let mut assets = std::collections::BTreeSet::new();
        for row in equipment {
            let asset = row["asset_key"].as_str().context("Identifiant QR absent")?;
            uuid::Uuid::parse_str(asset)?;
            if !assets.insert(asset) {
                bail!("Identifiant QR dupliqué dans le fichier.")
            }
            let taken:i64=sqlx::query_scalar("SELECT count(*) FROM equipment q JOIN entities e ON e.id=q.id JOIN entities b ON b.id=q.building_id WHERE q.asset_key=? AND e.archived_at IS NULL AND b.archived_at IS NULL AND (? IS NULL OR q.building_id<>?)").bind(asset).bind(&preview.existing).bind(&preview.existing).fetch_one(&c.pool).await?;
            if taken > 0 {
                bail!("Un équipement actif d’un autre bâtiment utilise déjà ce QR. Aucune donnée intégrée.")
            }
        }
    }
    let new_id = map.get(&p.building).context("Bâtiment manquant")?.clone();
    let mut tx = c.pool.begin().await?;
    sqlx::query("PRAGMA defer_foreign_keys=ON")
        .execute(&mut *tx)
        .await?;
    let mut written = vec![];
    let result:Result<()>=async{
 for (id,name) in &p.authors{
 sqlx::query("INSERT INTO users(id,organisation_id,login,display_name,password_hash,active,created_at,is_external) VALUES(?,?,?,?,?,0,?,1)").bind(&map[id]).bind(&org).bind(format!("recu-{}",map[id])).bind(format!("Reçu · {name}")).bind(&c.dummy_hash).bind(db::now()).execute(&mut *tx).await?;
 }
 for f in &p.files{
 let id=&map[&f.id];let key=crypto::random_key();let bytes=B64.decode(&f.bytes)?;let sealed=crypto::seal(&key,&bytes,id.as_bytes())?;let storage=format!("{id}.blob");let file=c.root.join("files").join(&storage);
 db::atomic_write(&file,&sealed)?;written.push(file);
 sqlx::query("INSERT INTO files(id,organisation_id,storage_key,original_name,mime_type,plaintext_sha256,byte_size,encryption_version,nonce,key_envelope,created_at,created_by) VALUES(?,?,?,?,?,?,?,1,?,?,?,?)").bind(id).bind(&org).bind(storage).bind(&f.name).bind(&f.mime).bind(&f.sha256).bind(bytes.len() as i64).bind(&sealed[..12]).bind(crypto::seal(&c.key,&key,format!("file:{id}").as_bytes())?).bind(db::now()).bind(&actor).execute(&mut *tx).await?;
 }
 for (table,rs) in &p.tables{
 let columns=sqlx::query(&format!("PRAGMA table_info({table})")).fetch_all(&mut *tx).await?;
 let names:BTreeSet<String>=columns.iter().map(|r|r.get(1)).collect();
 for row in rs{
 if table=="equipment_categories"&&row.get("id").and_then(Value::as_str).is_some_and(|id|reused.contains(id)){continue}
 if row.len()!=names.len()||row.keys().any(|k|!names.contains(k)){bail!("Colonnes incompatibles dans {table}")}
 let fks=sqlx::query(&format!("PRAGMA foreign_key_list({table})")).fetch_all(&mut *tx).await?;
 for fk in fks{let target:String=fk.get(2);let col:String=fk.get(3);if let Some(value)=row.get(&col).and_then(Value::as_str){if !["erp_types","document_categories"].contains(&target.as_str())&&!map.contains_key(value){bail!("Relation externe interdite : {table}.{col}")}}}
 let keys:Vec<&str>=row.keys().map(String::as_str).collect();let sql=format!("INSERT INTO {table} ({}) VALUES ({})",keys.join(","),vec!["?";keys.len()].join(","));let mut q=sqlx::query(&sql);
 for (column,val) in row.iter(){let v=if table=="equipment"&&column=="qr_token"{Value::String(db::id())}else if column=="asset_key"{val.clone()}else{remap(val,&map)};q=match v{Value::Null=>q.bind(Option::<String>::None),Value::Number(n)=>q.bind(n.as_i64().context("Nombre invalide")?),Value::String(s)=>q.bind(s),_=>bail!("Valeur SQL interdite")};}
 q.execute(&mut *tx).await?;
 }}
 if let Some(old)=&preview.existing{
 sqlx::query("UPDATE entities SET archived_at=? WHERE id=? OR building_id=?").bind(db::now()).bind(old).bind(old).execute(&mut *tx).await?;
 }
 sqlx::query("INSERT INTO exchange_origins(lineage,building_id,last_received_hash,updated_at) VALUES(?,?,?,?) ON CONFLICT(lineage) DO UPDATE SET building_id=excluded.building_id,last_received_hash=excluded.last_received_hash,local_fingerprint=NULL,updated_at=excluded.updated_at").bind(&p.lineage).bind(&new_id).bind(digest).bind(db::now()).execute(&mut *tx).await?;
 sqlx::query("INSERT INTO exchange_receipts(id,lineage,building_id,previous_building_id,received_hash,source_created_at,imported_at,imported_by,summary_json) VALUES(?,?,?,?,?,?,?,?,?)").bind(db::id()).bind(&p.lineage).bind(&new_id).bind(&preview.existing).bind(digest).bind(&p.created_at).bind(db::now()).bind(&actor).bind(json!({"authors":p.authors,"name":p.name,"entries":preview.records,"files":preview.documents}).to_string()).execute(&mut *tx).await?;
 audit::append(&mut tx,&c.key,Some(&actor),"building.import",json!({"lineage":p.lineage,"building":new_id,"previous":preview.existing,"digest":digest})).await?;
 if !sqlx::query("PRAGMA foreign_key_check").fetch_all(&mut *tx).await?.is_empty(){bail!("Relations du fichier incompatibles. Aucune donnée intégrée.")}
 Ok(())}.await;
    if let Err(e) = result {
        tx.rollback().await?;
        for f in written {
            let _ = std::fs::remove_file(f);
        }
        return Err(e);
    }
    if let Err(e) = tx.commit().await {
        for f in written {
            let _ = std::fs::remove_file(f);
        }
        return Err(e.into());
    }
    let local = content_hash(&collect(c, &new_id).await?)?;
    sqlx::query("UPDATE exchange_origins SET local_fingerprint=? WHERE lineage=?")
        .bind(local)
        .bind(&p.lineage)
        .execute(&c.pool)
        .await?;
    audit::checkpoint(&c.pool, &c.root).await?;
    Ok(new_id)
}

#[cfg(all(test, windows))]
#[path = "exchange_tests.rs"]
mod tests;
