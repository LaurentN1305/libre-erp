use crate::{audit, auth, crypto, db, vault, Core};
use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use std::collections::HashSet;
type Tx<'a> = sqlx::Transaction<'a, sqlx::Sqlite>;
#[derive(Deserialize, Serialize, Clone)]
pub struct Participant {
    pub user_id: Option<String>,
    pub provider_id: Option<String>,
    pub role: String,
}
#[derive(Deserialize, Serialize, Clone)]
pub struct Contact {
    pub name: String,
    pub role: String,
    pub contact: String,
}
#[derive(Deserialize, Serialize, Clone)]
pub struct Input {
    pub id: Option<String>,
    pub building_id: String,
    pub title: String,
    pub description: String,
    pub kind: String,
    pub origin: String,
    pub observation_id: Option<String>,
    pub priority: i64,
    pub planned_start: Option<String>,
    pub planned_end: Option<String>,
    pub actual_start: Option<String>,
    pub actual_end: Option<String>,
    pub target_due_on: Option<String>,
    pub estimated_budget_cents: i64,
    pub progress_percent: i64,
    pub project_manager: String,
    pub architect: String,
    pub commission_authorisation: String,
    pub safety_impact: String,
    pub compensatory_measures: String,
    pub equipment_ids: Vec<String>,
    pub predecessors: Vec<String>,
    pub participants: Vec<Participant>,
    pub contacts: Vec<Contact>,
    pub base_version: i64,
    pub reason: String,
}
pub fn today() -> String {
    chrono::Utc::now()
        .with_timezone(&chrono_tz::Europe::Paris)
        .date_naive()
        .to_string()
}
fn date(s: &Option<String>) -> Result<()> {
    if let Some(s) = s {
        let d = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")?;
        if d.to_string() != *s {
            bail!("Date invalide : utilisez le calendrier.");
        }
    }
    Ok(())
}
fn reason(s: &str) -> Result<()> {
    if s.trim().is_empty() || s.len() > 1000 {
        bail!("Indiquez un motif (1 à 1 000 caractères).");
    }
    Ok(())
}
pub async fn permit(core: &Core, user: &str, building: &str, permission: &str) -> Result<bool> {
    if !core.sharing.can_write() && (permission.ends_with(".write") || permission=="work.validate") {return Ok(false);}
    let n:i64=sqlx::query_scalar("SELECT count(*) FROM role_grants g JOIN role_permissions p ON p.role_code=g.role_code JOIN users u ON u.id=g.user_id JOIN buildings b ON b.organisation_id=u.organisation_id WHERE g.user_id=? AND b.id=? AND g.organisation_id=b.organisation_id AND g.role_code<>'prestataire' AND (g.building_id IS NULL OR g.building_id=b.id) AND p.permission_code=? AND (g.expires_at IS NULL OR g.expires_at>?)").bind(user).bind(building).bind(permission).bind(db::now()).fetch_one(&core.pool).await?;
    Ok(n > 0)
}
async fn assigned(core: &Core, user: &str, id: &str, building: &str) -> Result<bool> {
    let n:i64=sqlx::query_scalar("SELECT count(*) FROM role_grants g JOIN role_permissions p ON p.role_code=g.role_code AND p.permission_code='work.read' JOIN users u ON u.id=g.user_id JOIN buildings b ON b.organisation_id=u.organisation_id WHERE g.user_id=? AND b.id=? AND g.organisation_id=b.organisation_id AND g.role_code='prestataire' AND (g.building_id IS NULL OR g.building_id=b.id) AND (g.expires_at IS NULL OR g.expires_at>?) AND EXISTS(SELECT 1 FROM work_participants p WHERE p.work_id=? AND (p.user_id=u.id OR p.provider_id IN(SELECT provider_id FROM provider_users WHERE user_id=u.id)))").bind(user).bind(building).bind(db::now()).bind(id).fetch_one(&core.pool).await?;
    Ok(n > 0)
}
async fn read(core: &Core, user: &str, id: &str, building: &str) -> Result<bool> {
    Ok(permit(core, user, building, "work.read").await?
        || assigned(core, user, id, building).await?)
}
pub async fn access(core: &Core, token: &str, id: &str, write: bool) -> Result<(String, String)> {
    let actor = auth::require(core, token, None).await?;
    let building: String = sqlx::query_scalar("SELECT building_id FROM works WHERE id=?")
        .bind(id)
        .fetch_one(&core.pool)
        .await?;
    if !read(core, &actor, id, &building).await?
        || (write && !permit(core, &actor, &building, "work.write").await?)
    {
        bail!("Votre rôle ne permet pas cette opération sur ce travail.");
    }
    Ok((actor, building))
}
pub(crate) async fn writable(core: &Core, id: &str, physical: bool) -> Result<()> {
    let r=sqlx::query("SELECT w.status,e.archived_at FROM works w JOIN entities e ON e.id=w.building_id WHERE w.id=?").bind(id).fetch_one(&core.pool).await?;
    let status: String = r.get(0);
    if r.get::<Option<String>, _>(1).is_some()
        || status == "archived"
        || (physical && status == "closed")
    {
        bail!("Travail ou bâtiment archivé, ou travail clôturé : réactivez-le avant cette modification.");
    }
    Ok(())
}
pub(crate) async fn cas(core: &Core, id: &str, version: i64) -> Result<()> {
    let current: i64 = sqlx::query_scalar("SELECT revision FROM works WHERE id=?")
        .bind(id)
        .fetch_one(&core.pool)
        .await?;
    if current != version {
        bail!("Le travail a été modifié. Actualisez avant de recommencer.");
    }
    Ok(())
}
async fn payload(tx: &mut Tx<'_>, id: &str) -> Result<Value> {
    let r=sqlx::query("SELECT w.*,b.name AS building_name,u.display_name AS validator FROM works w JOIN buildings b ON b.id=w.building_id LEFT JOIN users u ON u.id=w.validated_by WHERE w.id=?").bind(id).fetch_one(&mut **tx).await?;
    let mut v = json!({"id":id,"building_id":r.get::<String,_>("building_id"),"building":r.get::<String,_>("building_name"),"reference":r.get::<String,_>("reference"),"title":r.get::<String,_>("title"),"description":r.get::<String,_>("description"),"kind":r.get::<String,_>("type"),"origin":r.get::<String,_>("origin"),"observation_id":r.get::<Option<String>,_>("observation_id"),"priority":r.get::<i64,_>("priority"),"status":r.get::<String,_>("status"),"planned_start":r.get::<Option<String>,_>("planned_start"),"planned_end":r.get::<Option<String>,_>("planned_end"),"actual_start":r.get::<Option<String>,_>("actual_start"),"actual_end":r.get::<Option<String>,_>("actual_end"),"target_due_on":r.get::<Option<String>,_>("target_due_on"),"estimated_budget_cents":r.get::<i64,_>("estimated_budget_cents"),"progress_percent":r.get::<i64,_>("progress_percent"),"project_manager":r.get::<Option<String>,_>("project_manager"),"architect":r.get::<Option<String>,_>("architect"),"commission_authorisation":r.get::<Option<String>,_>("commission_authorisation"),"safety_impact":r.get::<Option<String>,_>("safety_impact"),"compensatory_measures":r.get::<Option<String>,_>("compensatory_measures"),"validated_at":r.get::<Option<String>,_>("validated_at"),"validator":r.get::<Option<String>,_>("validator"),"version":r.get::<i64,_>("revision")});
    v["abandoned_reason"] = json!(r.get::<Option<String>, _>("abandoned_reason"));
    v["abandoned_from"] = json!(r.get::<Option<String>, _>("abandoned_from"));
    v["auto_linked"] = json!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM work_observation_sync WHERE work_id=?")
            .bind(id)
            .fetch_one(&mut **tx)
            .await?
            > 0
    );
    let costs=sqlx::query("SELECT c.id,c.description,c.amount_cents,c.incurred_on,c.receipt_file_id,r.counter_id FROM work_costs c LEFT JOIN work_cost_reversals r ON r.cost_id=c.id WHERE c.work_id=? ORDER BY c.incurred_on,c.id").bind(id).fetch_all(&mut **tx).await?;
    let actual: i64 = costs.iter().map(|c| c.get::<i64, _>(2)).sum();
    v["actual_budget_cents"] = json!(actual);
    v["costs"]=json!(costs.into_iter().map(|c|json!({"id":c.get::<String,_>(0),"description":c.get::<String,_>(1),"amount_cents":c.get::<i64,_>(2),"incurred_on":c.get::<String,_>(3),"file_id":c.get::<Option<String>,_>(4),"reversed_by":c.get::<Option<String>,_>(5)})).collect::<Vec<_>>());
    v["participants"]=json!(sqlx::query("SELECT p.user_id,p.provider_id,p.role,coalesce(u.display_name,v.name),u.active FROM work_participants p LEFT JOIN users u ON u.id=p.user_id LEFT JOIN providers v ON v.id=p.provider_id WHERE p.work_id=? ORDER BY p.role").bind(id).fetch_all(&mut **tx).await?.into_iter().map(|p|json!({"user_id":p.get::<Option<String>,_>(0),"provider_id":p.get::<Option<String>,_>(1),"role":p.get::<String,_>(2),"name":p.get::<String,_>(3),"active":p.get::<Option<i64>,_>(4)})).collect::<Vec<_>>());
    v["contacts"]=json!(sqlx::query("SELECT name,role,contact FROM work_contacts WHERE work_id=? ORDER BY name").bind(id).fetch_all(&mut **tx).await?.into_iter().map(|c|json!({"name":c.get::<String,_>(0),"role":c.get::<String,_>(1),"contact":c.get::<String,_>(2)})).collect::<Vec<_>>());
    v["predecessors"] = json!(
        sqlx::query_scalar::<_, String>(
            "SELECT predecessor_id FROM work_dependencies WHERE work_id=? ORDER BY predecessor_id"
        )
        .bind(id)
        .fetch_all(&mut **tx)
        .await?
    );
    v["equipment"]=json!(sqlx::query("SELECT e.id,e.reference,e.designation FROM equipment e JOIN work_equipment w ON w.equipment_id=e.id WHERE w.work_id=? ORDER BY e.reference").bind(id).fetch_all(&mut **tx).await?.into_iter().map(|e|json!({"id":e.get::<String,_>(0),"reference":e.get::<String,_>(1),"name":e.get::<String,_>(2)})).collect::<Vec<_>>());
    v["files"]=json!(sqlx::query("SELECT f.id,f.original_name,f.mime_type,f.byte_size,d.purpose FROM work_documents d JOIN files f ON f.id=d.file_id WHERE d.work_id=? AND d.active=1 ORDER BY f.created_at").bind(id).fetch_all(&mut **tx).await?.into_iter().map(|f|json!({"id":f.get::<String,_>(0),"name":f.get::<String,_>(1),"mime":f.get::<String,_>(2),"size":f.get::<i64,_>(3),"purpose":f.get::<String,_>(4)})).collect::<Vec<_>>());
    if let Some(obs) = r.get::<Option<String>, _>("observation_id") {
        let o=sqlx::query("SELECT o.reference,o.description,o.severity,o.status,o.due_on,o.visit_id,r.entry_id,e.block FROM observations o LEFT JOIN register_observations r ON r.observation_id=o.id LEFT JOIN register_entries e ON e.id=r.entry_id WHERE o.id=?").bind(obs).fetch_one(&mut **tx).await?;
        v["observation"] = json!({"reference":o.get::<String,_>(0),"description":o.get::<String,_>(1),"severity":o.get::<i64,_>(2),"status":o.get::<String,_>(3),"due_on":o.get::<Option<String>,_>(4),"commission":o.get::<Option<String>,_>(5).is_some(),"entry_id":o.get::<Option<String>,_>(6),"block":o.get::<Option<String>,_>(7)});
    }
    Ok(v)
}
fn redact(v: &mut Value, budget: bool) {
    if !budget {
        if let Some(o) = v.as_object_mut() {
            o.remove("estimated_budget_cents");
            o.remove("actual_budget_cents");
            o.remove("costs");
        }
        if let Some(files) = v["files"].as_array_mut() {
            files.retain(|f| !matches!(f["purpose"].as_str(), Some("quote" | "invoice")));
        }
    }
    v["budget_visible"] = json!(budget);
}
async fn revision(
    tx: &mut Tx<'_>,
    key: &[u8],
    id: &str,
    actor: &str,
    why: &str,
    event: &str,
) -> Result<()> {
    sqlx::query("UPDATE works SET revision=revision+1 WHERE id=?")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    let v = payload(tx, id).await?;
    let n = v["version"].as_i64().unwrap();
    crate::inventory::linked_work_changed(tx, key, id, actor).await?;
    sqlx::query("INSERT INTO entity_versions VALUES(?,?,?,?,?,?)")
        .bind(id)
        .bind(n)
        .bind(v.to_string())
        .bind(db::now())
        .bind(actor)
        .bind(why)
        .execute(&mut **tx)
        .await?;
    audit::append(
        tx,
        key,
        Some(actor),
        event,
        json!({"work_id":id,"version":n,"reason":why}),
    )
    .await
}
async fn deadline(tx: &mut Tx<'_>, id: &str) -> Result<()> {
    let r = sqlx::query(
        "SELECT building_id,status,title,coalesce(target_due_on,planned_end) FROM works WHERE id=?",
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query("UPDATE deadlines SET completed_at=? WHERE entity_id=? AND kind LIKE 'work:%' AND completed_at IS NULL").bind(db::now()).bind(id).execute(&mut **tx).await?;
    if !matches!(r.get::<String, _>(1).as_str(), "closed" | "archived") {
        if let Some(due) = r.get::<Option<String>, _>(3) {
            sqlx::query("INSERT INTO deadlines(id,entity_id,building_id,kind,due_on) VALUES(?,?,?,?,?) ON CONFLICT(entity_id,kind,due_on) DO UPDATE SET completed_at=NULL").bind(db::id()).bind(id).bind(r.get::<String,_>(0)).bind(format!("work:{}",r.get::<String,_>(2))).bind(due).execute(&mut **tx).await?;
        }
    }
    Ok(())
}
async fn reference(tx: &mut Tx<'_>, org: &str) -> Result<String> {
    let year = chrono::Utc::now()
        .with_timezone(&chrono_tz::Europe::Paris)
        .format("%Y")
        .to_string()
        .parse::<i64>()?;
    loop {
        let n:i64=sqlx::query_scalar("INSERT INTO reference_counters VALUES(?,'TRA',?,1) ON CONFLICT(organisation_id,prefix,year) DO UPDATE SET last_value=last_value+1 RETURNING last_value").bind(org).bind(year).fetch_one(&mut **tx).await?;
        let r = format!("TRA-{year}-{n:04}");
        let exists:i64=sqlx::query_scalar("SELECT count(*) FROM works w JOIN entities e ON e.id=w.id WHERE e.organisation_id=? AND w.reference=?").bind(org).bind(&r).fetch_one(&mut **tx).await?;
        if exists == 0 {
            return Ok(r);
        }
    }
}
async fn system(tx: &mut Tx<'_>, org: &str, key: &[u8]) -> Result<String> {
    if let Some(id) =
        sqlx::query_scalar("SELECT id FROM users WHERE organisation_id=? AND is_system=1")
            .bind(org)
            .fetch_optional(&mut **tx)
            .await?
    {
        return Ok(id);
    }
    let id = db::id();
    let hash = crypto::password_hash(&hex::encode(crypto::random_key().as_slice()))?;
    sqlx::query("INSERT INTO users(id,organisation_id,login,display_name,password_hash,active,created_at,is_system) VALUES(?,?,?,'Système — automatisation',?,0,?,1)").bind(&id).bind(org).bind(format!("_system_{}",db::id())).bind(hash).bind(db::now()).execute(&mut **tx).await?;
    audit::append(
        tx,
        key,
        None,
        "system.identity.create",
        json!({"id":id,"organisation_id":org}),
    )
    .await?;
    Ok(id)
}
pub async fn sync_observation(tx: &mut Tx<'_>, key: &[u8], obs: &str) -> Result<()> {
    let r=sqlx::query("SELECT o.building_id,o.description,o.severity,o.due_on,o.status,o.visit_id,e.organisation_id,b.archived_at FROM observations o JOIN entities e ON e.id=o.id JOIN entities b ON b.id=o.building_id WHERE o.id=? AND e.archived_at IS NULL").bind(obs).fetch_optional(&mut **tx).await?;
    let Some(r) = r else { return Ok(()) };
    if !matches!(r.get::<String, _>(4).as_str(), "open" | "in_progress")
        || r.get::<Option<String>, _>(7).is_some()
    {
        return Ok(());
    }
    let org: String = r.get(6);
    let building: String = r.get(0);
    let severity: i64 = r.get(2);
    let due: Option<String> = r.get(3);
    let mapped: Option<String> =
        sqlx::query_scalar("SELECT work_id FROM work_observation_sync WHERE observation_id=?")
            .bind(obs)
            .fetch_optional(&mut **tx)
            .await?;
    if let Some(id) = mapped {
        let w = sqlx::query("SELECT priority,target_due_on,status FROM works WHERE id=?")
            .bind(&id)
            .fetch_one(&mut **tx)
            .await?;
        if !matches!(w.get::<String, _>(2).as_str(), "closed" | "archived")
            && (severity > w.get::<i64, _>(0) || due != w.get::<Option<String>, _>(1))
        {
            let actor = system(tx, &org, key).await?;
            sqlx::query("UPDATE works SET priority=max(priority,?),target_due_on=? WHERE id=?")
                .bind(severity)
                .bind(due)
                .bind(&id)
                .execute(&mut **tx)
                .await?;
            deadline(tx, &id).await?;
            revision(
                tx,
                key,
                &id,
                &actor,
                "Actualisation de l’observation de commission",
                "work.observation.sync",
            )
            .await?;
        }
        return Ok(());
    }
    if r.get::<Option<String>, _>(5).is_none() {
        return Ok(());
    }
    if let Some(id) = sqlx::query_scalar::<_, String>(
        "SELECT id FROM works WHERE observation_id=? ORDER BY id LIMIT 1",
    )
    .bind(obs)
    .fetch_optional(&mut **tx)
    .await?
    {
        sqlx::query("INSERT INTO work_observation_sync VALUES(?,?)")
            .bind(obs)
            .bind(id)
            .execute(&mut **tx)
            .await?;
        return Ok(());
    }
    let actor = system(tx, &org, key).await?;
    let id = db::id();
    let reference = reference(tx, &org).await?;
    let description: String = r.get(1);
    let title = format!(
        "Observation commission · {}",
        description.chars().take(120).collect::<String>()
    );
    sqlx::query("INSERT INTO entities(id,organisation_id,building_id,kind,created_at,created_by) VALUES(?,?,?,'work',?,?)").bind(&id).bind(&org).bind(&building).bind(db::now()).bind(&actor).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO works(id,building_id,reference,title,description,type,origin,observation_id,priority,target_due_on) VALUES(?,?,?,?,?,'compliance','commission_observation',?,?,?)").bind(&id).bind(&building).bind(reference).bind(title).bind(description).bind(obs).bind(severity).bind(due).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO work_observation_sync VALUES(?,?)")
        .bind(obs)
        .bind(&id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("INSERT INTO work_status_events VALUES(?,?,NULL,'identified',?,?,'Création automatique depuis la commission')").bind(db::id()).bind(&id).bind(db::now()).bind(&actor).execute(&mut **tx).await?;
    deadline(tx, &id).await?;
    revision(
        tx,
        key,
        &id,
        &actor,
        "Création automatique depuis une observation de commission",
        "work.auto.create",
    )
    .await
}
pub async fn sync_legacy(pool: &SqlitePool, key: &[u8]) -> Result<()> {
    let ids:Vec<String>=sqlx::query_scalar("SELECT id FROM observations WHERE status IN('open','in_progress') AND (visit_id IS NOT NULL OR id IN(SELECT observation_id FROM work_observation_sync))").fetch_all(pool).await?;
    if ids.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for id in ids {
        sync_observation(&mut tx, key, &id).await?;
    }
    tx.commit().await?;
    Ok(())
}
pub async fn save(core: &Core, token: &str, input: Input) -> Result<String> {
    crate::shared_workspace::require_write(core)?;
    let actor = auth::require(core, token, None).await?;
    if !permit(core, &actor, &input.building_id, "work.write").await? {
        bail!("Votre rôle ne permet pas de gérer les travaux de ce bâtiment.");
    }
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
        .bind(&actor)
        .fetch_one(&core.pool)
        .await?;
    let archived: Option<String> =
        sqlx::query_scalar("SELECT archived_at FROM entities WHERE id=?")
            .bind(&input.building_id)
            .fetch_one(&core.pool)
            .await?;
    if archived.is_some() {
        bail!("Réactivez le bâtiment avant de créer ou modifier un travail.");
    }
    reason(&input.reason)?;
    if input.title.trim().is_empty()
        || input.title.len() > 250
        || input.description.len() > 30000
        || !(1..=4).contains(&input.priority)
        || !(0..=100).contains(&input.progress_percent)
        || !(0..=1_000_000_000_000).contains(&input.estimated_budget_cents)
        || input.equipment_ids.len() > 500
        || input.predecessors.len() > 100
        || input.participants.len() > 100
        || input.contacts.len() > 100
    {
        bail!("Vérifiez les champs et les limites du travail.");
    }
    for s in [
        &input.project_manager,
        &input.architect,
        &input.commission_authorisation,
        &input.safety_impact,
        &input.compensatory_measures,
    ] {
        if s.len() > 10000 {
            bail!("Une note dépasse 10 000 caractères.");
        }
    }
    if !["preventive", "corrective", "compliance"].contains(&input.kind.as_str())
        || ![
            "commission_observation",
            "failure",
            "regulation",
            "maintenance",
            "other",
        ]
        .contains(&input.origin.as_str())
    {
        bail!("Type ou origine du travail invalide.");
    }
    for d in [
        &input.planned_start,
        &input.planned_end,
        &input.actual_start,
        &input.actual_end,
        &input.target_due_on,
    ] {
        date(d)?;
    }
    if input.planned_start.is_some() != input.planned_end.is_some()
        || input.planned_start > input.planned_end
    {
        bail!("Renseignez les deux dates prévisionnelles dans l’ordre.");
    }
    if input.actual_end.is_some() && input.actual_start.is_none()
        || input
            .actual_start
            .as_deref()
            .is_some_and(|d| d > today().as_str())
        || input
            .actual_end
            .as_deref()
            .is_some_and(|d| d > today().as_str())
        || input.actual_end.is_some() && input.actual_start > input.actual_end
    {
        bail!("Les dates réelles doivent être cohérentes et ne peuvent pas être futures.");
    }
    let mut historical_commission = false;
    if let Some(id) = &input.id {
        let old=sqlx::query("SELECT w.observation_id,w.origin FROM works w JOIN work_observation_sync s ON s.work_id=w.id WHERE w.id=?").bind(id).fetch_optional(&core.pool).await?;
        if let Some(old) = old {
            historical_commission = old.get::<String, _>(1) == "commission_observation";
            if input.observation_id != old.get::<Option<String>, _>(0)
                || input.origin != old.get::<String, _>(1)
            {
                bail!("Le lien automatique avec l’observation ne peut pas être supprimé ou réaffecté.");
            }
        }
    }
    let mut target = input.target_due_on.clone();
    if let Some(obs) = &input.observation_id {
        let r =
            sqlx::query("SELECT building_id,visit_id,severity,due_on FROM observations WHERE id=?")
                .bind(obs)
                .fetch_one(&core.pool)
                .await?;
        if r.get::<String, _>(0) != input.building_id
            || input.origin == "commission_observation"
                && r.get::<Option<String>, _>(1).is_none()
                && !historical_commission
        {
            bail!("L’observation ne correspond pas au bâtiment ou à une commission.");
        }
        if input.priority < r.get::<i64, _>(2) {
            bail!("La priorité ne peut pas être inférieure à la gravité de l’observation.");
        }
        target = r.get(3);
    } else if input.origin == "commission_observation" {
        bail!("Sélectionnez une observation de commission.");
    }
    let status = if let Some(id) = &input.id {
        let (_, b) = access(core, token, id, true).await?;
        if b != input.building_id {
            bail!("Le bâtiment d’un travail ne peut pas être changé.");
        }
        writable(core, id, true).await?;
        cas(core, id, input.base_version).await?;
        sqlx::query_scalar::<_, String>("SELECT status FROM works WHERE id=?")
            .bind(id)
            .fetch_one(&core.pool)
            .await?
    } else {
        if input.base_version != 0 {
            bail!("Version initiale invalide.");
        }
        "identified".into()
    };
    if matches!(status.as_str(), "identified" | "scheduled")
        && (input.actual_start.is_some()
            || input.actual_end.is_some()
            || input.progress_percent != 0)
    {
        bail!("Démarrez le travail avant de renseigner son avancement et ses dates réelles.");
    }
    if status == "scheduled" && input.planned_start.is_none() {
        bail!("Un travail programmé doit conserver ses dates prévisionnelles.");
    }
    if matches!(status.as_str(), "in_progress" | "pending_validation")
        && input.actual_start.is_none()
    {
        bail!("La date réelle de début est requise.");
    }
    if status == "pending_validation"
        && (input.actual_end.is_none() || input.progress_percent != 100)
    {
        bail!("Reprenez le travail en cours avant de retirer sa date de fin ou son achèvement.");
    }
    let mut seen = HashSet::new();
    for id in &input.equipment_ids {
        if !seen.insert(id) {
            bail!("Équipement en double.");
        }
        let n: i64 =
            sqlx::query_scalar("SELECT count(*) FROM equipment WHERE id=? AND building_id=?")
                .bind(id)
                .bind(&input.building_id)
                .fetch_one(&core.pool)
                .await?;
        if n != 1 {
            bail!("Équipement d’un autre bâtiment ou inconnu.");
        }
    }
    let mut seen = HashSet::new();
    for p in &input.participants {
        if p.role.trim().is_empty()
            || p.role.len() > 250
            || p.user_id.is_some() == p.provider_id.is_some()
        {
            bail!("Choisissez un intervenant et son rôle.");
        }
        let tag = format!("{:?}:{:?}:{}", p.user_id, p.provider_id, p.role);
        if !seen.insert(tag) {
            bail!("Intervenant en double.");
        }
        let n: i64 = if let Some(id) = &p.user_id {
            sqlx::query_scalar(
                "SELECT count(*) FROM users WHERE id=? AND organisation_id=? AND is_system=0",
            )
            .bind(id)
            .bind(&org)
            .fetch_one(&core.pool)
            .await?
        } else {
            sqlx::query_scalar("SELECT count(*) FROM providers p JOIN entities e ON e.id=p.id WHERE p.id=? AND e.organisation_id=?").bind(&p.provider_id).bind(&org).fetch_one(&core.pool).await?
        };
        if n != 1 {
            bail!("Intervenant extérieur à l’organisation.");
        }
    }
    for c in &input.contacts {
        if c.name.trim().is_empty()
            || c.name.len() > 250
            || c.role.trim().is_empty()
            || c.role.len() > 250
            || c.contact.len() > 2000
        {
            bail!("Renseignez le nom et le rôle de chaque contact externe.");
        }
    }
    let mut seen = HashSet::new();
    for pred in &input.predecessors {
        if !seen.insert(pred) || input.id.as_ref() == Some(pred) {
            bail!("Prédécesseur en double ou identique au travail.");
        }
        let p = sqlx::query("SELECT building_id,planned_end FROM works WHERE id=?")
            .bind(pred)
            .fetch_one(&core.pool)
            .await?;
        if p.get::<String, _>(0) != input.building_id {
            bail!("Les dépendances doivent rester dans le même bâtiment.");
        }
        if let Some(id) = &input.id {
            let cycle:i64=sqlx::query_scalar("WITH RECURSIVE chain(id) AS(SELECT predecessor_id FROM work_dependencies WHERE work_id=? UNION SELECT d.predecessor_id FROM work_dependencies d JOIN chain c ON d.work_id=c.id) SELECT count(*) FROM chain WHERE id=?").bind(pred).bind(id).fetch_one(&core.pool).await?;
            if cycle > 0 {
                bail!("Cette dépendance créerait une boucle dans le planning.");
            }
        }
    }
    let id = input.id.clone().unwrap_or_else(db::id);
    let mut tx = core.pool.begin().await?;
    if input.id.is_none() {
        let reference = reference(&mut tx, &org).await?;
        sqlx::query("INSERT INTO entities(id,organisation_id,building_id,kind,created_at,created_by) VALUES(?,?,?,'work',?,?)").bind(&id).bind(&org).bind(&input.building_id).bind(db::now()).bind(&actor).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO works(id,building_id,reference,title,description,type,origin,priority) VALUES(?,?,?,?,?,?,?,?)").bind(&id).bind(&input.building_id).bind(reference).bind(input.title.trim()).bind(&input.description).bind(&input.kind).bind(&input.origin).bind(input.priority).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO work_status_events VALUES(?,?,NULL,'identified',?,?,?)")
            .bind(db::id())
            .bind(&id)
            .bind(db::now())
            .bind(&actor)
            .bind(&input.reason)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE works SET title=?,description=?,type=?,origin=?,observation_id=?,priority=?,planned_start=?,planned_end=?,actual_start=?,actual_end=?,target_due_on=?,estimated_budget_cents=?,progress_percent=?,project_manager=?,architect=?,commission_authorisation=?,safety_impact=?,compensatory_measures=? WHERE id=?").bind(input.title.trim()).bind(&input.description).bind(&input.kind).bind(&input.origin).bind(&input.observation_id).bind(input.priority).bind(&input.planned_start).bind(&input.planned_end).bind(&input.actual_start).bind(&input.actual_end).bind(&target).bind(input.estimated_budget_cents).bind(input.progress_percent).bind(&input.project_manager).bind(&input.architect).bind(&input.commission_authorisation).bind(&input.safety_impact).bind(&input.compensatory_measures).bind(&id).execute(&mut *tx).await?;
    for table in [
        "work_equipment",
        "work_dependencies",
        "work_participants",
        "work_contacts",
    ] {
        let sql = format!("DELETE FROM {table} WHERE work_id=?");
        sqlx::query(&sql).bind(&id).execute(&mut *tx).await?;
    }
    for eq in &input.equipment_ids {
        sqlx::query("INSERT INTO work_equipment VALUES(?,?,?)")
            .bind(&id)
            .bind(eq)
            .bind(&input.building_id)
            .execute(&mut *tx)
            .await?;
    }
    for p in &input.predecessors {
        sqlx::query("INSERT INTO work_dependencies(work_id,predecessor_id) VALUES(?,?)")
            .bind(&id)
            .bind(p)
            .execute(&mut *tx)
            .await?;
    }
    for p in &input.participants {
        sqlx::query("INSERT INTO work_participants VALUES(?,?,?,?,?)")
            .bind(db::id())
            .bind(&id)
            .bind(&p.user_id)
            .bind(&p.provider_id)
            .bind(&p.role)
            .execute(&mut *tx)
            .await?;
    }
    for c in &input.contacts {
        sqlx::query("INSERT INTO work_contacts VALUES(?,?,?,?,?)")
            .bind(db::id())
            .bind(&id)
            .bind(c.name.trim())
            .bind(c.role.trim())
            .bind(&c.contact)
            .execute(&mut *tx)
            .await?;
    }
    deadline(&mut tx, &id).await?;
    revision(&mut tx, &core.key, &id, &actor, &input.reason, "work.save").await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await?;
    Ok(id)
}
pub async fn transition(
    core: &Core,
    token: &str,
    id: &str,
    version: i64,
    to: &str,
    why: &str,
) -> Result<()> {
    crate::shared_workspace::require_write(core)?;
    transition_impl(core, token, id, version, to, why, false).await
}
pub async fn simple_transition(
    core: &Core,
    token: &str,
    id: &str,
    version: i64,
    done: bool,
    why: &str,
) -> Result<()> {
    crate::shared_workspace::require_write(core)?;
    let (actor, building) = access(core, token, id, false).await?;
    let from: String = sqlx::query_scalar("SELECT status FROM works WHERE id=?")
        .bind(id)
        .fetch_one(&core.pool)
        .await?;
    let target = if done {
        if from == "pending_validation" || permit(core, &actor, &building, "work.validate").await? {
            "closed"
        } else {
            "pending_validation"
        }
    } else {
        "in_progress"
    };
    if done && !matches!(from.as_str(), "in_progress" | "pending_validation") {
        bail!("Démarrez l’intervention avant de la terminer.")
    }
    transition_impl(core, token, id, version, target, why, true).await
}
async fn transition_impl(
    core: &Core,
    token: &str,
    id: &str,
    version: i64,
    to: &str,
    why: &str,
    simple: bool,
) -> Result<()> {
    let (actor, building) = access(core, token, id, false).await?;
    reason(why)?;
    cas(core, id, version).await?;
    let r = sqlx::query(
        "SELECT status,planned_start,planned_end,actual_start,actual_end FROM works WHERE id=?",
    )
    .bind(id)
    .fetch_one(&core.pool)
    .await?;
    let from: String = r.get(0);
    let edge = matches!(
        (from.as_str(), to),
        ("identified", "scheduled")
            | ("scheduled", "identified")
            | ("scheduled", "in_progress")
            | ("in_progress", "pending_validation")
            | ("pending_validation", "in_progress")
            | ("pending_validation", "closed")
            | ("closed", "in_progress")
            | ("closed", "archived")
            | ("archived", "closed")
    );
    let edge = edge
        || (simple
            && matches!(
                (from.as_str(), to),
                ("identified", "in_progress") | ("in_progress", "closed")
            ));
    if !edge {
        bail!("Cette transition ne respecte pas le cycle des travaux.");
    }
    let permission = if from == "closed" || from == "archived" || to == "closed" {
        "work.validate"
    } else {
        "work.write"
    };
    if !permit(core, &actor, &building, permission).await? {
        bail!("Votre rôle ne permet pas cette transition ou validation.");
    }
    let archived: Option<String> =
        sqlx::query_scalar("SELECT archived_at FROM entities WHERE id=?")
            .bind(&building)
            .fetch_one(&core.pool)
            .await?;
    if archived.is_some() {
        bail!("Le bâtiment est archivé.");
    }
    if !simple
        && matches!(to, "scheduled" | "in_progress" | "pending_validation")
        && (r.get::<Option<String>, _>(1).is_none() || r.get::<Option<String>, _>(2).is_none())
    {
        bail!("Renseignez d’abord les dates prévisionnelles dans la fiche.");
    }
    let mut actual_start: Option<String> = r.get(3);
    let mut actual_end: Option<String> = r.get(4);
    if to == "in_progress" {
        if actual_start.is_none() {
            actual_start = Some(today())
        }
        actual_end = None;
    }
    if to == "pending_validation" || (simple && to == "closed") {
        if actual_start.is_none() {
            bail!("Date de démarrage réelle absente.");
        }
        if actual_end.is_none() {
            actual_end = Some(today());
        }
    }
    let mut tx = core.pool.begin().await?;
    sqlx::query("UPDATE works SET status=?,actual_start=?,actual_end=?,progress_percent=CASE WHEN ? IN('pending_validation','closed','archived') THEN 100 WHEN ?='in_progress' AND status IN('closed','pending_validation') THEN min(progress_percent,95) ELSE progress_percent END,validated_by=CASE WHEN ?='closed' AND status IN('pending_validation','in_progress') THEN ? WHEN ?='in_progress' THEN NULL ELSE validated_by END,validated_at=CASE WHEN ?='closed' AND status IN('pending_validation','in_progress') THEN ? WHEN ?='in_progress' THEN NULL ELSE validated_at END WHERE id=?").bind(to).bind(actual_start).bind(actual_end).bind(to).bind(to).bind(to).bind(&actor).bind(to).bind(to).bind(db::now()).bind(to).bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE entities SET archived_at=? WHERE id=?")
        .bind(if to == "archived" {
            Some(db::now())
        } else {
            None
        })
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO work_status_events VALUES(?,?,?,?,?,?,?)")
        .bind(db::id())
        .bind(id)
        .bind(&from)
        .bind(to)
        .bind(db::now())
        .bind(&actor)
        .bind(why)
        .execute(&mut *tx)
        .await?;
    deadline(&mut tx, id).await?;
    revision(&mut tx, &core.key, id, &actor, why, "work.transition").await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await
}
pub async fn cost(
    core: &Core,
    token: &str,
    id: &str,
    version: i64,
    description: &str,
    amount: i64,
    incurred_on: &str,
    file: Option<String>,
    why: &str,
) -> Result<()> {
    crate::shared_workspace::require_write(core)?;
    let (actor, b) = access(core, token, id, true).await?;
    if !permit(core, &actor, &b, "work.budget.read").await? {
        bail!("Accès budgétaire refusé.");
    }
    writable(core, id, false).await?;
    cas(core, id, version).await?;
    reason(why)?;
    if description.trim().is_empty()
        || description.len() > 2000
        || !(1..=1_000_000_000_000).contains(&amount)
    {
        bail!("Description requise et montant positif limité à 10 milliards d’euros.");
    }
    date(&Some(incurred_on.into()))?;
    if incurred_on > today().as_str() {
        bail!("Une dépense réalisée ne peut pas avoir une date future.");
    }
    if let Some(file) = &file {
        let n:i64=sqlx::query_scalar("SELECT count(*) FROM work_documents WHERE work_id=? AND file_id=? AND purpose IN('invoice','quote')").bind(id).bind(file).fetch_one(&core.pool).await?;
        if n != 1 {
            bail!("Justificatif extérieur à ce travail ou catégorie non financière.");
        }
    }
    let mut tx = core.pool.begin().await?;
    sqlx::query("INSERT INTO work_costs(id,work_id,description,amount_cents,incurred_on,receipt_file_id) VALUES(?,?,?,?,?,?)").bind(db::id()).bind(id).bind(description.trim()).bind(amount).bind(incurred_on).bind(file).execute(&mut *tx).await?;
    revision(&mut tx, &core.key, id, &actor, why, "work.cost.add").await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await
}
pub async fn reverse_cost(
    core: &Core,
    token: &str,
    id: &str,
    version: i64,
    cost_id: &str,
    why: &str,
) -> Result<()> {
    crate::shared_workspace::require_write(core)?;
    let (actor, b) = access(core, token, id, true).await?;
    if !permit(core, &actor, &b, "work.budget.read").await? {
        bail!("Accès budgétaire refusé.");
    }
    writable(core, id, false).await?;
    cas(core, id, version).await?;
    reason(why)?;
    let r=sqlx::query("SELECT amount_cents,receipt_file_id,description FROM work_costs WHERE id=? AND work_id=? AND amount_cents>0 AND NOT EXISTS(SELECT 1 FROM work_cost_reversals WHERE cost_id=?)").bind(cost_id).bind(id).bind(cost_id).fetch_optional(&core.pool).await?.ok_or_else(||anyhow::anyhow!("Dépense introuvable ou déjà annulée."))?;
    let counter = db::id();
    let mut tx = core.pool.begin().await?;
    sqlx::query("INSERT INTO work_costs(id,work_id,description,amount_cents,incurred_on,receipt_file_id) VALUES(?,?,?,?,?,?)").bind(&counter).bind(id).bind(format!("Annulation : {}",r.get::<String,_>(2))).bind(-r.get::<i64,_>(0)).bind(today()).bind(r.get::<Option<String>,_>(1)).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO work_cost_reversals VALUES(?,?)")
        .bind(cost_id)
        .bind(counter)
        .execute(&mut *tx)
        .await?;
    revision(&mut tx, &core.key, id, &actor, why, "work.cost.reverse").await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await
}
pub async fn snapshot(core: &Core, token: &str) -> Result<Value> {
    let actor = auth::require(core, token, None).await?;
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
        .bind(&actor)
        .fetch_one(&core.pool)
        .await?;
    let ids=sqlx::query("SELECT w.id,w.building_id FROM works w JOIN entities e ON e.id=w.id WHERE e.organisation_id=? ORDER BY w.priority DESC,w.reference").bind(&org).fetch_all(&core.pool).await?;
    let mut records = vec![];
    for r in ids {
        let id: String = r.get(0);
        let b: String = r.get(1);
        if !read(core, &actor, &id, &b).await? {
            continue;
        }
        let budget = permit(core, &actor, &b, "work.budget.read").await?;
        let write = permit(core, &actor, &b, "work.write").await?;
        let validate = permit(core, &actor, &b, "work.validate").await?;
        let archived: Option<String> =
            sqlx::query_scalar("SELECT archived_at FROM entities WHERE id=?")
                .bind(&b)
                .fetch_one(&core.pool)
                .await?;
        let mut tx = core.pool.begin().await?;
        let mut v = payload(&mut tx, &id).await?;
        tx.commit().await?;
        redact(&mut v, budget);
        v["can_write"] = json!(write && archived.is_none());
        v["can_validate"] = json!(validate && archived.is_none());
        v["can_read_register"] = json!(permit(core, &actor, &b, "register.read").await?);
        records.push(v);
    }
    let buildings=sqlx::query("SELECT b.id,b.name,e.archived_at FROM buildings b JOIN entities e ON e.id=b.id WHERE b.organisation_id=? ORDER BY b.name").bind(&org).fetch_all(&core.pool).await?;
    let mut bs = vec![];
    let mut write_buildings = vec![];
    for b in buildings {
        let id: String = b.get(0);
        let write = permit(core, &actor, &id, "work.write").await?
            && b.get::<Option<String>, _>(2).is_none();
        if permit(core, &actor, &id, "work.read").await?
            || records.iter().any(|w| w["building_id"] == id)
        {
            bs.push(json!({"id":id,"name":b.get::<String,_>(1),"archived":b.get::<Option<String>,_>(2).is_some(),"can_create":write,"budget_visible":permit(core,&actor,&id,"work.budget.read").await?}));
            if write {
                write_buildings.push(id);
            }
        }
    }
    let mut observations = vec![];
    let mut equipment = vec![];
    for b in &write_buildings {
        for o in sqlx::query("SELECT o.id,o.reference,o.description,o.severity,o.status,o.due_on,o.visit_id FROM observations o JOIN entities e ON e.id=o.id WHERE o.building_id=? AND e.archived_at IS NULL ORDER BY issued_on DESC").bind(b).fetch_all(&core.pool).await?{observations.push(json!({"id":o.get::<String,_>(0),"reference":o.get::<String,_>(1),"description":o.get::<String,_>(2),"severity":o.get::<i64,_>(3),"status":o.get::<String,_>(4),"due_on":o.get::<Option<String>,_>(5),"commission":o.get::<Option<String>,_>(6).is_some(),"building_id":b}));}
        for e in sqlx::query("SELECT e.id,e.reference,e.designation FROM equipment e JOIN entities n ON n.id=e.id WHERE e.building_id=? AND n.archived_at IS NULL ORDER BY e.reference").bind(b).fetch_all(&core.pool).await?{equipment.push(json!({"id":e.get::<String,_>(0),"reference":e.get::<String,_>(1),"name":e.get::<String,_>(2),"building_id":b}));}
    }
    let users = if write_buildings.is_empty() {
        vec![]
    } else {
        sqlx::query("SELECT id,display_name,active FROM users WHERE organisation_id=? AND is_system=0 ORDER BY display_name").bind(&org).fetch_all(&core.pool).await?.into_iter().map(|u|json!({"id":u.get::<String,_>(0),"name":u.get::<String,_>(1),"active":u.get::<i64,_>(2)==1})).collect()
    };
    let providers = if write_buildings.is_empty() {
        vec![]
    } else {
        sqlx::query("SELECT p.id,p.name,e.archived_at FROM providers p JOIN entities e ON e.id=p.id WHERE e.organisation_id=? ORDER BY p.name").bind(&org).fetch_all(&core.pool).await?.into_iter().map(|p|json!({"id":p.get::<String,_>(0),"name":p.get::<String,_>(1),"archived":p.get::<Option<String>,_>(2).is_some()})).collect()
    };
    Ok(
        json!({"records":records,"buildings":bs,"observations":observations,"equipment":equipment,"users":users,"providers":providers,"today":today()}),
    )
}
pub async fn history(core: &Core, token: &str, id: &str) -> Result<Value> {
    let (actor, b) = access(core, token, id, false).await?;
    let budget = permit(core, &actor, &b, "work.budget.read").await?;
    let rows=sqlx::query("SELECT v.version,v.payload_json,v.recorded_at,u.display_name,v.reason FROM entity_versions v JOIN users u ON u.id=v.recorded_by WHERE v.entity_id=? ORDER BY version DESC").bind(id).fetch_all(&core.pool).await?;
    let mut out = vec![];
    for r in rows {
        let mut value: Value = serde_json::from_str(&r.get::<String, _>(1))?;
        redact(&mut value, budget);
        out.push(json!({"version":r.get::<i64,_>(0),"payload":value,"recorded_at":r.get::<String,_>(2),"actor":r.get::<String,_>(3),"reason":if budget{r.get::<String,_>(4)}else{"Mise à jour du travail".into()}}));
    }
    Ok(json!(out))
}
pub async fn attach(
    core: &Core,
    token: &str,
    id: &str,
    version: i64,
    purpose: &str,
    path: &std::path::Path,
) -> Result<()> {
    crate::shared_workspace::require_write(core)?;
    let (actor, b) = access(core, token, id, true).await?;
    writable(core, id, false).await?;
    cas(core, id, version).await?;
    if !["quote", "invoice", "report", "photo", "other"].contains(&purpose) {
        bail!("Catégorie de document invalide.");
    }
    if matches!(purpose, "quote" | "invoice")
        && !permit(core, &actor, &b, "work.budget.read").await?
    {
        bail!("Accès budgétaire refusé.");
    }
    if std::fs::metadata(path)?.len() > 20 * 1024 * 1024 {
        bail!("Document limité à 20 Mio.");
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM work_documents WHERE work_id=? AND active=1")
            .bind(id)
            .fetch_one(&core.pool)
            .await?;
    if count >= 100 {
        bail!("Un travail est limité à 100 pièces jointes actives.");
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
    let mut tx = core.pool.begin().await?;
    sqlx::query(
        "INSERT INTO entity_files(entity_id,file_id,organisation_id,purpose) VALUES(?,?,?,?)",
    )
    .bind(id)
    .bind(&file)
    .bind(&org)
    .bind(format!("work.{purpose}"))
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO work_documents(work_id,file_id,purpose) VALUES(?,?,?)")
        .bind(id)
        .bind(&file)
        .bind(purpose)
        .execute(&mut *tx)
        .await?;
    revision(
        &mut tx,
        &core.key,
        id,
        &actor,
        &format!("Ajout du document {name}"),
        "work.file.add",
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await
}
pub async fn file_active(
    core: &Core,
    token: &str,
    id: &str,
    version: i64,
    file: &str,
    active: bool,
    why: &str,
) -> Result<()> {
    crate::shared_workspace::require_write(core)?;
    let (actor, b) = access(core, token, id, true).await?;
    writable(core, id, false).await?;
    cas(core, id, version).await?;
    reason(why)?;
    let purpose: String =
        sqlx::query_scalar("SELECT purpose FROM work_documents WHERE work_id=? AND file_id=?")
            .bind(id)
            .bind(file)
            .fetch_one(&core.pool)
            .await?;
    if matches!(purpose.as_str(), "quote" | "invoice")
        && !permit(core, &actor, &b, "work.budget.read").await?
    {
        bail!("Accès budgétaire refusé.");
    }
    let mut tx = core.pool.begin().await?;
    sqlx::query("UPDATE work_documents SET active=? WHERE work_id=? AND file_id=?")
        .bind(active)
        .bind(id)
        .bind(file)
        .execute(&mut *tx)
        .await?;
    revision(&mut tx, &core.key, id, &actor, why, "work.file.link").await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await
}
pub async fn file(core: &Core, token: &str, id: &str, file: &str) -> Result<Value> {
    let (actor, b) = access(core, token, id, false).await?;
    let r=sqlx::query("SELECT d.purpose,f.organisation_id,f.original_name,f.mime_type FROM work_documents d JOIN files f ON f.id=d.file_id WHERE d.work_id=? AND d.file_id=?").bind(id).bind(file).fetch_one(&core.pool).await?;
    if matches!(r.get::<String, _>(0).as_str(), "quote" | "invoice")
        && !permit(core, &actor, &b, "work.budget.read").await?
    {
        bail!("Votre rôle ne peut pas consulter les devis et factures.");
    }
    let bytes = vault::read(core, file, &r.get::<String, _>(1)).await?;
    Ok(json!({"name":r.get::<String,_>(2),"mime":r.get::<String,_>(3),"bytes":B64.encode(&*bytes)}))
}
pub async fn search(core: &Core, token: &str, query: &str) -> Result<Value> {
    if query.chars().count() < 2 || query.len() > 200 {
        return Ok(json!([]));
    };
    let actor = auth::require(core, token, None).await?;
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
        .bind(&actor)
        .fetch_one(&core.pool)
        .await?;
    let rows=sqlx::query("SELECT w.id,w.building_id,w.reference,w.title,w.description FROM works w JOIN entities e ON e.id=w.id WHERE e.organisation_id=? AND w.status<>'archived' ORDER BY w.priority DESC,w.reference").bind(org).fetch_all(&core.pool).await?;
    let q = query.to_lowercase();
    let mut out = vec![];
    for r in rows {
        let id: String = r.get(0);
        let b: String = r.get(1);
        if format!(
            "{} {} {}",
            r.get::<String, _>(2),
            r.get::<String, _>(3),
            r.get::<String, _>(4)
        )
        .to_lowercase()
        .contains(&q)
            && read(core, &actor, &id, &b).await?
        {
            out.push(
                json!({"id":id,"reference":r.get::<String,_>(2),"title":r.get::<String,_>(3)}),
            );
            if out.len() >= 50 {
                break;
            }
        }
    }
    Ok(json!(out))
}
pub async fn abandon(
    core: &Core,
    token: &str,
    id: &str,
    version: i64,
    why: &str,
    restore: bool,
) -> Result<()> {
    crate::shared_workspace::require_write(core)?;
    let (actor, b) = access(core, token, id, false).await?;
    if !permit(core, &actor, &b, "work.validate").await? {
        bail!("La décision doit être prise par un responsable habilité à valider.");
    }
    reason(why)?;
    cas(core, id, version).await?;
    let building_archived: Option<String> =
        sqlx::query_scalar("SELECT archived_at FROM entities WHERE id=?")
            .bind(&b)
            .fetch_one(&core.pool)
            .await?;
    if building_archived.is_some() {
        bail!("Le bâtiment est archivé.");
    }
    let r = sqlx::query("SELECT status,abandoned_from,observation_id FROM works WHERE id=?")
        .bind(id)
        .fetch_one(&core.pool)
        .await?;
    let from: String = r.get(0);
    let to = if restore {
        if from != "archived" {
            bail!("Le travail n’est pas classé sans suite.");
        }
        r.get::<Option<String>, _>(1)
            .ok_or_else(|| anyhow::anyhow!("Utilisez la réactivation normale de ce dossier."))?
    } else {
        if matches!(from.as_str(), "closed" | "archived") {
            bail!("Le travail est déjà clôturé ou archivé.");
        }
        if let Some(obs) = r.get::<Option<String>, _>(2) {
            let status: String = sqlx::query_scalar("SELECT status FROM observations WHERE id=?")
                .bind(obs)
                .fetch_one(&core.pool)
                .await?;
            if matches!(status.as_str(), "open" | "in_progress") {
                bail!("L’observation est encore ouverte. Consignez sa levée ou son acceptation dans le registre avant de classer ce travail sans suite.");
            }
        }
        "archived".into()
    };
    let mut tx = core.pool.begin().await?;
    sqlx::query("UPDATE works SET status=?,abandoned_reason=?,abandoned_from=?,validated_by=?,validated_at=? WHERE id=?").bind(&to).bind(if restore{None}else{Some(why)}).bind(if restore{None}else{Some(from.as_str())}).bind(if restore{None}else{Some(actor.as_str())}).bind(if restore{None}else{Some(db::now())}).bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE entities SET archived_at=? WHERE id=?")
        .bind(if restore { None } else { Some(db::now()) })
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO work_status_events VALUES(?,?,?,?,?,?,?)")
        .bind(db::id())
        .bind(id)
        .bind(&from)
        .bind(&to)
        .bind(db::now())
        .bind(&actor)
        .bind(why)
        .execute(&mut *tx)
        .await?;
    deadline(&mut tx, id).await?;
    revision(
        &mut tx,
        &core.key,
        id,
        &actor,
        why,
        if restore {
            "work.abandon.restore"
        } else {
            "work.abandon"
        },
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await
}
