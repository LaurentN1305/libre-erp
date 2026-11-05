use crate::{audit, auth, db, Core};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use sqlx::Row;

pub async fn set_scope(core: &Core, token: &str, user: &str, buildings: Vec<String>) -> Result<()> {
    crate::shared_workspace::require_write(core)?;
    let actor = auth::require(core, token, Some("users.manage")).await?;
    if actor == user {
        bail!("Votre propre périmètre doit rester global.");
    }
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
        .bind(&actor)
        .fetch_one(&core.pool)
        .await?;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE id=? AND organisation_id=?")
        .bind(user)
        .bind(&org)
        .fetch_one(&core.pool)
        .await?;
    if n != 1 || buildings.len() > 10000 {
        bail!("Périmètre invalide.");
    }
    for id in &buildings {
        if !allowed(core, &actor, id, "buildings.read").await? {
            bail!("Bâtiment non accessible.");
        }
    }
    let mut tx = core.pool.begin().await?;
    let roles=sqlx::query("SELECT role_code,CASE WHEN sum(expires_at IS NULL)>0 THEN NULL ELSE max(expires_at) END AS expiry FROM role_grants WHERE user_id=? AND (expires_at IS NULL OR expires_at>?) GROUP BY role_code").bind(user).bind(db::now()).fetch_all(&mut *tx).await?;
    sqlx::query("DELETE FROM role_grants WHERE user_id=?")
        .bind(user)
        .execute(&mut *tx)
        .await?;
    for grant in roles {
        let role: String = grant.get(0);
        let expiry: Option<String> = grant.get(1);
        if buildings.is_empty() {
            sqlx::query(
                "INSERT INTO role_grants(id,user_id,organisation_id,role_code,expires_at) VALUES(?,?,?,?,?)",
            )
            .bind(db::id())
            .bind(user)
            .bind(&org)
            .bind(&role)
            .bind(&expiry)
            .execute(&mut *tx)
            .await?;
        } else {
            for id in &buildings {
                sqlx::query("INSERT INTO role_grants(id,user_id,organisation_id,role_code,building_id,expires_at) VALUES(?,?,?,?,?,?)").bind(db::id()).bind(user).bind(&org).bind(&role).bind(id).bind(&expiry).execute(&mut *tx).await?;
            }
        }
    }
    sqlx::query("UPDATE sessions SET revoked_at=? WHERE user_id=?")
        .bind(db::now())
        .bind(user)
        .execute(&mut *tx)
        .await?;
    audit::append(
        &mut tx,
        &core.key,
        Some(&actor),
        "user.building_scope",
        serde_json::json!({"user_id":user,"buildings":buildings}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await
}

#[derive(Deserialize, Serialize)]
pub struct BuildingInput {
    #[serde(default)]
    pub location: Option<crate::maps::Location>,
    pub id: Option<String>,
    pub name: String,
    pub address: String,
    pub category: i64,
    pub capacity: i64,
    pub types: Vec<String>,
    pub manager: Option<String>,
    #[serde(default)]
    pub opened_on: Option<String>,
}

pub async fn allowed(core: &Core, user: &str, building: &str, permission: &str) -> Result<bool> {
    if !core.sharing.can_write() && (permission.ends_with(".write") || permission=="work.validate") {return Ok(false);}
    let n:i64=sqlx::query_scalar("SELECT count(*) FROM role_grants g JOIN role_permissions p ON p.role_code=g.role_code JOIN users u ON u.id=g.user_id JOIN buildings b ON b.organisation_id=u.organisation_id WHERE g.user_id=? AND b.id=? AND g.organisation_id=u.organisation_id AND (g.building_id IS NULL OR g.building_id=b.id) AND p.permission_code=? AND (g.expires_at IS NULL OR g.expires_at>?)")
        .bind(user).bind(building).bind(permission).bind(db::now()).fetch_one(&core.pool).await?;
    Ok(n > 0)
}
async fn check(core: &Core, token: &str, building: &str, permission: &str) -> Result<String> {
    let actor = auth::require(core, token, None).await?;
    if !allowed(core, &actor, building, permission).await? {
        bail!("Ce bâtiment n’est pas accessible avec votre rôle.");
    }
    Ok(actor)
}
pub async fn snapshot(core: &Core, token: &str) -> Result<serde_json::Value> {
    let actor = auth::require(core, token, None).await?;
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
        .bind(&actor)
        .fetch_one(&core.pool)
        .await?;
    let rows=sqlx::query("SELECT b.*,e.archived_at,u.display_name AS manager_name FROM buildings b JOIN entities e ON e.id=b.id LEFT JOIN users u ON u.id=b.safety_manager_id WHERE b.organisation_id=? ORDER BY b.name COLLATE NOCASE").bind(&org).fetch_all(&core.pool).await?;
    let mut buildings = vec![];
    for r in rows {
        let id: String = r.get("id");
        if !allowed(core, &actor, &id, "buildings.read").await? {
            continue;
        }
        let mut item = serde_json::json!({"id":id,"name":r.get::<String,_>("name"),"address":r.get::<String,_>("address"),"category":r.get::<i64,_>("erp_category"),"capacity":r.get::<i64,_>("capacity"),"opened_on":r.get::<Option<String>,_>("opened_on"),"manager":r.get::<Option<String>,_>("safety_manager_id"),"manager_name":r.get::<Option<String>,_>("manager_name"),"photo":r.get::<Option<String>,_>("photo_file_id").is_some(),"archived":r.get::<Option<String>,_>("archived_at").is_some(),"editable":allowed(core,&actor,&id,"buildings.write").await?});
        let types:Vec<String>=sqlx::query_scalar("SELECT type_code FROM building_erp_types WHERE building_id=? ORDER BY is_primary DESC,type_code").bind(&id).fetch_all(&core.pool).await?;
        item["location"] = r
            .get::<Option<String>, _>("location_json")
            .map(|s| serde_json::from_str::<serde_json::Value>(&s))
            .transpose()?
            .unwrap_or(serde_json::Value::Null);
        item["can_add_work"] = serde_json::json!(
            r.get::<Option<String>, _>("archived_at").is_none()
                && allowed(core, &actor, &id, "work.write").await?
        );
        item["types"] = serde_json::json!(types);
        item["alerts_read"] = serde_json::json!(allowed(core, &actor, &id, "alerts.read").await?);
        item["documents_read"] =
            serde_json::json!(allowed(core, &actor, &id, "documents.read").await?);
        item["can_add_document"] = serde_json::json!(
            item["documents_read"].as_bool() == Some(true)
                && r.get::<Option<String>, _>("archived_at").is_none()
                && allowed(core, &actor, &id, "documents.write").await?
        );
        item["contracts_read"] =
            serde_json::json!(allowed(core, &actor, &id, "contracts.read").await?);
        item["inventory_read"] =
            serde_json::json!(allowed(core, &actor, &id, "inventory.read").await?);
        item["register_read"] =
            serde_json::json!(allowed(core, &actor, &id, "register.read").await?);
        let c=sqlx::query("SELECT count(*) AS total,coalesce(sum(status='compliant'),0) AS compliant,coalesce(sum(status='non_compliant'),0) AS non_compliant,coalesce(sum(status='unknown'),0) AS unknown FROM compliance_requirements WHERE building_id=? AND applicable=1 AND status<>'not_applicable'").bind(&id).fetch_one(&core.pool).await?;
        item["compliance"] = serde_json::json!({"total":c.get::<i64,_>("total"),"compliant":c.get::<i64,_>("compliant"),"non_compliant":c.get::<i64,_>("non_compliant"),"unknown":c.get::<i64,_>("unknown")});
        let work_read = allowed(core, &actor, &id, "work.budget.read").await?;
        if work_read {
            let b=sqlx::query("SELECT count(*) AS count,coalesce(sum(v.estimated_budget_cents),0) AS estimated,coalesce(sum(v.actual_budget_cents),0) AS actual FROM works w JOIN work_budgets v ON v.id=w.id JOIN entities e ON e.id=w.id WHERE w.building_id=? AND e.archived_at IS NULL AND w.status<>'archived'").bind(&id).fetch_one(&core.pool).await?;
            item["budget"] = serde_json::json!({"count":b.get::<i64,_>("count"),"estimated":b.get::<i64,_>("estimated"),"actual":b.get::<i64,_>("actual")});
        }
        let alerts_read = allowed(core, &actor, &id, "alerts.read").await?;
        if alerts_read {
            let candidates=sqlx::query_scalar::<_,String>("SELECT a.deadline_id FROM alerts a JOIN deadlines d ON d.id=a.deadline_id JOIN alert_rules r ON r.id=a.rule_id WHERE d.building_id=? AND a.resolved_at IS NULL AND (CASE WHEN a.managed=1 THEN a.severity ELSE r.severity END)=4").bind(&id).fetch_all(&core.pool).await?;
            let mut critical = 0;
            for d in candidates {
                if crate::alerts::visible(core, &actor, &d, "alerts.read").await? {
                    critical += 1
                }
            }
            item["critical"] = serde_json::json!(critical);
            let ds=sqlx::query("SELECT d.id,d.kind,d.due_on,e.kind,d.entity_id FROM deadlines d JOIN entities e ON e.id=d.entity_id WHERE d.building_id=? AND e.organisation_id=? AND e.archived_at IS NULL AND d.completed_at IS NULL AND date(d.due_on)<=date('now','+30 days') ORDER BY d.due_on,d.id").bind(&id).bind(&org).fetch_all(&core.pool).await?;
            let mut visible = vec![];
            for d in ds {
                let entity: String = d.get(4);
                let is_manual = sqlx::query_scalar::<_, i64>(
                    "SELECT count(*) FROM manual_deadlines WHERE deadline_id=?",
                )
                .bind(d.get::<String, _>(0))
                .fetch_one(&core.pool)
                .await?
                    > 0;
                if is_manual
                    && !crate::alerts::visible(core, &actor, &d.get::<String, _>(0), "alerts.read")
                        .await?
                {
                    continue;
                }
                let ok = match d.get::<String, _>(3).as_str() {
                    "document" => crate::documents::access(core, token, &entity, false)
                        .await
                        .is_ok(),
                    "contract" => crate::contracts::access(core, token, &entity, false)
                        .await
                        .is_ok(),
                    _ => true,
                };
                if ok {
                    visible.push(serde_json::json!({"id":d.get::<String,_>(0),"kind":d.get::<String,_>(1),"due_on":d.get::<String,_>(2)}))
                }
            }
            item["deadlines"] = serde_json::json!(visible);
        }
        buildings.push(item);
    }
    let types = sqlx::query("SELECT code,label FROM erp_types ORDER BY code")
        .fetch_all(&core.pool)
        .await?
        .into_iter()
        .map(|r| serde_json::json!({"code":r.get::<String,_>(0),"label":r.get::<String,_>(1)}))
        .collect::<Vec<_>>();
    let create = auth::permissions(&core.pool, &actor)
        .await?
        .contains(&"buildings.write".into());
    let managers = if create || buildings.iter().any(|b| b["editable"] == true) {
        sqlx::query("SELECT id,display_name FROM users WHERE organisation_id=? AND active=1 ORDER BY display_name").bind(org).fetch_all(&core.pool).await?.into_iter().map(|r|serde_json::json!({"id":r.get::<String,_>(0),"name":r.get::<String,_>(1)})).collect::<Vec<_>>()
    } else {
        vec![]
    };
    Ok(
        serde_json::json!({"buildings":buildings,"types":types,"managers":managers,"can_create":create}),
    )
}
pub async fn save(core: &Core, token: &str, input: BuildingInput) -> Result<String> {
    crate::shared_workspace::require_write(core)?;
    let actor = if let Some(id) = &input.id {
        check(core, token, id, "buildings.write").await?
    } else {
        auth::require(core, token, Some("buildings.write")).await?
    };
    if input.name.trim().is_empty()
        || input.name.len() > 200
        || input.address.trim().is_empty()
        || input.address.len() > 1000
        || !(1..=5).contains(&input.category)
        || !(0..=1_000_000).contains(&input.capacity)
        || input.types.is_empty()
        || input.types.len() > 22
    {
        bail!("Vérifiez le nom, l’adresse, la catégorie, la capacité et les types ERP.");
    }
    crate::maps::validate(&input.location)?;
    if let Some(date) = &input.opened_on {
        let parsed = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map_err(|_| anyhow::anyhow!("Date d’ouverture invalide."))?;
        if parsed.format("%Y-%m-%d").to_string() != *date {
            bail!("Date d’ouverture invalide.");
        }
    }
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
        .bind(&actor)
        .fetch_one(&core.pool)
        .await?;
    if let Some(manager) = &input.manager {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM users WHERE id=? AND organisation_id=? AND active=1",
        )
        .bind(manager)
        .bind(&org)
        .fetch_one(&core.pool)
        .await?;
        if n != 1 {
            bail!("Responsable non disponible.");
        }
    }
    let id = input.id.clone().unwrap_or_else(db::id);
    let mut tx = core.pool.begin().await?;
    if input.id.is_none() {
        sqlx::query("INSERT INTO entities(id,organisation_id,kind,created_at,created_by) VALUES(?,?,'building',?,?)").bind(&id).bind(&org).bind(db::now()).bind(&actor).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO buildings(id,organisation_id,name,address,erp_category,capacity,safety_manager_id,opened_on,location_json) VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,address=excluded.address,erp_category=excluded.erp_category,capacity=excluded.capacity,safety_manager_id=excluded.safety_manager_id,opened_on=excluded.opened_on,location_json=excluded.location_json").bind(&id).bind(&org).bind(input.name.trim()).bind(input.address.trim()).bind(input.category).bind(input.capacity).bind(&input.manager).bind(&input.opened_on).bind(input.location.as_ref().map(serde_json::to_string).transpose()?).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM building_erp_types WHERE building_id=?")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    for (i, t) in input.types.iter().enumerate() {
        sqlx::query("INSERT INTO building_erp_types VALUES(?,?,?)")
            .bind(&id)
            .bind(t)
            .bind(i == 0)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("INSERT INTO entity_versions SELECT ?,coalesce(max(version),0)+1,?,?,?,'Fiche bâtiment' FROM entity_versions WHERE entity_id=?").bind(&id).bind(serde_json::to_string(&input)?).bind(db::now()).bind(&actor).bind(&id).execute(&mut *tx).await?;
    audit::append(
        &mut tx,
        &core.key,
        Some(&actor),
        "building.save",
        serde_json::json!({"building_id":id}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await?;
    Ok(id)
}
pub async fn archive(core: &Core, token: &str, id: &str, archived: bool) -> Result<()> {
    crate::shared_workspace::require_write(core)?;
    let actor = check(core, token, id, "buildings.write").await?;
    let mut tx = core.pool.begin().await?;
    sqlx::query("UPDATE entities SET archived_at=? WHERE id=?")
        .bind(if archived { Some(db::now()) } else { None })
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::append(
        &mut tx,
        &core.key,
        Some(&actor),
        "building.archive",
        serde_json::json!({"building_id":id,"archived":archived}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await
}
pub async fn photo(core: &Core, token: &str, id: &str) -> Result<Option<String>> {
    check(core, token, id, "buildings.read").await?;
    let row = sqlx::query("SELECT organisation_id,photo_file_id FROM buildings WHERE id=?")
        .bind(id)
        .fetch_one(&core.pool)
        .await?;
    let file: Option<String> = row.get(1);
    if let Some(file) = file {
        use base64::Engine;
        let bytes = crate::vault::read(core, &file, &row.get::<String, _>(0)).await?;
        let mime: String = sqlx::query_scalar("SELECT mime_type FROM files WHERE id=?")
            .bind(file)
            .fetch_one(&core.pool)
            .await?;
        Ok(Some(format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&*bytes)
        )))
    } else {
        Ok(None)
    }
}
pub async fn set_photo(
    core: &Core,
    token: &str,
    id: &str,
    path: Option<std::path::PathBuf>,
) -> Result<()> {
    crate::shared_workspace::require_write(core)?;
    let actor = check(core, token, id, "buildings.write").await?;
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM buildings WHERE id=?")
        .bind(id)
        .fetch_one(&core.pool)
        .await?;
    let file = if let Some(path) = path {
        if std::fs::metadata(&path)?.len() > 8 * 1024 * 1024 {
            bail!("Photo limitée à 8 Mio.");
        }
        let bytes = std::fs::read(&path)?;
        let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if bytes.starts_with(&[255, 216, 255]) {
            "image/jpeg"
        } else {
            bail!("Choisissez une photo PNG ou JPEG.");
        };
        Some(crate::vault::put(core, &org, &actor, "photo-batiment", mime, &bytes).await?)
    } else {
        None
    };
    let mut tx = core.pool.begin().await?;
    sqlx::query("UPDATE buildings SET photo_file_id=? WHERE id=?")
        .bind(&file)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::append(
        &mut tx,
        &core.key,
        Some(&actor),
        "building.photo",
        serde_json::json!({"building_id":id,"file_id":file}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await
}
