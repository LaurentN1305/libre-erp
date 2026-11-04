use crate::{audit, auth, dashboard, db, vault, Core};
use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use std::collections::BTreeMap;
pub fn schema() -> Value {
    serde_json::from_str(include_str!("../../src/register-schema.json")).expect("Schéma embarqué")
}
#[derive(Deserialize, Serialize, Clone)]
pub struct Input {
    pub id: Option<String>,
    pub building_id: String,
    pub block: String,
    pub title: String,
    pub effective_on: String,
    pub applicability: String,
    pub justification: String,
    pub fields: BTreeMap<String, String>,
    pub files: Vec<String>,
    pub base_version: i64,
    pub reason: String,
}
pub async fn access(core: &Core, token: &str, building: &str, write: bool) -> Result<String> {
    let actor = auth::require(core, token, None).await?;
    if !dashboard::allowed(
        core,
        &actor,
        building,
        if write {
            "register.write"
        } else {
            "register.read"
        },
    )
    .await?
    {
        bail!("Accès au registre refusé pour ce bâtiment.");
    }
    if write {
        let archived: Option<String> =
            sqlx::query_scalar("SELECT archived_at FROM entities WHERE id=?")
                .bind(building)
                .fetch_one(&core.pool)
                .await?;
        if archived.is_some() {
            bail!("Réactivez le bâtiment avant de modifier son registre.");
        }
    }
    Ok(actor)
}
fn validate(input: &Input) -> Result<()> {
    if input.title.trim().is_empty()
        || input.title.len() > 250
        || input.reason.trim().is_empty()
        || input.reason.len() > 1000
        || input.justification.len() > 10000
        || input.files.len() > 50
        || input.base_version < 0
    {
        bail!("Titre et motif de modification requis ; vérifiez les limites de saisie.");
    }
    chrono::NaiveDate::parse_from_str(&input.effective_on, "%Y-%m-%d")?;
    if !["applicable", "not_applicable", "to_confirm"].contains(&input.applicability.as_str()) {
        bail!("Applicabilité inconnue.");
    }
    if input.applicability != "applicable" && input.justification.trim().is_empty() {
        bail!("Une justification est requise pour une rubrique non applicable ou à confirmer.");
    }
    let all = schema();
    let block = all
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["code"] == input.block)
        .ok_or_else(|| anyhow::anyhow!("Bloc inconnu"))?;
    let fields = block["fields"].as_array().unwrap();
    for (key, value) in &input.fields {
        if value.len() > 30000 || !fields.iter().any(|f| f["key"] == *key) {
            bail!("Champ inconnu ou trop long.");
        }
    }
    for f in fields {
        let key = f["key"].as_str().unwrap();
        let value = input.fields.get(key).map(|x| x.trim()).unwrap_or("");
        if input.applicability == "applicable" && f["required"] == true && value.is_empty() {
            bail!("Champ requis : {}", f["label"].as_str().unwrap());
        }
        if !value.is_empty() {
            match f["kind"].as_str().unwrap() {
                "date" => {
                    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")?;
                }
                "number" => {
                    let n: i64 = value.parse()?;
                    if !(0..=1_000_000).contains(&n) {
                        bail!("Nombre hors limites.");
                    }
                }
                _ => {}
            }
        }
    }
    if input.block == "E"
        && input.applicability == "applicable"
        && !input
            .fields
            .get("statut")
            .is_some_and(|s| ["actif", "suspendu", "parti"].contains(&s.as_str()))
    {
        bail!("Statut du personnel : actif, suspendu ou parti.");
    }
    if input.applicability == "applicable" && ["C", "G"].contains(&input.block.as_str()) {
        let suivi = input.fields.get("suivi").map(String::as_str).unwrap_or("");
        if !["none", "open", "resolved"].contains(&suivi) {
            bail!("Choisissez le suivi de l’observation.");
        }
        if suivi != "none" {
            let severity = input
                .fields
                .get("gravite")
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            if !(1..=4).contains(&severity) {
                bail!("Renseignez une gravité de 1 à 4 pour l’observation.");
            }
        }
        if input.block == "C"
            && !input
                .fields
                .get("type_controle")
                .is_some_and(|s| ["commission", "verification"].contains(&s.as_str()))
        {
            bail!("Choisissez la nature du passage.");
        }
    }

    Ok(())
}
async fn entry(core: &Core, id: &str) -> Result<Value> {
    let r=sqlx::query("SELECT r.id,r.building_id,r.block,r.title,r.current_version,r.effective_on,r.applicability,r.justification,v.payload_json,v.recorded_at,u.display_name,v.reason,e.archived_at FROM register_entries r JOIN entities e ON e.id=r.id JOIN entity_versions v ON v.entity_id=r.id AND v.version=r.current_version JOIN users u ON u.id=v.recorded_by WHERE r.id=?").bind(id).fetch_one(&core.pool).await?;
    let payload: Value = serde_json::from_str(&r.get::<String, _>(8))?;
    Ok(
        json!({"id":r.get::<String,_>(0),"building_id":r.get::<String,_>(1),"block":r.get::<String,_>(2),"title":r.get::<String,_>(3),"version":r.get::<i64,_>(4),"effective_on":r.get::<String,_>(5),"applicability":r.get::<String,_>(6),"justification":r.get::<Option<String>,_>(7),"fields":payload["fields"],"files":payload["files"],"recorded_at":r.get::<String,_>(9),"actor":r.get::<String,_>(10),"reason":r.get::<String,_>(11),"archived":r.get::<Option<String>,_>(12).is_some()}),
    )
}
pub async fn snapshot(core: &Core, token: &str, building: &str) -> Result<Value> {
    let actor = access(core, token, building, false).await?;
    let r=sqlx::query("SELECT b.name,b.address,b.erp_category,b.capacity,u.display_name,e.archived_at,o.name AS organisation,b.opened_on FROM buildings b JOIN entities e ON e.id=b.id JOIN organisations o ON o.id=b.organisation_id LEFT JOIN users u ON u.id=b.safety_manager_id WHERE b.id=?").bind(building).fetch_one(&core.pool).await?;
    let types:Vec<String>=sqlx::query_scalar("SELECT type_code FROM building_erp_types WHERE building_id=? ORDER BY is_primary DESC,type_code").bind(building).fetch_all(&core.pool).await?;
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM register_entries WHERE building_id=? ORDER BY block,effective_on DESC,id",
    )
    .bind(building)
    .fetch_all(&core.pool)
    .await?;
    let mut entries = vec![];
    for id in ids {
        entries.push(entry(core, &id).await?);
    }
    let files=sqlx::query("SELECT DISTINCT f.id,f.original_name,f.mime_type,f.byte_size FROM files f JOIN entity_files ef ON ef.file_id=f.id JOIN register_entries r ON r.id=ef.entity_id WHERE r.building_id=?").bind(building).fetch_all(&core.pool).await?.into_iter().map(|f|json!({"id":f.get::<String,_>(0),"name":f.get::<String,_>(1),"mime":f.get::<String,_>(2),"size":f.get::<i64,_>(3)})).collect::<Vec<_>>();
    let writable = dashboard::allowed(core, &actor, building, "register.write").await?
        && r.get::<Option<String>, _>(5).is_none();
    Ok(
        json!({"building":{"id":building,"name":r.get::<String,_>(0),"address":r.get::<String,_>(1),"category":r.get::<i64,_>(2),"capacity":r.get::<i64,_>(3),"manager":r.get::<Option<String>,_>(4),"organisation":r.get::<String,_>(6),"opened_on":r.get::<Option<String>,_>(7),"types":types},"entries":entries,"files":files,"writable":writable,"generated_at":db::now(),"schema":schema()}),
    )
}
pub async fn save(core: &Core, token: &str, input: Input) -> Result<String> {
    crate::shared_workspace::require_write(core)?;
    let actor = access(core, token, &input.building_id, true).await?;
    validate(&input)?;
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
        .bind(&actor)
        .fetch_one(&core.pool)
        .await?;
    let id = input.id.clone().unwrap_or_else(db::id);
    if input.id.is_some() {
        let row=sqlx::query("SELECT r.current_version,r.building_id,r.block,e.archived_at FROM register_entries r JOIN entities e ON e.id=r.id WHERE r.id=?").bind(&id).fetch_one(&core.pool).await?;
        if row.get::<String, _>(1) != input.building_id
            || row.get::<String, _>(2) != input.block
            || row.get::<i64, _>(0) != input.base_version
            || row.get::<Option<String>, _>(3).is_some()
        {
            bail!("Entrée modifiée ou archivée. Actualisez avant de recommencer.");
        }
    } else if input.base_version != 0 {
        bail!("Version initiale invalide.");
    }
    for file in &input.files {
        let n:i64=sqlx::query_scalar("SELECT count(*) FROM entity_files ef JOIN files f ON f.id=ef.file_id WHERE ef.entity_id=? AND ef.file_id=? AND f.organisation_id=?").bind(&id).bind(file).bind(&org).fetch_one(&core.pool).await?;
        if n != 1 {
            bail!("Pièce jointe non autorisée.");
        }
    }
    let version = input.base_version + 1;
    let now = db::now();
    let mut tx = core.pool.begin().await?;
    sqlx::query("INSERT OR IGNORE INTO registers(building_id,opened_on,title,reference) VALUES(?,?, 'Registre de sécurité',?)").bind(&input.building_id).bind(&input.effective_on).bind(format!("REG-{}",input.building_id)).execute(&mut *tx).await?;
    if input.id.is_none() {
        sqlx::query("INSERT INTO entities(id,organisation_id,building_id,kind,created_at,created_by) VALUES(?,?,?,'register_entry',?,?)").bind(&id).bind(&org).bind(&input.building_id).bind(&now).bind(&actor).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO entity_versions VALUES(?,?,?,?,?,?)")
        .bind(&id)
        .bind(version)
        .bind(serde_json::to_string(&input)?)
        .bind(&now)
        .bind(&actor)
        .bind(&input.reason)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO register_entries(id,building_id,block,title,current_version,effective_on,applicability,justification) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET title=excluded.title,current_version=excluded.current_version,effective_on=excluded.effective_on,applicability=excluded.applicability,justification=excluded.justification").bind(&id).bind(&input.building_id).bind(&input.block).bind(input.title.trim()).bind(version).bind(&input.effective_on).bind(&input.applicability).bind(&input.justification).execute(&mut *tx).await?;
    // Les échéances actives du registre alimentent le tableau de bord ; l'historique reste conservé.
    sqlx::query("UPDATE deadlines SET completed_at=? WHERE entity_id=? AND completed_at IS NULL")
        .bind(&now)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    if input.applicability == "applicable" {
        if let Some(due) = input.fields.get("echeance").filter(|s| !s.is_empty()) {
            sqlx::query("INSERT INTO deadlines(id,entity_id,building_id,kind,due_on) VALUES(?,?,?,?,?) ON CONFLICT(entity_id,kind,due_on) DO UPDATE SET completed_at=NULL").bind(db::id()).bind(&id).bind(&input.building_id).bind(format!("Registre {} · {}",input.block,input.title)).bind(due).execute(&mut *tx).await?;
        }
    }
    sync_observation(&mut tx, &input, &id, &org, &actor, &core.key).await?;
    audit::append(
        &mut tx,
        &core.key,
        Some(&actor),
        "register.save",
        json!({"id":id,"version":version,"building":input.building_id}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await?;
    Ok(id)
}
pub async fn history(core: &Core, token: &str, id: &str) -> Result<Value> {
    let building: String =
        sqlx::query_scalar("SELECT building_id FROM register_entries WHERE id=?")
            .bind(id)
            .fetch_one(&core.pool)
            .await?;
    access(core, token, &building, false).await?;
    let rows=sqlx::query("SELECT v.version,v.payload_json,v.recorded_at,u.display_name,v.reason FROM entity_versions v JOIN users u ON u.id=v.recorded_by WHERE v.entity_id=? ORDER BY version DESC").bind(id).fetch_all(&core.pool).await?;
    Ok(json!(rows.into_iter().map(|r|json!({"version":r.get::<i64,_>(0),"payload":serde_json::from_str::<Value>(&r.get::<String,_>(1)).unwrap_or(Value::Null),"recorded_at":r.get::<String,_>(2),"actor":r.get::<String,_>(3),"reason":r.get::<String,_>(4)})).collect::<Vec<_>>()))
}
pub async fn attachment(core: &Core, token: &str, building: &str, file: &str) -> Result<Value> {
    access(core, token, building, false).await?;
    let row=sqlx::query("SELECT f.organisation_id,f.mime_type,f.original_name FROM files f JOIN entity_files ef ON ef.file_id=f.id JOIN register_entries r ON r.id=ef.entity_id WHERE r.building_id=? AND f.id=? LIMIT 1").bind(building).bind(file).fetch_one(&core.pool).await?;
    let bytes = vault::read(core, file, &row.get::<String, _>(0)).await?;
    Ok(
        json!({"bytes":B64.encode(&*bytes),"mime":row.get::<String,_>(1),"name":row.get::<String,_>(2)}),
    )
}
pub async fn attach(core: &Core, token: &str, id: &str, path: &std::path::Path) -> Result<()> {
    crate::shared_workspace::require_write(core)?;
    let current = entry(core, id).await?;
    let building = current["building_id"].as_str().unwrap();
    let actor = access(core, token, building, true).await?;
    if current["archived"] == true {
        bail!("Réactivez l’entrée avant de joindre un document.");
    }
    if std::fs::metadata(path)?.len() > 20 * 1024 * 1024 {
        bail!("Pièce jointe limitée à 20 Mio.");
    }
    if current["files"].as_array().map_or(0, Vec::len) >= 50 {
        bail!("Une entrée est limitée à 50 pièces jointes.");
    }
    let bytes = std::fs::read(path)?;
    let mime = if bytes.starts_with(b"%PDF-") {
        "application/pdf"
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[255, 216, 255]) {
        "image/jpeg"
    } else {
        bail!("Choisissez un PDF, PNG ou JPEG.");
    };
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
        .bind(&actor)
        .fetch_one(&core.pool)
        .await?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("Nom de fichier invalide"))?;
    let file = vault::put(core, &org, &actor, name, mime, &bytes).await?;
    sqlx::query("INSERT INTO entity_files(entity_id,file_id,organisation_id,purpose) VALUES(?,?,?,'register')").bind(id).bind(&file).bind(org).execute(&core.pool).await?;
    let mut files: Vec<String> = serde_json::from_value(current["files"].clone())?;
    files.push(file);
    save(
        core,
        token,
        Input {
            id: Some(id.into()),
            building_id: building.into(),
            block: current["block"].as_str().unwrap().into(),
            title: current["title"].as_str().unwrap().into(),
            effective_on: current["effective_on"].as_str().unwrap().into(),
            applicability: current["applicability"].as_str().unwrap().into(),
            justification: current["justification"].as_str().unwrap_or("").into(),
            fields: serde_json::from_value(current["fields"].clone())?,
            files,
            base_version: current["version"].as_i64().unwrap(),
            reason: format!("Ajout de la pièce jointe {name}"),
        },
    )
    .await?;
    Ok(())
}
pub async fn archive(core: &Core, token: &str, id: &str, archived: bool) -> Result<()> {
    crate::shared_workspace::require_write(core)?;
    let building: String =
        sqlx::query_scalar("SELECT building_id FROM register_entries WHERE id=?")
            .bind(id)
            .fetch_one(&core.pool)
            .await?;
    let actor = access(core, token, &building, true).await?;
    if archived {
        let open:i64=sqlx::query_scalar("SELECT count(*) FROM register_observations r JOIN observations o ON o.id=r.observation_id WHERE r.entry_id=? AND o.status IN('open','in_progress')").bind(id).fetch_one(&core.pool).await?;
        if open > 0 {
            bail!("Une observation est encore ouverte. Consignez sa levée avant d’archiver cette entrée.");
        }
    }
    let mut tx = core.pool.begin().await?;
    sqlx::query("UPDATE entities SET archived_at=? WHERE id=?")
        .bind(if archived { Some(db::now()) } else { None })
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if archived {
        sqlx::query(
            "UPDATE deadlines SET completed_at=? WHERE entity_id=? AND completed_at IS NULL",
        )
        .bind(db::now())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    if !archived {
        let row=sqlx::query("SELECT r.applicability,r.block,r.title,v.payload_json FROM register_entries r JOIN entity_versions v ON v.entity_id=r.id AND v.version=r.current_version WHERE r.id=?").bind(id).fetch_one(&mut *tx).await?;
        let payload: Value = serde_json::from_str(&row.get::<String, _>(3))?;
        if row.get::<String, _>(0) == "applicable" {
            if let Some(due) = payload["fields"]["echeance"]
                .as_str()
                .filter(|s| !s.is_empty())
            {
                sqlx::query("UPDATE deadlines SET completed_at=NULL WHERE entity_id=? AND kind=? AND due_on=?").bind(id).bind(format!("Registre {} · {}",row.get::<String,_>(1),row.get::<String,_>(2))).bind(due).execute(&mut *tx).await?;
            }
        }
    }
    audit::append(
        &mut tx,
        &core.key,
        Some(&actor),
        "register.archive",
        json!({"id":id,"archived":archived}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await
}
pub async fn search(core: &Core, token: &str, query: &str) -> Result<Value> {
    let actor = auth::require(core, token, None).await?;
    if query.chars().count() < 2 || query.len() > 200 {
        return Ok(json!([]));
    }
    let rows=sqlx::query("SELECT r.id,r.building_id,r.block,r.title,b.name,v.payload_json FROM register_entries r JOIN buildings b ON b.id=r.building_id JOIN users u ON u.organisation_id=b.organisation_id JOIN entities e ON e.id=r.id JOIN entity_versions v ON v.entity_id=r.id AND v.version=r.current_version WHERE u.id=? AND e.archived_at IS NULL AND EXISTS(SELECT 1 FROM role_grants g JOIN role_permissions p ON p.role_code=g.role_code WHERE g.user_id=u.id AND g.organisation_id=b.organisation_id AND (g.building_id IS NULL OR g.building_id=b.id) AND p.permission_code='register.read' AND (g.expires_at IS NULL OR g.expires_at>?)) ORDER BY r.effective_on DESC").bind(&actor).bind(db::now()).fetch_all(&core.pool).await?;
    let query = query.to_lowercase();
    let mut out = vec![];
    for r in rows {
        let title: String = r.get(3);
        let payload: String = r.get(5);
        if title.to_lowercase().contains(&query) || payload.to_lowercase().contains(&query) {
            out.push(json!({"id":r.get::<String,_>(0),"building_id":r.get::<String,_>(1),"block":r.get::<String,_>(2),"title":title,"building":r.get::<String,_>(4)}));
            if out.len() >= 50 {
                break;
            }
        }
    }
    Ok(json!(out))
}
async fn sync_observation(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    input: &Input,
    entry: &str,
    org: &str,
    actor: &str,
    key: &[u8],
) -> Result<()> {
    if !["C", "G"].contains(&input.block.as_str()) {
        return Ok(());
    }
    let value = |key: &str| input.fields.get(key).map(String::as_str).unwrap_or("");
    let visit = if input.block == "C"
        && input.applicability == "applicable"
        && value("type_controle") == "commission"
    {
        sqlx::query("INSERT INTO commission_visits(id,building_id,visited_on,commission_name,opinion,next_visit_on) VALUES(?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET visited_on=excluded.visited_on,commission_name=excluded.commission_name,opinion=excluded.opinion,next_visit_on=excluded.next_visit_on").bind(entry).bind(&input.building_id).bind(value("date")).bind(value("organisme")).bind(value("avis")).bind(if value("echeance").is_empty(){None}else{Some(value("echeance"))}).execute(&mut **tx).await?;
        Some(entry)
    } else {
        None
    };
    let previous: Option<String> =
        sqlx::query_scalar("SELECT observation_id FROM register_observations WHERE entry_id=?")
            .bind(entry)
            .fetch_optional(&mut **tx)
            .await?;
    let active =
        input.applicability == "applicable" && ["open", "resolved"].contains(&value("suivi"));
    if active {
        let id = previous.clone().unwrap_or_else(db::id);
        let closed = value("suivi") == "resolved";
        if previous.is_none() {
            sqlx::query("INSERT INTO entities(id,organisation_id,building_id,kind,created_at,created_by) VALUES(?,?,?,'observation',?,?)").bind(&id).bind(org).bind(&input.building_id).bind(db::now()).bind(actor).execute(&mut **tx).await?;
        }
        sqlx::query("INSERT INTO observations(id,building_id,visit_id,reference,description,severity,issued_on,due_on,status,resolved_at,validated_by) VALUES(?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET visit_id=excluded.visit_id,description=excluded.description,severity=excluded.severity,issued_on=excluded.issued_on,due_on=excluded.due_on,status=excluded.status,resolved_at=excluded.resolved_at,validated_by=excluded.validated_by").bind(&id).bind(&input.building_id).bind(visit).bind(format!("REG-{entry}")).bind(value("observations")).bind(value("gravite").parse::<i64>()?).bind(value("date")).bind(if value("echeance").is_empty(){None}else{Some(value("echeance"))}).bind(if closed{"resolved"}else{"open"}).bind(if closed{Some(db::now())}else{None}).bind(if closed{Some(actor)}else{None}).execute(&mut **tx).await?;
        if previous.is_none() {
            sqlx::query("INSERT INTO register_observations VALUES(?,?)")
                .bind(entry)
                .bind(&id)
                .execute(&mut **tx)
                .await?;
        }
        crate::works::sync_observation(tx, key, &id).await?;
    } else if let Some(id) = previous {
        sqlx::query(
            "UPDATE observations SET status='accepted',resolved_at=?,validated_by=? WHERE id=?",
        )
        .bind(db::now())
        .bind(actor)
        .bind(id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
