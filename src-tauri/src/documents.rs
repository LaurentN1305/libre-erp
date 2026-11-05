use crate::{audit, auth, contracts, db, inventory, preferences, vault, works, Core};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sqlx::{Row, SqliteConnection};
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn why(v: &Value) -> Result<&str> {
    let w = s(v, "reason");
    if w.trim().len() < 3 || w.len() > 1000 {
        bail!("Indiquez un commentaire de 3 à 1 000 caractères.")
    };
    Ok(w)
}
async fn allowed(c: &Core, u: &str, b: &str, write: bool, financial: bool) -> Result<bool> {
    Ok(works::permit(
        c,
        u,
        b,
        if write {
            "documents.write"
        } else {
            "documents.read"
        },
    )
    .await?
        && (!financial || works::permit(c, u, b, "documents.financial.read").await?))
}
pub async fn access(c: &Core, t: &str, id: &str, write: bool) -> Result<String> {
    let u = auth::require(c, t, None).await?;
    let r=sqlx::query("SELECT e.building_id,d.restricted,e.archived_at FROM documents d JOIN entities e ON e.id=d.id WHERE d.id=?").bind(id).fetch_one(&c.pool).await?;
    let b: Option<String> = r.get(0);
    let b = b.context("Document sans bâtiment : accès réservé à une migration ultérieure")?;
    if !allowed(c, &u, &b, write, r.get::<i64, _>(1) == 1).await? {
        bail!("Document non accessible dans votre périmètre.")
    };
    if write {
        let active: Option<String> =
            sqlx::query_scalar("SELECT archived_at FROM entities WHERE id=?")
                .bind(&b)
                .fetch_one(&c.pool)
                .await?;
        if active.is_some() {
            bail!("Bâtiment archivé")
        }
    }
    for link in
        sqlx::query_scalar::<_, String>("SELECT entity_id FROM document_links WHERE document_id=?")
            .bind(id)
            .fetch_all(&c.pool)
            .await?
    {
        if !target(c, t, &u, &b, &link).await? {
            bail!("Une entité liée au document n’est plus accessible.")
        }
    }
    Ok(u)
}
async fn target(c: &Core, t: &str, u: &str, b: &str, id: &str) -> Result<bool> {
    let Some(r) = sqlx::query("SELECT kind,building_id,organisation_id FROM entities WHERE id=?")
        .bind(id)
        .fetch_optional(&c.pool)
        .await?
    else {
        return Ok(false);
    };
    if r.get::<String, _>(2) != preferences::org(c, u).await? {
        return Ok(false);
    };
    let kind: String = r.get(0);
    let own = r.get::<Option<String>, _>(1).as_deref() == Some(b) || id == b;
    Ok(match kind.as_str() {
        "building" => id == b && works::permit(c, u, b, "buildings.read").await?,
        "equipment" => own && inventory::access(c, t, id, false).await.is_ok(),
        "work" => own && works::access(c, t, id, false).await.is_ok(),
        "register_entry" => own && crate::register::access(c, t, b, false).await.is_ok(),
        "contract" => {
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM contract_buildings WHERE contract_id=? AND building_id=?",
            )
            .bind(id)
            .bind(b)
            .fetch_one(&c.pool)
            .await?
                > 0
                && contracts::access(c, t, id, false).await.is_ok()
        }
        "provider" => contracts::provider_access(c, u, id, false).await?,
        _ => false,
    })
}
async fn raw(conn: &mut SqliteConnection, id: &str) -> Result<Value> {
    let r=sqlx::query("SELECT d.*,e.building_id,e.archived_at,b.name AS building,f.id AS file_id,f.original_name,f.mime_type,f.byte_size,v.created_at,u.display_name FROM documents d JOIN entities e ON e.id=d.id JOIN buildings b ON b.id=e.building_id JOIN document_versions v ON v.document_id=d.id AND v.version=d.current_version JOIN files f ON f.id=v.file_id JOIN users u ON u.id=v.created_by WHERE d.id=?").bind(id).fetch_one(&mut *conn).await?;
    let links = sqlx::query_scalar::<_, String>(
        "SELECT entity_id FROM document_links WHERE document_id=? ORDER BY entity_id",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(
        json!({"id":id,"building_id":r.get::<String,_>("building_id"),"building":r.get::<String,_>("building"),"category":r.get::<String,_>("category_code"),"document_on":r.get::<Option<String>,_>("document_on"),"deleted":r.get::<Option<String>,_>("deleted_at").is_some(),"title":r.get::<String,_>("title"),"notes":r.get::<String,_>("notes"),"expires_on":r.get::<Option<String>,_>("expires_on"),"retention_until":r.get::<Option<String>,_>("retention_until"),"restricted":r.get::<i64,_>("restricted")==1,"revision":r.get::<i64,_>("revision"),"version":r.get::<i64,_>("current_version"),"file_id":r.get::<String,_>("file_id"),"name":r.get::<String,_>("original_name"),"mime":r.get::<String,_>("mime_type"),"size":r.get::<i64,_>("byte_size"),"date":r.get::<String,_>("created_at"),"actor":r.get::<String,_>("display_name"),"archived":r.get::<Option<String>,_>("archived_at").is_some(),"links":links,"legacy":false}),
    )
}
async fn load(c: &Core, id: &str) -> Result<Value> {
    let mut conn = c.pool.acquire().await?;
    raw(&mut conn, id).await
}
async fn legacy_access(
    c: &Core,
    t: &str,
    entity: &str,
    file: &str,
) -> Result<(String, Option<String>)> {
    let u = auth::require(c, t, None).await?;
    let r=sqlx::query("SELECT e.kind,coalesce(e.building_id,CASE WHEN e.kind='building' THEN e.id END),e.organisation_id FROM entities e JOIN entity_files ef ON ef.entity_id=e.id WHERE e.id=? AND ef.file_id=? LIMIT 1").bind(entity).bind(file).fetch_one(&c.pool).await?;
    let kind: String = r.get(0);
    let b: Option<String> = r.get(1);
    let org: String = r.get(2);
    if org != preferences::org(c, &u).await? {
        bail!("Organisation inaccessible")
    };
    if kind == "provider" {
        if !auth::permissions(&c.pool, &u)
            .await?
            .iter()
            .any(|p| p == "documents.read")
            || !contracts::provider_access(c, &u, entity, false).await?
        {
            bail!("Prestataire inaccessible")
        };
    } else if kind == "contract" {
        contracts::access(c, t, entity, false).await?;
        for b in sqlx::query_scalar::<_, String>(
            "SELECT building_id FROM contract_buildings WHERE contract_id=?",
        )
        .bind(entity)
        .fetch_all(&c.pool)
        .await?
        {
            if !allowed(c, &u, &b, false, true).await? {
                bail!("Pièce contractuelle restreinte")
            }
        }
    } else {
        let b = b.as_deref().context("Pièce sans bâtiment")?;
        let financial = if kind == "work" {
            sqlx::query_scalar::<_,i64>("SELECT count(*) FROM work_documents WHERE work_id=? AND file_id=? AND purpose IN('quote','invoice')").bind(entity).bind(file).fetch_one(&c.pool).await?>0
        } else {
            false
        };
        if !allowed(c, &u, b, false, financial).await? || !target(c, t, &u, b, entity).await? {
            bail!("Pièce non accessible")
        }
    }
    Ok((org, b))
}
pub async fn snapshot(c: &Core,t: &str)->Result<Value>{snapshot_inner(c,t,false).await}
pub(crate) async fn snapshot_export(c: &Core,t: &str)->Result<Value>{snapshot_inner(c,t,true).await}
async fn snapshot_inner(c: &Core,t: &str,full:bool)->Result<Value>{
    let u = auth::require(c, t, None).await?;
    let org = preferences::org(c, &u).await?;
    let mut buildings = vec![];
    let mut targets = vec![];
    let mut records = vec![];
    for r in sqlx::query("SELECT b.id,b.name,e.archived_at FROM buildings b JOIN entities e ON e.id=b.id WHERE b.organisation_id=? ORDER BY b.name").bind(&org).fetch_all(&c.pool).await?{let b:String=r.get(0);if !allowed(c,&u,&b,false,false).await?{continue}buildings.push(json!({"id":b,"name":r.get::<String,_>(1),"archived":r.get::<Option<String>,_>(2).is_some(),"can_write":allowed(c,&u,&b,true,false).await?&&r.get::<Option<String>,_>(2).is_none(),"financial":allowed(c,&u,&b,false,true).await?}));for row in sqlx::query("SELECT e.id,e.kind,coalesce(q.designation,w.title,r.title,co.subject,p.name,b.name,'Fiche liée') AS title FROM entities e LEFT JOIN equipment q ON q.id=e.id LEFT JOIN works w ON w.id=e.id LEFT JOIN register_entries r ON r.id=e.id LEFT JOIN contracts co ON co.id=e.id LEFT JOIN providers p ON p.id=e.id LEFT JOIN buildings b ON b.id=e.id WHERE (e.building_id=? OR e.id=? OR e.id IN(SELECT contract_id FROM contract_buildings WHERE building_id=?) OR e.kind='provider') AND e.organisation_id=? AND e.archived_at IS NULL AND e.kind IN('building','equipment','work','register_entry','contract','provider')").bind(&b).bind(&b).bind(&b).bind(&org).fetch_all(&c.pool).await?{let id:String=row.get(0);if target(c,t,&u,&b,&id).await?{targets.push(json!({"id":id,"building_id":b,"kind":row.get::<String,_>(1),"name":row.get::<String,_>(2)}))}}}
    for id in sqlx::query_scalar::<_,String>(if full {"SELECT d.id FROM documents d JOIN entities e ON e.id=d.id WHERE e.organisation_id=? ORDER BY d.title"} else {"SELECT d.id FROM documents d JOIN entities e ON e.id=d.id WHERE e.organisation_id=? ORDER BY d.title LIMIT 5000"}).bind(&org).fetch_all(&c.pool).await?{if access(c,t,&id,false).await.is_ok(){let mut v=load(c,&id).await?;v["can_write"]=json!(access(c,t,&id,true).await.is_ok());records.push(v)}}
    for r in sqlx::query(if full {"SELECT DISTINCT e.id,e.kind,coalesce(e.building_id,CASE WHEN e.kind='building' THEN e.id END) AS building_id,b.name AS building,f.id AS fid,f.original_name,f.mime_type,f.byte_size,f.created_at FROM entity_files ef JOIN entities e ON e.id=ef.entity_id JOIN files f ON f.id=ef.file_id LEFT JOIN buildings b ON b.id=coalesce(e.building_id,CASE WHEN e.kind='building' THEN e.id END) WHERE e.organisation_id=? AND e.kind<>'document' ORDER BY f.created_at DESC"} else {"SELECT DISTINCT e.id,e.kind,coalesce(e.building_id,CASE WHEN e.kind='building' THEN e.id END) AS building_id,b.name AS building,f.id AS fid,f.original_name,f.mime_type,f.byte_size,f.created_at FROM entity_files ef JOIN entities e ON e.id=ef.entity_id JOIN files f ON f.id=ef.file_id LEFT JOIN buildings b ON b.id=coalesce(e.building_id,CASE WHEN e.kind='building' THEN e.id END) WHERE e.organisation_id=? AND e.kind<>'document' ORDER BY f.created_at DESC LIMIT 5000"}).bind(&org).fetch_all(&c.pool).await?{let id:String=r.get("id");let file:String=r.get("fid");if legacy_access(c,t,&id,&file).await.is_ok(){let mut associated=vec![];for bid in sqlx::query_scalar::<_,String>("SELECT building_id FROM contract_buildings WHERE contract_id=? OR contract_id IN(SELECT id FROM contracts WHERE provider_id=?)").bind(&id).bind(&id).fetch_all(&c.pool).await?{if allowed(c,&u,&bid,false,false).await?{associated.push(bid)}}records.push(json!({"id":format!("legacy:{id}:{file}"),"source_id":id,"source_kind":r.get::<String,_>("kind"),"building_ids":associated,"building_id":r.get::<Option<String>,_>("building_id"),"building":r.get::<Option<String>,_>("building"),"title":r.get::<String,_>("original_name"),"name":r.get::<String,_>("original_name"),"mime":r.get::<String,_>("mime_type"),"size":r.get::<i64,_>("byte_size"),"date":r.get::<String,_>("created_at"),"file_id":file,"category":"other","expires_on":null,"archived":false,"legacy":true,"can_write":false}))}}
    let categories = sqlx::query("SELECT code,label FROM document_categories ORDER BY label")
        .fetch_all(&c.pool)
        .await?
        .iter()
        .map(|r| json!({"code":r.get::<String,_>(0),"label":r.get::<String,_>(1)}))
        .collect::<Vec<_>>();
    Ok(
        json!({"records":records,"buildings":buildings,"targets":targets,"categories":categories,"today":works::today()}),
    )
}
async fn revision(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    key: &[u8],
    id: &str,
    u: &str,
    reason: &str,
) -> Result<()> {
    sqlx::query("UPDATE documents SET revision=revision+1 WHERE id=?")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    let v = raw(&mut **tx, id).await?;
    sqlx::query("INSERT INTO entity_versions(entity_id,version,payload_json,recorded_at,recorded_by,reason) VALUES(?,?,?,?,?,?)").bind(id).bind(v["revision"].as_i64().unwrap()).bind(v.to_string()).bind(db::now()).bind(u).bind(reason).execute(&mut **tx).await?;
    audit::append(
        tx,
        key,
        Some(u),
        "document.changed",
        json!({"id":id,"revision":v["revision"],"version":v["version"],"reason":reason}),
    )
    .await
}
pub async fn save(c: &Core, t: &str, v: Value, path: Option<&std::path::Path>) -> Result<String> {
    crate::shared_workspace::require_write(c)?;
    let u = auth::require(c, t, None).await?;
    let reason = why(&v)?;
    let b = s(&v, "building_id");
    let old = v["id"].as_str();
    let id = old.map(str::to_owned).unwrap_or_else(db::id);
    let restricted = v["restricted"] == true || ["quote", "invoice"].contains(&s(&v, "category"));
    if !allowed(c, &u, b, true, restricted).await? {
        bail!("Modification non autorisée")
    };
    if sqlx::query_scalar::<_, Option<String>>("SELECT archived_at FROM entities WHERE id=?")
        .bind(b)
        .fetch_one(&c.pool)
        .await?
        .is_some()
    {
        bail!("Bâtiment archivé")
    };
    if s(&v, "title").trim().is_empty()
        || s(&v, "title").len() > 250
        || s(&v, "notes").len() > 20000
    {
        bail!("Titre ou notes invalides")
    };
    for field in ["document_on", "expires_on", "retention_until"] {
        if let Some(d) = v[field].as_str().filter(|s| !s.is_empty()) {
            let date = chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")?;
            if date.to_string() != d {
                bail!("Date invalide")
            }
        }
    }
    if sqlx::query_scalar::<_, i64>("SELECT count(*) FROM document_categories WHERE code=?")
        .bind(s(&v, "category"))
        .fetch_one(&c.pool)
        .await?
        != 1
    {
        bail!("Catégorie inconnue")
    };
    let mut links = vec![b.to_string()];
    for link in v["links"].as_array().context("Liens manquants")? {
        let link = link.as_str().context("Lien invalide")?;
        if !target(c, t, &u, b, link).await? {
            bail!("Lien non accessible ou d’un autre bâtiment")
        };
        if !links.iter().any(|l| l == link) {
            links.push(link.to_string())
        }
    }
    if links.len() > 100 {
        bail!("100 liens maximum")
    };
    let mut version = 1;
    let mut sticky = restricted;
    if old.is_some() {
        access(c, t, &id, true).await?;
        let current = load(c, &id).await?;
        if current["building_id"] != b
            || current["revision"] != v["base_revision"]
            || current["archived"] == true
            || current["deleted"] == true
        {
            bail!("La fiche a changé, est archivée ou le bâtiment est différent. Actualisez.")
        };
        sticky |= current["restricted"] == true;
        version = current["version"].as_i64().unwrap() + i64::from(path.is_some());
    }
    let file = if let Some(p) = path {
        if std::fs::metadata(p)?.len() > 50 * 1024 * 1024 {
            bail!("Document limité à 50 Mio")
        };
        let ext = p
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if ![
            "pdf", "png", "jpg", "jpeg", "doc", "docx", "xls", "xlsx", "odt", "ods", "dwg", "dxf",
            "txt", "csv", "zip",
        ]
        .contains(&ext.as_str())
        {
            bail!("Format non pris en charge. Choisissez PDF, image, document Office/OpenDocument, plan DWG/DXF, texte ou ZIP.")
        };
        let bytes = std::fs::read(p)?;
        if bytes.is_empty() {
            bail!("Fichier vide")
        };
        let mime = match ext.as_str() {
            "pdf" => "application/pdf",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            _ => "application/octet-stream",
        };
        Some(
            vault::put(
                c,
                &preferences::org(c, &u).await?,
                &u,
                p.file_name()
                    .and_then(|s| s.to_str())
                    .context("Nom invalide")?,
                mime,
                &bytes,
            )
            .await?,
        )
    } else {
        if old.is_none() {
            bail!("Choisissez le premier fichier")
        };
        None
    };
    let org = preferences::org(c, &u).await?;
    let mut tx = c.pool.begin().await?;
    if old.is_none() {
        sqlx::query("INSERT INTO entities(id,organisation_id,building_id,kind,created_at,created_by) VALUES(?,?,?,'document',?,?)").bind(&id).bind(&org).bind(b).bind(db::now()).bind(&u).execute(&mut *tx).await?;
        sqlx::query(
            "INSERT INTO documents(id,category_code,title,current_version) VALUES(?,?,?,1)",
        )
        .bind(&id)
        .bind(s(&v, "category"))
        .bind(s(&v, "title"))
        .execute(&mut *tx)
        .await?;
    }
    if let Some(f) = file {
        sqlx::query("INSERT INTO document_versions VALUES(?,?,?,?,?,?)")
            .bind(&id)
            .bind(version)
            .bind(f)
            .bind(db::now())
            .bind(&u)
            .bind(reason)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE documents SET title=?,category_code=?,current_version=?,expires_on=?,retention_until=?,notes=?,restricted=?,document_on=? WHERE id=?").bind(s(&v,"title").trim()).bind(s(&v,"category")).bind(version).bind(v["expires_on"].as_str().filter(|s|!s.is_empty())).bind(v["retention_until"].as_str().filter(|s|!s.is_empty())).bind(s(&v,"notes")).bind(i64::from(sticky)).bind(v["document_on"].as_str().filter(|s|!s.is_empty())).bind(&id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM document_links WHERE document_id=?")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    for link in links {
        sqlx::query("INSERT INTO document_links VALUES(?,?,'reference')")
            .bind(&id)
            .bind(link)
            .execute(&mut *tx)
            .await?;
    }
    deadline(&mut tx, &id).await?;
    revision(&mut tx, &c.key, &id, &u, reason).await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await?;
    Ok(id)
}
async fn deadline(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>, id: &str) -> Result<()> {
    sqlx::query("UPDATE deadlines SET completed_at=? WHERE entity_id=? AND kind LIKE 'document:%' AND completed_at IS NULL").bind(db::now()).bind(id).execute(&mut **tx).await?;
    let r=sqlx::query("SELECT d.title,d.expires_on,e.building_id,coalesce(e.archived_at,d.deleted_at) FROM documents d JOIN entities e ON e.id=d.id WHERE d.id=?").bind(id).fetch_one(&mut **tx).await?;
    if r.get::<Option<String>, _>(3).is_none() {
        if let Some(day) = r.get::<Option<String>, _>(1) {
            sqlx::query("INSERT INTO deadlines(id,entity_id,building_id,kind,due_on) VALUES(?,?,?,?,?) ON CONFLICT(entity_id,kind,due_on) DO UPDATE SET completed_at=NULL").bind(db::id()).bind(id).bind(r.get::<String,_>(2)).bind(format!("document:Expiration · {}",r.get::<String,_>(0))).bind(day).execute(&mut **tx).await?;
        }
    }
    Ok(())
}
pub async fn trash(c: &Core, t: &str, id: &str, base_revision: i64, deleted: bool) -> Result<()> {
    let u = access(c, t, id, true).await?;
    let mut tx = c.pool.begin().await?;
    let state = raw(&mut *tx, id).await?;
    if state["revision"] != base_revision || state["deleted"] == deleted {
        bail!("La fiche a changé. Actualisez.")
    }
    crate::shared_workspace::require_write(c)?;
    sqlx::query("UPDATE documents SET deleted_at=? WHERE id=?")
        .bind(deleted.then(db::now))
        .bind(id)
        .execute(&mut *tx)
        .await?;
    deadline(&mut tx, id).await?;
    let reason = if deleted { "Mis à la corbeille" } else { "Restauré depuis la corbeille" };
    revision(&mut tx, &c.key, id, &u, reason).await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}
pub async fn category_save(c: &Core, t: &str, code: Option<&str>, label: &str) -> Result<()> {
    let u = auth::require(c, t, Some("users.manage")).await?;
    let label = label.trim();
    if label.is_empty() || label.chars().count() > 100 {
        bail!("Le nom de la catégorie doit contenir de 1 à 100 caractères.")
    }
    let mut tx = c.pool.begin().await?;
    let labels: Vec<(String, String)> =
        sqlx::query_as("SELECT code,label FROM document_categories").fetch_all(&mut *tx).await?;
    if labels.iter().any(|(key, existing)| Some(key.as_str()) != code && existing.to_lowercase() == label.to_lowercase()) {
        bail!("Une catégorie porte déjà ce nom.")
    }
    if code.is_some_and(|code| !labels.iter().any(|(key, _)| key == code)) {
        bail!("Catégorie inconnue.")
    }
    crate::shared_workspace::require_write(c)?;
    let code = code.map(str::to_owned).unwrap_or_else(db::id);
    sqlx::query("INSERT INTO document_categories(code,label) VALUES(?,?) ON CONFLICT(code) DO UPDATE SET label=excluded.label")
        .bind(&code).bind(label).execute(&mut *tx).await?;
    audit::append(&mut tx, &c.key, Some(&u), "document.category_saved", json!({"code":code,"label":label})).await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}
pub async fn archive(
    c: &Core,
    t: &str,
    id: &str,
    revision_number: i64,
    archived: bool,
    reason: &str,
) -> Result<()> {
    crate::shared_workspace::require_write(c)?;
    let u = access(c, t, id, true).await?;
    why(&json!({"reason":reason}))?;
    let v = load(c, id).await?;
    if v["revision"] != revision_number || v["deleted"] == true {
        bail!("La fiche a changé. Actualisez")
    };
    let mut tx = c.pool.begin().await?;
    sqlx::query("UPDATE entities SET archived_at=? WHERE id=?")
        .bind(if archived { Some(db::now()) } else { None })
        .bind(id)
        .execute(&mut *tx)
        .await?;
    deadline(&mut tx, id).await?;
    revision(&mut tx, &c.key, id, &u, reason).await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}
pub async fn history(c: &Core, t: &str, id: &str) -> Result<Value> {
    access(c, t, id, false).await?;
    let mut versions = vec![];
    for r in sqlx::query("SELECT v.*,f.original_name,f.byte_size,u.display_name FROM document_versions v JOIN files f ON f.id=v.file_id JOIN users u ON u.id=v.created_by WHERE document_id=? ORDER BY version DESC LIMIT 100").bind(id).fetch_all(&c.pool).await?{versions.push(json!({"version":r.get::<i64,_>("version"),"file_id":r.get::<String,_>("file_id"),"name":r.get::<String,_>("original_name"),"size":r.get::<i64,_>("byte_size"),"date":r.get::<String,_>("created_at"),"actor":r.get::<String,_>("display_name"),"reason":r.get::<String,_>("change_note")}))}
    let mut edits = vec![];
    for r in sqlx::query("SELECT v.*,u.display_name FROM entity_versions v JOIN users u ON u.id=v.recorded_by WHERE entity_id=? ORDER BY version DESC LIMIT 100").bind(id).fetch_all(&c.pool).await?{edits.push(json!({"revision":r.get::<i64,_>("version"),"date":r.get::<String,_>("recorded_at"),"actor":r.get::<String,_>("display_name"),"reason":r.get::<String,_>("reason"),"payload":serde_json::from_str::<Value>(&r.get::<String,_>("payload_json"))?}))}
    Ok(json!({"versions":versions,"edits":edits}))
}
pub async fn file(
    c: &Core,
    t: &str,
    id: &str,
    f: &str,
    source: Option<&str>,
) -> Result<(String, Vec<u8>)> {
    let org = if let Some(source) = source {
        legacy_access(c, t, source, f).await?.0
    } else {
        let u = access(c, t, id, false).await?;
        if sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM document_versions WHERE document_id=? AND file_id=?",
        )
        .bind(id)
        .bind(f)
        .fetch_one(&c.pool)
        .await?
            != 1
        {
            bail!("Version non liée à ce document")
        };
        preferences::org(c, &u).await?
    };
    let name: String =
        sqlx::query_scalar("SELECT original_name FROM files WHERE id=? AND organisation_id=?")
            .bind(f)
            .bind(&org)
            .fetch_one(&c.pool)
            .await?;
    Ok((name, vault::read(c, f, &org).await?.to_vec()))
}
