use crate::{audit, auth, dashboard, db, Core};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Row, SqliteConnection};
type Tx<'a> = sqlx::Transaction<'a, sqlx::Sqlite>;
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Input {
    pub id: Option<String>,
    pub building_id: String,
    pub designation: String,
    pub category_id: String,
    pub location_id: Option<String>,
    pub manufacturer: String,
    pub model: String,
    pub serial_number: String,
    pub purchased_on: Option<String>,
    pub installed_on: Option<String>,
    pub commissioned_on: Option<String>,
    pub lifespan_months: Option<i64>,
    pub life_due_override: Option<String>,
    pub purchase_cost_cents: Option<i64>,
    pub rated_power_w: Option<i64>,
    pub condition: String,
    pub maintenance_interval_months: Option<i64>,
    pub maintenance_interval_days: Option<i64>,
    pub last_maintenance_on: Option<String>,
    pub maintenance_due_override: Option<String>,
    pub notes: String,
    pub base_version: i64,
    pub reason: String,
}
pub fn today() -> String {
    crate::works::today()
}
fn date(s: &Option<String>) -> Result<()> {
    if let Some(s) = s {
        let d = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")?;
        if d.to_string() != *s {
            bail!("Date invalide : utilisez le calendrier.")
        }
    }
    Ok(())
}
fn reason(s: &str) -> Result<()> {
    if s.trim().chars().count() < 3 || s.len() > 1000 {
        bail!("Renseignez un commentaire entre 3 et 1 000 caractères.")
    }
    Ok(())
}
fn due(base: &Option<String>, months: Option<i64>, days: Option<i64>) -> Result<Option<String>> {
    if let Some(s) = base {
        let d = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")?;
        let result = if let Some(n) = months {
            d.checked_add_months(chrono::Months::new(n.try_into()?))
        } else if let Some(n) = days {
            d.checked_add_signed(chrono::Duration::days(n))
        } else {
            None
        };
        if (months.is_some() || days.is_some()) && result.is_none() {
            bail!("Date calculée hors limites.")
        }
        return Ok(result.map(|d| d.to_string()));
    }
    Ok(None)
}
async fn permit(c: &Core, u: &str, b: &str, p: &str) -> Result<bool> {
    dashboard::allowed(c, u, b, p).await
}
pub async fn access(c: &Core, token: &str, id: &str, write: bool) -> Result<(String, String)> {
    let u = auth::require(c, token, None).await?;
    let b: String = sqlx::query_scalar("SELECT building_id FROM equipment WHERE id=?")
        .bind(id)
        .fetch_one(&c.pool)
        .await?;
    if !permit(
        c,
        &u,
        &b,
        if write {
            "inventory.write"
        } else {
            "inventory.read"
        },
    )
    .await?
    {
        bail!("Votre rôle ne permet pas cette opération sur cet équipement.")
    }
    if write && !permit(c, &u, &b, "inventory.read").await? {
        bail!("Lecture de cet équipement non autorisée.")
    }
    Ok((u, b))
}
async fn available(c: &Core, b: &str, id: Option<&str>, version: i64) -> Result<()> {
    let archived: Option<String> =
        sqlx::query_scalar("SELECT archived_at FROM entities WHERE id=?")
            .bind(b)
            .fetch_one(&c.pool)
            .await?;
    if archived.is_some() {
        bail!("Le bâtiment est archivé.")
    }
    if let Some(id) = id {
        let row=sqlx::query("SELECT q.revision,e.archived_at FROM equipment q JOIN entities e ON e.id=q.id WHERE q.id=?").bind(id).fetch_one(&c.pool).await?;
        if row.get::<i64, _>(0) != version {
            bail!("La fiche a changé. Actualisez-la avant de recommencer.")
        }
        if row.get::<Option<String>, _>(1).is_some() {
            bail!("Réactivez l’équipement avant de le modifier.")
        }
    }
    Ok(())
}
async fn profile(conn: &mut SqliteConnection, id: &str) -> Result<Value> {
    let r=sqlx::query("SELECT q.*,e.organisation_id,e.archived_at,b.name AS building,be.archived_at AS building_archived,c.name AS category,l.name AS location FROM equipment q JOIN entities e ON e.id=q.id JOIN buildings b ON b.id=q.building_id JOIN entities be ON be.id=b.id JOIN equipment_categories c ON c.id=q.category_id LEFT JOIN locations l ON l.id=q.location_id WHERE q.id=?").bind(id).fetch_one(&mut *conn).await?;
    let mut v = json!({"id":id,"building_id":r.get::<String,_>("building_id"),"building":r.get::<String,_>("building"),"reference":r.get::<String,_>("reference"),"designation":r.get::<String,_>("designation"),"category_id":r.get::<String,_>("category_id"),"category":r.get::<String,_>("category"),"location_id":r.get::<Option<String>,_>("location_id"),"location":r.get::<Option<String>,_>("location"),"manufacturer":r.get::<Option<String>,_>("manufacturer"),"model":r.get::<Option<String>,_>("model"),"serial_number":r.get::<Option<String>,_>("serial_number"),"purchased_on":r.get::<Option<String>,_>("purchased_on"),"installed_on":r.get::<Option<String>,_>("installed_on"),"commissioned_on":r.get::<Option<String>,_>("commissioned_on"),"lifespan_months":r.get::<Option<i64>,_>("lifespan_months"),"life_due_override":r.get::<Option<String>,_>("life_due_override"),"end_of_life_on":r.get::<Option<String>,_>("end_of_life_on"),"purchase_cost_cents":r.get::<Option<i64>,_>("purchase_cost_cents"),"rated_power_w":r.get::<Option<i64>,_>("rated_power_w"),"condition":r.get::<String,_>("condition"),"maintenance_interval_months":r.get::<Option<i64>,_>("maintenance_interval_months"),"maintenance_interval_days":r.get::<Option<i64>,_>("maintenance_interval_days"),"last_maintenance_on":r.get::<Option<String>,_>("last_maintenance_on"),"maintenance_due_override":r.get::<Option<String>,_>("maintenance_due_override"),"next_maintenance_on":r.get::<Option<String>,_>("next_maintenance_on"),"notes":r.get::<Option<String>,_>("notes"),"asset_key":r.get::<String,_>("asset_key"),"version":r.get::<i64,_>("revision"),"archived":r.get::<Option<String>,_>("archived_at").is_some(),"building_archived":r.get::<Option<String>,_>("building_archived").is_some()});
    v["photos"]=json!(sqlx::query("SELECT f.id,f.original_name,f.mime_type,f.byte_size FROM equipment_photos p JOIN files f ON f.id=p.file_id WHERE p.equipment_id=? AND p.active=1 ORDER BY f.created_at,f.id").bind(id).fetch_all(&mut *conn).await?.into_iter().map(|r|json!({"id":r.get::<String,_>(0),"name":r.get::<String,_>(1),"mime":r.get::<String,_>(2),"size":r.get::<i64,_>(3)})).collect::<Vec<_>>());
    v["works"]=json!(sqlx::query("SELECT w.id,w.reference,w.title,w.status FROM work_equipment we JOIN works w ON w.id=we.work_id WHERE we.equipment_id=? ORDER BY w.reference").bind(id).fetch_all(&mut *conn).await?.into_iter().map(|r|json!({"id":r.get::<String,_>(0),"reference":r.get::<String,_>(1),"title":r.get::<String,_>(2),"status":r.get::<String,_>(3)})).collect::<Vec<_>>());
    Ok(v)
}
fn redact(v: &mut Value, cost: bool, work: bool) {
    v["cost_visible"] = json!(cost);
    if !cost {
        v["purchase_cost_cents"] = Value::Null
    }
    if !work {
        v["works"] = json!([])
    }
}
async fn deadlines(tx: &mut Tx<'_>, id: &str) -> Result<()> {
    let r=sqlx::query("SELECT q.building_id,q.reference,q.designation,q.next_maintenance_on,q.end_of_life_on,e.archived_at,b.archived_at FROM equipment q JOIN entities e ON e.id=q.id JOIN entities b ON b.id=q.building_id WHERE q.id=?").bind(id).fetch_one(&mut **tx).await?;
    sqlx::query("UPDATE deadlines SET completed_at=? WHERE entity_id=? AND kind LIKE 'equipment:%' AND completed_at IS NULL").bind(db::now()).bind(id).execute(&mut **tx).await?;
    if r.get::<Option<String>, _>(5).is_none() && r.get::<Option<String>, _>(6).is_none() {
        for (column, label) in [(3, "Entretien"), (4, "Fin de vie prévue")] {
            if let Some(day) = r.get::<Option<String>, _>(column) {
                let kind = format!(
                    "equipment:{label} · {} · {}",
                    r.get::<String, _>(1),
                    r.get::<String, _>(2)
                );
                sqlx::query("INSERT INTO deadlines(id,entity_id,building_id,kind,due_on) VALUES(?,?,?,?,?) ON CONFLICT(entity_id,kind,due_on) DO UPDATE SET completed_at=NULL").bind(db::id()).bind(id).bind(r.get::<String,_>(0)).bind(kind).bind(day).execute(&mut **tx).await?;
            }
        }
    }
    Ok(())
}
async fn revision(
    tx: &mut Tx<'_>,
    key: &[u8],
    id: &str,
    actor: &str,
    kind: &str,
    description: &str,
    occurred: Option<&str>,
    work: Option<&str>,
) -> Result<()> {
    sqlx::query("UPDATE equipment SET revision=revision+1 WHERE id=?")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    let v = profile(&mut **tx, id).await?;
    let n = v["version"].as_i64().unwrap();
    sqlx::query("INSERT INTO entity_versions VALUES(?,?,?,?,?,?)")
        .bind(id)
        .bind(n)
        .bind(v.to_string())
        .bind(db::now())
        .bind(actor)
        .bind(description)
        .execute(&mut **tx)
        .await?;
    sqlx::query("INSERT INTO equipment_events VALUES(?,?,?,?,?,?,?,?)")
        .bind(db::id())
        .bind(id)
        .bind(occurred.map(str::to_owned).unwrap_or_else(db::now))
        .bind(db::now())
        .bind(actor)
        .bind(kind)
        .bind(description)
        .bind(work)
        .execute(&mut **tx)
        .await?;
    audit::append(
        tx,
        key,
        Some(actor),
        &format!("equipment.{kind}"),
        json!({"equipment_id":id,"version":n,"description":description}),
    )
    .await
}
pub async fn snapshot(c: &Core, token: &str) -> Result<Value> {
    let user = auth::require(c, token, None).await?;
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
        .bind(&user)
        .fetch_one(&c.pool)
        .await?;
    let mut buildings = vec![];
    let mut permissions = std::collections::BTreeMap::new();
    for r in sqlx::query("SELECT b.id,b.name,e.archived_at FROM buildings b JOIN entities e ON e.id=b.id WHERE b.organisation_id=? ORDER BY b.name").bind(&org).fetch_all(&c.pool).await?{let id:String=r.get(0);if permit(c,&user,&id,"inventory.read").await?{let write=permit(c,&user,&id,"inventory.write").await?;let cost=permit(c,&user,&id,"inventory.cost.read").await?;let work=permit(c,&user,&id,"work.read").await?;let archived=r.get::<Option<String>,_>(2).is_some();buildings.push(json!({"id":id,"name":r.get::<String,_>(1),"archived":archived,"cost_visible":cost,"can_write":write&&!archived,"can_add_work":permit(c,&user,&id,"work.write").await?&&!archived}));permissions.insert(id,(write,cost,work));}}
    let mut records = vec![];
    for r in sqlx::query("SELECT q.id,q.building_id FROM equipment q JOIN entities e ON e.id=q.id WHERE e.organisation_id=? ORDER BY q.reference").bind(&org).fetch_all(&c.pool).await?{let b:String=r.get(1);if let Some((write,cost,work))=permissions.get(&b){let id:String=r.get(0);let mut conn=c.pool.acquire().await?;let mut v=profile(&mut conn,&id).await?;redact(&mut v,*cost,*work);v["can_write"]=json!(*write&&!v["archived"].as_bool().unwrap_or(true)&&!v["building_archived"].as_bool().unwrap_or(true));records.push(v);}}
    let categories=sqlx::query("SELECT id,parent_id,code,name FROM equipment_categories ORDER BY name").fetch_all(&c.pool).await?.into_iter().map(|r|json!({"id":r.get::<String,_>(0),"parent_id":r.get::<Option<String>,_>(1),"code":r.get::<String,_>(2),"name":r.get::<String,_>(3)})).collect::<Vec<_>>();
    let mut locations = vec![];
    for r in sqlx::query(
        "SELECT id,building_id,parent_id,kind,name FROM locations ORDER BY sort_order,name",
    )
    .fetch_all(&c.pool)
    .await?
    {
        if permissions.contains_key(&r.get::<String, _>(1)) {
            locations.push(json!({"id":r.get::<String,_>(0),"building_id":r.get::<String,_>(1),"parent_id":r.get::<Option<String>,_>(2),"kind":r.get::<String,_>(3),"name":r.get::<String,_>(4)}));
        }
    }
    Ok(
        json!({"records":records,"buildings":buildings,"categories":categories,"locations":locations,"today":today()}),
    )
}
pub async fn detail(c: &Core, token: &str, id: &str) -> Result<Value> {
    let (u, b) = access(c, token, id, false).await?;
    let mut conn = c.pool.acquire().await?;
    let mut v = profile(&mut conn, id).await?;
    drop(conn);
    redact(
        &mut v,
        permit(c, &u, &b, "inventory.cost.read").await?,
        permit(c, &u, &b, "work.read").await?,
    );
    v["can_write"] = json!(
        permit(c, &u, &b, "inventory.write").await?
            && !v["archived"].as_bool().unwrap_or(true)
            && !v["building_archived"].as_bool().unwrap_or(true)
    );
    v["readings"]=json!(sqlx::query("SELECT r.*,u.display_name,cc.reason AS cancellation_reason,cc.recorded_at AS cancelled_at FROM consumption_readings r JOIN users u ON u.id=r.recorded_by LEFT JOIN consumption_cancellations cc ON cc.reading_id=r.id WHERE r.equipment_id=? ORDER BY r.measured_at,r.id LIMIT 5000").bind(id).fetch_all(&c.pool).await?.into_iter().map(|r|json!({"id":r.get::<String,_>("id"),"date":r.get::<String,_>("measured_at"),"quantity":r.get::<i64,_>("quantity_milliunits"),"unit":r.get::<String,_>("unit"),"mode":r.get::<String,_>("mode"),"period_start":r.get::<Option<String>,_>("period_start"),"reset":r.get::<i64,_>("reset")==1,"actor":r.get::<String,_>("display_name"),"cancelled":r.get::<Option<String>,_>("cancelled_at").is_some(),"cancellation_reason":r.get::<Option<String>,_>("cancellation_reason")})).collect::<Vec<_>>());
    Ok(v)
}
pub(crate) struct Prepared { input: Input, actor: String, org: String, life: Option<String>, maintenance: Option<String> }
pub(crate) async fn prepare(c: &Core, token: &str, mut input: Input) -> Result<Prepared> {
    reason(&input.reason)?;
    let actor = auth::require(c, token, None).await?;
    if !permit(c, &actor, &input.building_id, "inventory.write").await? {
        bail!("Création ou modification non autorisée sur ce bâtiment.")
    }
    available(
        c,
        &input.building_id,
        input.id.as_deref(),
        input.base_version,
    )
    .await?;
    if input.designation.trim().is_empty()
        || input.designation.len() > 250
        || input.manufacturer.len() > 250
        || input.model.len() > 250
        || input.serial_number.len() > 250
        || input.notes.len() > 20000
        || !["new", "good", "worn", "faulty", "out_of_service"].contains(&input.condition.as_str())
    {
        bail!("Vérifiez la désignation, les informations et l’état de l’équipement.")
    }
    for d in [
        &input.purchased_on,
        &input.installed_on,
        &input.commissioned_on,
        &input.life_due_override,
        &input.last_maintenance_on,
        &input.maintenance_due_override,
    ] {
        date(d)?
    }
    let now = today();
    for d in [
        &input.purchased_on,
        &input.installed_on,
        &input.commissioned_on,
        &input.last_maintenance_on,
    ] {
        if d.as_ref().is_some_and(|s| s > &now) {
            bail!("Une date réelle ne peut pas être dans le futur.")
        }
    }
    if input.installed_on.is_some()
        && input.commissioned_on.is_some()
        && input.commissioned_on < input.installed_on
    {
        bail!("La mise en service doit suivre l’installation.")
    }
    for (value, max) in [
        (input.lifespan_months, 1200),
        (input.maintenance_interval_months, 120),
        (input.maintenance_interval_days, 3650),
    ] {
        if value.is_some_and(|v| v < 1 || v > max) {
            bail!("Durée ou fréquence hors limites.")
        }
    }
    if input.maintenance_interval_days.is_some() && input.maintenance_interval_months.is_some() {
        bail!("Choisissez une seule unité de fréquence.")
    }
    if input
        .purchase_cost_cents
        .is_some_and(|v| v < 0 || v > 1_000_000_000_000)
        || input
            .rated_power_w
            .is_some_and(|v| v < 0 || v > 1_000_000_000)
    {
        bail!("Coût ou puissance invalide.")
    }
    let anchor = input.commissioned_on.clone().or(input.installed_on.clone());
    let life = input
        .life_due_override
        .clone()
        .or(due(&anchor, input.lifespan_months, None)?);
    if life.is_some() && anchor.is_some() && life < anchor {
        bail!("La fin de vie prévue précède la mise en service.")
    }
    let base = input.last_maintenance_on.clone().or(anchor);
    let maintenance = input.maintenance_due_override.clone().or(due(
        &base,
        input.maintenance_interval_months,
        input.maintenance_interval_days,
    )?);
    if maintenance.is_some()
        && input.last_maintenance_on.is_some()
        && maintenance < input.last_maintenance_on
    {
        bail!("Le prochain entretien précède le dernier entretien.")
    }
    if sqlx::query_scalar::<_, i64>("SELECT count(*) FROM equipment_categories WHERE id=?")
        .bind(&input.category_id)
        .fetch_one(&c.pool)
        .await?
        != 1
    {
        bail!("Catégorie inconnue.")
    }
    if let Some(location) = &input.location_id {
        if sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM locations WHERE id=? AND building_id=?",
        )
        .bind(location)
        .bind(&input.building_id)
        .fetch_one(&c.pool)
        .await?
            != 1
        {
            bail!("L’emplacement n’appartient pas au bâtiment.")
        }
    }
    if let Some(id) = &input.id {
        let (_, b) = access(c, token, id, true).await?;
        if b != input.building_id {
            bail!("Le bâtiment d’un équipement ne peut pas être changé.")
        }
    }
    if !permit(c, &actor, &input.building_id, "inventory.cost.read").await? {
        if input.purchase_cost_cents.is_some() {
            bail!("Accès au coût d’achat refusé.")
        }
        input.purchase_cost_cents = if let Some(id) = &input.id {
            sqlx::query_scalar("SELECT purchase_cost_cents FROM equipment WHERE id=?")
                .bind(id)
                .fetch_one(&c.pool)
                .await?
        } else {
            None
        };
    }
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
        .bind(&actor)
        .fetch_one(&c.pool)
        .await?;
    Ok(Prepared {input,actor,org,life,maintenance})
}
pub(crate) async fn write_prepared(tx: &mut Tx<'_>, key: &[u8], p: Prepared) -> Result<String> {
    let Prepared {input,actor,org,life,maintenance}=p;
    let id = input.id.clone().unwrap_or_else(db::id);
    if input.id.is_none() {
        let year = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::Paris)
            .format("%Y")
            .to_string()
            .parse::<i64>()?;
        let reference = loop {
            let n:i64=sqlx::query_scalar("INSERT INTO reference_counters VALUES(?,'EQP',?,1) ON CONFLICT(organisation_id,prefix,year) DO UPDATE SET last_value=last_value+1 RETURNING last_value").bind(&org).bind(year).fetch_one(&mut **tx).await?;
            let r = format!("EQP-{year}-{n:04}");
            if sqlx::query_scalar::<_,i64>("SELECT count(*) FROM equipment q JOIN entities e ON e.id=q.id WHERE e.organisation_id=? AND q.reference=?").bind(&org).bind(&r).fetch_one(&mut **tx).await?==0{break r}
        };
        let qr = db::id();
        sqlx::query("INSERT INTO entities(id,organisation_id,building_id,kind,created_at,created_by) VALUES(?,?,?,'equipment',?,?)").bind(&id).bind(&org).bind(&input.building_id).bind(db::now()).bind(&actor).execute(&mut **tx).await?;
        sqlx::query("INSERT INTO equipment(id,building_id,reference,designation,category_id,condition,qr_token,asset_key) VALUES(?,?,?,?,?,'good',?,?)").bind(&id).bind(&input.building_id).bind(reference).bind(&input.designation).bind(&input.category_id).bind(&qr).bind(&qr).execute(&mut **tx).await?;
    }
    // Effacer d’abord l’ancienne fréquence jours pour ne jamais laisser deux unités actives.
    sqlx::query("UPDATE equipment SET maintenance_interval_days=NULL WHERE id=?")
        .bind(&id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE equipment SET designation=?,category_id=?,location_id=?,manufacturer=?,model=?,serial_number=?,purchased_on=?,installed_on=?,commissioned_on=?,lifespan_months=?,life_due_override=?,end_of_life_on=?,purchase_cost_cents=?,rated_power_w=?,condition=?,maintenance_interval_months=?,maintenance_interval_days=?,last_maintenance_on=?,maintenance_due_override=?,next_maintenance_on=?,notes=? WHERE id=?").bind(input.designation.trim()).bind(&input.category_id).bind(&input.location_id).bind(&input.manufacturer).bind(&input.model).bind(&input.serial_number).bind(&input.purchased_on).bind(&input.installed_on).bind(&input.commissioned_on).bind(input.lifespan_months).bind(&input.life_due_override).bind(life).bind(input.purchase_cost_cents).bind(input.rated_power_w).bind(&input.condition).bind(input.maintenance_interval_months).bind(input.maintenance_interval_days).bind(&input.last_maintenance_on).bind(&input.maintenance_due_override).bind(maintenance).bind(&input.notes).bind(&id).execute(&mut **tx).await?;
    deadlines(tx, &id).await?;
    revision(
        tx,
        key,
        &id,
        &actor,
        if input.id.is_none() {
            "created"
        } else {
            "edited"
        },
        &input.reason,
        None,
        None,
    )
    .await?;
    Ok(id)
}
pub async fn save(c: &Core, token: &str, input: Input) -> Result<String> {
    crate::shared_workspace::require_write(c)?;
    let p=prepare(c,token,input).await?;
    let mut tx=c.pool.begin().await?;
    let id=write_prepared(&mut tx,&c.key,p).await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool,&c.root).await?;
    Ok(id)
}

pub async fn archive(
    c: &Core,
    token: &str,
    id: &str,
    version: i64,
    archived: bool,
    why: &str,
) -> Result<()> {
    crate::shared_workspace::require_write(c)?;
    let (u, b) = access(c, token, id, true).await?;
    reason(why)?;
    available(c, &b, None, 0).await?;
    let n: i64 = sqlx::query_scalar("SELECT revision FROM equipment WHERE id=?")
        .bind(id)
        .fetch_one(&c.pool)
        .await?;
    if n != version {
        bail!("La fiche a changé. Actualisez-la.")
    }
    let mut tx = c.pool.begin().await?;
    sqlx::query("UPDATE entities SET archived_at=? WHERE id=?")
        .bind(if archived { Some(db::now()) } else { None })
        .bind(id)
        .execute(&mut *tx)
        .await?;
    deadlines(&mut tx, id).await?;
    revision(
        &mut tx,
        &c.key,
        id,
        &u,
        if archived { "archived" } else { "restored" },
        why,
        None,
        None,
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}
pub async fn location_add(
    c: &Core,
    token: &str,
    building: &str,
    parent: Option<String>,
    kind: &str,
    name: &str,
) -> Result<String> {
    crate::shared_workspace::require_write(c)?;
    let user = auth::require(c, token, None).await?;
    if !permit(c, &user, building, "inventory.write").await? {
        bail!("Emplacement non autorisé.")
    }
    available(c, building, None, 0).await?;
    let rank = |s: &str| match s {
        "level" => 0,
        "zone" => 1,
        "room" => 2,
        _ => 9,
    };
    if rank(kind) == 9 || name.trim().is_empty() || name.len() > 200 {
        bail!("Vérifiez le type et le nom de l’emplacement.")
    }
    if let Some(id) = &parent {
        let row = sqlx::query("SELECT building_id,kind FROM locations WHERE id=?")
            .bind(id)
            .fetch_one(&c.pool)
            .await?;
        if row.get::<String, _>(0) != building || rank(&row.get::<String, _>(1)) >= rank(kind) {
            bail!("Choisissez un parent du même bâtiment : niveau, puis zone, puis pièce.")
        }
    }
    let id = db::id();
    let mut tx = c.pool.begin().await?;
    sqlx::query("INSERT INTO locations(id,building_id,parent_id,kind,name) VALUES(?,?,?,?,?)")
        .bind(&id)
        .bind(building)
        .bind(parent)
        .bind(kind)
        .bind(name.trim())
        .execute(&mut *tx)
        .await?;
    audit::append(
        &mut tx,
        &c.key,
        Some(&user),
        "inventory.location",
        json!({"id":id,"building_id":building,"kind":kind,"name":name.trim()}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await?;
    Ok(id)
}
pub async fn category_add(
    c: &Core,
    token: &str,
    parent: Option<String>,
    name: &str,
) -> Result<String> {
    crate::shared_workspace::require_write(c)?;
    let user = auth::require(c, token, Some("inventory.write")).await?;
    if name.trim().is_empty() || name.len() > 200 {
        bail!("Nom de catégorie invalide.")
    }
    if let Some(id) = &parent {
        let p: Option<String> =
            sqlx::query_scalar("SELECT parent_id FROM equipment_categories WHERE id=?")
                .bind(id)
                .fetch_one(&c.pool)
                .await?;
        if p.is_some() {
            bail!("Choisissez une catégorie principale comme parent.")
        }
    }
    let id = db::id();
    let mut tx = c.pool.begin().await?;
    sqlx::query("INSERT INTO equipment_categories VALUES(?,?,?,?)")
        .bind(&id)
        .bind(parent)
        .bind(format!("CAT-{id}"))
        .bind(name.trim())
        .execute(&mut *tx)
        .await?;
    audit::append(
        &mut tx,
        &c.key,
        Some(&user),
        "inventory.category",
        json!({"id":id,"name":name.trim()}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await?;
    Ok(id)
}
pub async fn maintenance(
    c: &Core,
    token: &str,
    id: &str,
    version: i64,
    day: String,
    next: Option<String>,
    work: Option<String>,
    why: &str,
) -> Result<()> {
    crate::shared_workspace::require_write(c)?;
    let (u, b) = access(c, token, id, true).await?;
    reason(why)?;
    available(c, &b, Some(id), version).await?;
    date(&Some(day.clone()))?;
    date(&next)?;
    if day > today() || next.as_ref().is_some_and(|v| v < &day) {
        bail!("Vérifiez la date réelle et le prochain entretien.")
    }
    let r=sqlx::query("SELECT last_maintenance_on,maintenance_interval_months,maintenance_interval_days,coalesce(commissioned_on,installed_on) FROM equipment WHERE id=?").bind(id).fetch_one(&c.pool).await?;
    if r.get::<Option<String>, _>(0).is_some_and(|d| day < d)
        || r.get::<Option<String>, _>(3).is_some_and(|d| day < d)
    {
        bail!("L’entretien précède une date déjà enregistrée.")
    }
    if let Some(w) = &work {
        if !permit(c, &u, &b, "work.read").await?
            || sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM works WHERE id=? AND building_id=?",
            )
            .bind(w)
            .bind(&b)
            .fetch_one(&c.pool)
            .await?
                != 1
        {
            bail!("Intervention liée non accessible ou d’un autre bâtiment.")
        }
    }
    let calculated = next
        .clone()
        .or(due(&Some(day.clone()), r.get(1), r.get(2))?);
    let mut tx = c.pool.begin().await?;
    sqlx::query("UPDATE equipment SET last_maintenance_on=?,next_maintenance_on=?,maintenance_due_override=? WHERE id=?").bind(&day).bind(calculated).bind(next).bind(id).execute(&mut *tx).await?;
    deadlines(&mut tx, id).await?;
    revision(
        &mut tx,
        &c.key,
        id,
        &u,
        "maintenance",
        why,
        Some(&day),
        work.as_deref(),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}
async fn sequence(tx: &mut Tx<'_>, id: &str) -> Result<()> {
    let mut prev = std::collections::BTreeMap::<String, i64>::new();
    for r in sqlx::query("SELECT r.unit,r.quantity_milliunits,r.reset FROM consumption_readings r WHERE r.equipment_id=? AND r.mode='cumulative' AND NOT EXISTS(SELECT 1 FROM consumption_cancellations c WHERE c.reading_id=r.id) ORDER BY r.measured_at,r.id").bind(id).fetch_all(&mut **tx).await?{let unit:String=r.get(0);let value:i64=r.get(1);if r.get::<i64,_>(2)==0&&prev.get(&unit).is_some_and(|old|*old>value){bail!("Un index ne peut pas diminuer sans remplacement/remise à zéro du compteur. Annuler une remise à zéro peut aussi rendre les relevés suivants incohérents.")}prev.insert(unit,value);}
    Ok(())
}
#[derive(Deserialize)]
pub struct ReadingInput {
    pub equipment_id: String,
    pub base_version: i64,
    pub date: String,
    pub quantity: i64,
    pub unit: String,
    pub mode: String,
    pub period_start: Option<String>,
    pub reset: bool,
    pub reason: String,
}
pub async fn reading(c: &Core, token: &str, input: ReadingInput) -> Result<()> {
    crate::shared_workspace::require_write(c)?;
    let (u, b) = access(c, token, &input.equipment_id, true).await?;
    available(c, &b, Some(&input.equipment_id), input.base_version).await?;
    reason(&input.reason)?;
    date(&Some(input.date.clone()))?;
    date(&input.period_start)?;
    if input.date > today()
        || input.quantity < 0
        || input.quantity > 1_000_000_000_000_000
        || !["kWh", "m3", "L", "h"].contains(&input.unit.as_str())
        || !["cumulative", "interval"].contains(&input.mode.as_str())
        || input.reset && input.mode != "cumulative"
    {
        bail!("Vérifiez la date, la quantité, l’unité et le type du relevé.")
    }
    if input.mode == "interval" && input.period_start.as_ref().is_none_or(|s| s >= &input.date)
        || input.mode == "cumulative" && input.period_start.is_some()
    {
        bail!("Une consommation doit avoir une période de début antérieure à sa date de fin.")
    }
    let id = db::id();
    let mut tx = c.pool.begin().await?;
    if sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM consumption_readings WHERE equipment_id=?",
    )
    .bind(&input.equipment_id)
    .fetch_one(&mut *tx)
    .await?
        >= 5000
    {
        bail!("La fiche contient déjà 5 000 relevés.")
    }
    if sqlx::query_scalar::<_,i64>("SELECT count(*) FROM consumption_readings r WHERE r.equipment_id=? AND r.unit=? AND r.mode=? AND r.measured_at=? AND NOT EXISTS(SELECT 1 FROM consumption_cancellations c WHERE c.reading_id=r.id)").bind(&input.equipment_id).bind(&input.unit).bind(&input.mode).bind(&input.date).fetch_one(&mut *tx).await?>0{bail!("Un relevé actif existe déjà à cette date ; annulez-le avant de le corriger.")}
    if input.mode=="interval"&&sqlx::query_scalar::<_,i64>("SELECT count(*) FROM consumption_readings r WHERE r.equipment_id=? AND r.unit=? AND r.mode='interval' AND r.measured_at>? AND r.period_start<? AND NOT EXISTS(SELECT 1 FROM consumption_cancellations c WHERE c.reading_id=r.id)").bind(&input.equipment_id).bind(&input.unit).bind(&input.period_start).bind(&input.date).fetch_one(&mut *tx).await?>0{bail!("Cette période recouvre une consommation déjà enregistrée.")}
    sqlx::query("INSERT INTO consumption_readings VALUES(?,?,?,?,?,?,?,?,?)")
        .bind(&id)
        .bind(&input.equipment_id)
        .bind(&input.date)
        .bind(input.quantity)
        .bind(&input.unit)
        .bind(&input.mode)
        .bind(&input.period_start)
        .bind(&u)
        .bind(input.reset as i64)
        .execute(&mut *tx)
        .await?;
    sequence(&mut tx, &input.equipment_id).await?;
    revision(
        &mut tx,
        &c.key,
        &input.equipment_id,
        &u,
        "reading",
        &input.reason,
        Some(&input.date),
        None,
    )
    .await?;
    audit::append(&mut tx,&c.key,Some(&u),"equipment.reading.value",json!({"reading_id":id,"quantity":input.quantity,"unit":input.unit,"mode":input.mode,"reset":input.reset,"date":input.date})).await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}
pub async fn reading_cancel(
    c: &Core,
    token: &str,
    id: &str,
    version: i64,
    reading: &str,
    why: &str,
) -> Result<()> {
    crate::shared_workspace::require_write(c)?;
    let (u, b) = access(c, token, id, true).await?;
    available(c, &b, Some(id), version).await?;
    reason(why)?;
    if sqlx::query_scalar::<_,i64>("SELECT count(*) FROM consumption_readings r WHERE r.id=? AND r.equipment_id=? AND NOT EXISTS(SELECT 1 FROM consumption_cancellations c WHERE c.reading_id=r.id)").bind(reading).bind(id).fetch_one(&c.pool).await?!=1{bail!("Relevé absent ou déjà annulé.")}
    let mut tx = c.pool.begin().await?;
    sqlx::query("INSERT INTO consumption_cancellations VALUES(?,?,?,?)")
        .bind(reading)
        .bind(db::now())
        .bind(&u)
        .bind(why)
        .execute(&mut *tx)
        .await?;
    sequence(&mut tx, id).await?;
    revision(
        &mut tx,
        &c.key,
        id,
        &u,
        "reading_cancelled",
        why,
        None,
        None,
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}
pub async fn photo_attach(
    c: &Core,
    token: &str,
    id: &str,
    version: i64,
    path: &std::path::Path,
) -> Result<()> {
    crate::shared_workspace::require_write(c)?;
    let (u, b) = access(c, token, id, true).await?;
    available(c, &b, Some(id), version).await?;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM equipment_photos WHERE equipment_id=? AND active=1",
    )
    .bind(id)
    .fetch_one(&c.pool)
    .await?;
    if n >= 16 || std::fs::metadata(path)?.len() > 8 * 1024 * 1024 {
        bail!("16 photos maximum, 8 Mio par photo.")
    }
    let bytes = std::fs::read(path)?;
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[255, 216, 255]) {
        "image/jpeg"
    } else {
        bail!("Choisissez une photo PNG ou JPEG.")
    };
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
        .bind(&u)
        .fetch_one(&c.pool)
        .await?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .context("Nom de photo invalide")?;
    let file = crate::vault::put(c, &org, &u, name, mime, &bytes).await?;
    let mut tx = c.pool.begin().await?;
    sqlx::query("INSERT INTO equipment_photos VALUES(?,?,1)")
        .bind(id)
        .bind(&file)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO entity_files(entity_id,file_id,organisation_id,purpose) VALUES(?,?,?,'equipment_photo')").bind(id).bind(file).bind(org).execute(&mut *tx).await?;
    revision(
        &mut tx,
        &c.key,
        id,
        &u,
        "photo",
        "Ajout d’une photo",
        None,
        None,
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}
pub async fn photo_remove(
    c: &Core,
    token: &str,
    id: &str,
    version: i64,
    file: &str,
    why: &str,
) -> Result<()> {
    crate::shared_workspace::require_write(c)?;
    let (u, b) = access(c, token, id, true).await?;
    available(c, &b, Some(id), version).await?;
    reason(why)?;
    let mut tx = c.pool.begin().await?;
    if sqlx::query(
        "UPDATE equipment_photos SET active=0 WHERE equipment_id=? AND file_id=? AND active=1",
    )
    .bind(id)
    .bind(file)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        != 1
    {
        bail!("Photo absente de la version courante.")
    }
    revision(&mut tx, &c.key, id, &u, "photo_removed", why, None, None).await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}
pub async fn photo(c: &Core, token: &str, id: &str, file: &str) -> Result<Value> {
    access(c, token, id, false).await?;
    let r=sqlx::query("SELECT f.organisation_id,f.mime_type,f.original_name FROM equipment_photos p JOIN files f ON f.id=p.file_id WHERE p.equipment_id=? AND p.file_id=?").bind(id).bind(file).fetch_one(&c.pool).await?;
    use base64::Engine;
    let bytes = crate::vault::read(c, file, &r.get::<String, _>(0)).await?;
    Ok(
        json!({"mime":r.get::<String,_>(1),"name":r.get::<String,_>(2),"bytes":base64::engine::general_purpose::STANDARD.encode(&bytes)}),
    )
}
pub async fn history(c: &Core, token: &str, id: &str, offset: i64) -> Result<Value> {
    let (u, b) = access(c, token, id, false).await?;
    if !(0..=1_000_000).contains(&offset) {
        bail!("Page invalide.")
    }
    let cost = permit(c, &u, &b, "inventory.cost.read").await?;
    let work = permit(c, &u, &b, "work.read").await?;
    let mut versions = vec![];
    for r in sqlx::query("SELECT v.version,v.payload_json,v.recorded_at,u.display_name,v.reason FROM entity_versions v JOIN users u ON u.id=v.recorded_by WHERE entity_id=? ORDER BY version DESC LIMIT 25 OFFSET ?").bind(id).bind(offset).fetch_all(&c.pool).await?{let mut v:Value=serde_json::from_str(&r.get::<String,_>(1))?;redact(&mut v,cost,work);versions.push(json!({"version":r.get::<i64,_>(0),"payload":v,"recorded_at":r.get::<String,_>(2),"actor":r.get::<String,_>(3),"reason":r.get::<String,_>(4)}));}
    let events=sqlx::query("SELECT e.*,u.display_name FROM equipment_events e JOIN users u ON u.id=e.recorded_by WHERE equipment_id=? ORDER BY recorded_at DESC,id DESC LIMIT 25 OFFSET ?").bind(id).bind(offset).fetch_all(&c.pool).await?.into_iter().map(|r|json!({"id":r.get::<String,_>("id"),"date":r.get::<String,_>("occurred_at"),"recorded_at":r.get::<String,_>("recorded_at"),"actor":r.get::<String,_>("display_name"),"kind":r.get::<String,_>("event_type"),"description":r.get::<String,_>("description")})).collect::<Vec<_>>();
    let more = versions.len() == 25 || events.len() == 25;
    Ok(json!({"versions":versions,"events":events,"more":more}))
}
pub async fn search(c: &Core, token: &str, query: &str) -> Result<Value> {
    let user = auth::require(c, token, None).await?;
    let q = query.trim();
    if q.len() > 250 {
        bail!("Recherche trop longue.")
    }
    if q.len() < 2 {
        return Ok(json!([]));
    }
    let mut out = vec![];
    for r in sqlx::query("SELECT q.id,q.building_id,q.reference,q.designation,b.name,q.asset_key,e.archived_at,be.archived_at FROM equipment q JOIN entities e ON e.id=q.id JOIN buildings b ON b.id=q.building_id JOIN entities be ON be.id=b.id ORDER BY q.reference").fetch_all(&c.pool).await?{let text=format!("{} {} {}",r.get::<String,_>(2),r.get::<String,_>(3),r.get::<String,_>(4));let matches=if let Some(token)=q.strip_prefix("SERP1:"){r.get::<String,_>(5)==token.trim()}else{text.to_lowercase().contains(&q.to_lowercase())};if matches&&permit(c,&user,&r.get::<String,_>(1),"inventory.read").await?{out.push(json!({"id":r.get::<String,_>(0),"reference":r.get::<String,_>(2),"designation":r.get::<String,_>(3),"building":r.get::<String,_>(4),"archived":r.get::<Option<String>,_>(6).is_some()||r.get::<Option<String>,_>(7).is_some()}));if out.len()>=30{break}}}
    if q.starts_with("SERP1:") {
        let active: Vec<_> = out
            .iter()
            .filter(|r| r["archived"] == false)
            .cloned()
            .collect();
        if active.len() > 1 {
            bail!("Ce QR correspond à plusieurs équipements actifs. Vérifiez les bâtiments reçus.")
        }
        if !active.is_empty() {
            return Ok(json!(active));
        }
    }
    Ok(json!(out))
}
// Appelé AVANT la nouvelle version du travail : conserver aussi la trace des liens retirés.
pub(crate) async fn linked_work_changed(
    tx: &mut Tx<'_>,
    key: &[u8],
    work: &str,
    actor: &str,
) -> Result<()> {
    let w = sqlx::query("SELECT building_id,reference,title FROM works WHERE id=?")
        .bind(work)
        .fetch_one(&mut **tx)
        .await?;
    let b: String = w.get(0);
    let current =
        sqlx::query_scalar::<_, String>("SELECT equipment_id FROM work_equipment WHERE work_id=?")
            .bind(work)
            .fetch_all(&mut **tx)
            .await?;
    let mut ids = current.clone();
    if let Some(previous) = sqlx::query_scalar::<_, String>(
        "SELECT payload_json FROM entity_versions WHERE entity_id=? ORDER BY version DESC LIMIT 1",
    )
    .bind(work)
    .fetch_optional(&mut **tx)
    .await?
    {
        if let Some(es) = serde_json::from_str::<Value>(&previous)?["equipment"].as_array() {
            for e in es {
                if let Some(id) = e["id"].as_str() {
                    if !ids.iter().any(|s| s == id) {
                        ids.push(id.into())
                    }
                }
            }
        }
    }
    for id in ids {
        if sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM equipment WHERE id=? AND building_id=?",
        )
        .bind(&id)
        .bind(&b)
        .fetch_one(&mut **tx)
        .await?
            != 1
        {
            continue;
        }
        let text = format!(
            "{} · {} : {}",
            w.get::<String, _>(1),
            w.get::<String, _>(2),
            if current.contains(&id) {
                "intervention liée mise à jour"
            } else {
                "lien avec l’intervention retiré"
            }
        );
        revision(tx, key, &id, actor, "work", &text, None, Some(work)).await?;
    }
    Ok(())
}
