use crate::{audit, auth, db, preferences, vault, works, Core};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sqlx::{Row, SqliteConnection};
fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn num(v: &Value, k: &str) -> i64 {
    v[k].as_i64().unwrap_or(0)
}
fn date(s: &str) -> Result<()> {
    let d = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")?;
    if d.to_string() != s {
        bail!("Date invalide")
    };
    Ok(())
}
fn reason(v: &Value) -> Result<&str> {
    let s = text(v, "reason");
    if s.trim().len() < 3 || s.len() > 1000 {
        bail!("Indiquez un commentaire de 3 à 1 000 caractères.")
    };
    Ok(s)
}
fn list(v: &Value, k: &str) -> Result<Vec<String>> {
    let a = v[k].as_array().context("Sélection manquante")?;
    if a.len() > 1000 {
        bail!("Sélection trop grande")
    };
    let mut out = vec![];
    for s in a {
        let s = s.as_str().context("Identifiant invalide")?;
        if out.iter().any(|a| a == s) {
            bail!("Sélection dupliquée")
        };
        out.push(s.to_string())
    }
    Ok(out)
}
pub async fn provider_access(c: &Core, u: &str, p: &str, write: bool) -> Result<bool> {
    let o = preferences::org(c, u).await?;
    if sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM entities WHERE id=? AND organisation_id=? AND kind='provider'",
    )
    .bind(p)
    .bind(o)
    .fetch_one(&c.pool)
    .await?
        != 1
    {
        return Ok(false);
    };
    if preferences::global(
        c,
        u,
        if write {
            "contracts.write"
        } else {
            "contracts.read"
        },
    )
    .await?
    {
        return Ok(true);
    };
    if write {
        return Ok(false);
    };
    for id in sqlx::query_scalar::<_, String>("SELECT id FROM contracts WHERE provider_id=?")
        .bind(p)
        .fetch_all(&c.pool)
        .await?
    {
        if permitted(c, u, &id, false).await? {
            return Ok(true);
        }
    }
    Ok(false)
}
pub(crate) async fn permitted(c: &Core, u: &str, id: &str, write: bool) -> Result<bool> {
    let Some(r)=sqlx::query("SELECT e.organisation_id,c.provider_id FROM contracts c JOIN entities e ON e.id=c.id WHERE c.id=?").bind(id).fetch_optional(&c.pool).await? else{return Ok(false)};
    if r.get::<String, _>(0) != preferences::org(c, u).await? {
        return Ok(false);
    };
    let buildings = sqlx::query_scalar::<_, String>(
        "SELECT building_id FROM contract_buildings WHERE contract_id=?",
    )
    .bind(id)
    .fetch_all(&c.pool)
    .await?;
    if buildings.is_empty() {
        return Ok(false);
    };
    let provider: String = r.get(1);
    for b in buildings {
        if works::permit(
            c,
            u,
            &b,
            if write {
                "contracts.write"
            } else {
                "contracts.read"
            },
        )
        .await?
        {
            continue;
        };
        if !write {
            let n:i64=sqlx::query_scalar("SELECT count(*) FROM provider_users pu JOIN role_grants g ON g.user_id=pu.user_id JOIN role_permissions p ON p.role_code=g.role_code WHERE pu.provider_id=? AND pu.user_id=? AND g.role_code='prestataire' AND p.permission_code='contracts.read' AND (g.building_id IS NULL OR g.building_id=?) AND (g.expires_at IS NULL OR g.expires_at>?)").bind(&provider).bind(u).bind(&b).bind(db::now()).fetch_one(&c.pool).await?;
            if n > 0 {
                continue;
            }
        }
        return Ok(false);
    }
    Ok(true)
}
pub async fn access(c: &Core, t: &str, id: &str, write: bool) -> Result<String> {
    let u = auth::require(c, t, None).await?;
    if !permitted(c, &u, id, write).await? {
        bail!("Contrat non accessible dans votre périmètre.")
    };
    Ok(u)
}
async fn raw(conn: &mut SqliteConnection, id: &str, provider: bool) -> Result<Value> {
    if provider {
        let r = sqlx::query(
            "SELECT p.*,e.archived_at FROM providers p JOIN entities e ON e.id=p.id WHERE p.id=?",
        )
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
        let creds=sqlx::query("SELECT c.*,f.original_name FROM provider_credentials c LEFT JOIN files f ON f.id=c.file_id WHERE provider_id=? ORDER BY expires_on,id").bind(id).fetch_all(&mut *conn).await?.iter().map(|c|json!({"id":c.get::<String,_>("id"),"kind":c.get::<String,_>("kind"),"title":c.get::<String,_>("title"),"issued_on":c.get::<Option<String>,_>("issued_on"),"expires_on":c.get::<String,_>("expires_on"),"active":c.get::<i64,_>("active")==1,"file_id":c.get::<Option<String>,_>("file_id"),"file_name":c.get::<Option<String>,_>("original_name")})).collect::<Vec<_>>();
        return Ok(
            json!({"id":id,"name":r.get::<String,_>("name"),"registration_number":r.get::<Option<String>,_>("registration_number"),"address":r.get::<Option<String>,_>("address"),"email":r.get::<Option<String>,_>("email"),"phone":r.get::<Option<String>,_>("phone"),"notes":r.get::<String,_>("notes"),"version":r.get::<i64,_>("revision"),"archived":r.get::<Option<String>,_>("archived_at").is_some(),"credentials":creds}),
        );
    }
    let r=sqlx::query("SELECT c.*,p.name AS provider,e.archived_at FROM contracts c JOIN providers p ON p.id=c.provider_id JOIN entities e ON e.id=c.id WHERE c.id=?").bind(id).fetch_one(&mut *conn).await?;
    let buildings=sqlx::query("SELECT b.id,b.name,e.archived_at FROM contract_buildings cb JOIN buildings b ON b.id=cb.building_id JOIN entities e ON e.id=b.id WHERE contract_id=? ORDER BY b.name").bind(id).fetch_all(&mut *conn).await?.iter().map(|r|json!({"id":r.get::<String,_>(0),"name":r.get::<String,_>(1),"archived":r.get::<Option<String>,_>(2).is_some()})).collect::<Vec<_>>();
    let equipment = sqlx::query_scalar::<_, String>(
        "SELECT equipment_id FROM contract_equipment WHERE contract_id=? ORDER BY equipment_id",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let visits=sqlx::query("SELECT * FROM contract_visits WHERE contract_id=? ORDER BY planned_at,id").bind(id).fetch_all(&mut *conn).await?.iter().map(|r|json!({"id":r.get::<String,_>("id"),"building_id":r.get::<String,_>("building_id"),"planned_at":r.get::<String,_>("planned_at"),"completed_at":r.get::<Option<String>,_>("completed_at"),"status":r.get::<String,_>("status"),"notes":r.get::<Option<String>,_>("notes")})).collect::<Vec<_>>();
    let files=sqlx::query("SELECT cf.*,f.original_name FROM contract_files cf JOIN files f ON f.id=cf.file_id WHERE contract_id=? ORDER BY cf.created_at").bind(id).fetch_all(&mut *conn).await?.iter().map(|r|json!({"id":r.get::<String,_>("file_id"),"title":r.get::<String,_>("title"),"name":r.get::<String,_>("original_name"),"visit_id":r.get::<Option<String>,_>("visit_id")})).collect::<Vec<_>>();
    Ok(
        json!({"id":id,"reference":r.get::<String,_>("reference"),"subject":r.get::<String,_>("subject"),"provider_id":r.get::<String,_>("provider_id"),"provider":r.get::<String,_>("provider"),"starts_on":r.get::<String,_>("starts_on"),"ends_on":r.get::<Option<String>,_>("ends_on"),"notice_days":r.get::<i64,_>("notice_days"),"cost_cents":r.get::<i64,_>("cost_cents"),"cost_period":r.get::<String,_>("cost_period"),"intervention_interval_months":r.get::<Option<i64>,_>("intervention_interval_months"),"intervention_interval_days":r.get::<Option<i64>,_>("intervention_interval_days"),"terms":r.get::<Option<String>,_>("terms"),"version":r.get::<i64,_>("revision"),"archived":r.get::<Option<String>,_>("archived_at").is_some(),"buildings":buildings,"equipment_ids":equipment,"visits":visits,"files":files}),
    )
}
async fn cost(c: &Core, u: &str, id: &str) -> Result<bool> {
    for b in sqlx::query_scalar::<_, String>(
        "SELECT building_id FROM contract_buildings WHERE contract_id=?",
    )
    .bind(id)
    .fetch_all(&c.pool)
    .await?
    {
        if !works::permit(c, u, &b, "contracts.cost.read").await? {
            return Ok(false);
        }
    }
    Ok(true)
}
fn redact(v: &mut Value, financial: bool) {
    v["cost_visible"] = json!(financial);
    if !financial {
        v["cost_cents"] = Value::Null;
        v["terms"] = Value::Null;
        v["files"] = json!([])
    }
}
pub async fn snapshot(c: &Core, t: &str) -> Result<Value> {
    let u = auth::require(c, t, None).await?;
    let org = preferences::org(c, &u).await?;
    let mut buildings = vec![];
    for r in sqlx::query("SELECT b.id,b.name FROM buildings b JOIN entities e ON e.id=b.id WHERE b.organisation_id=? AND e.archived_at IS NULL ORDER BY name").bind(&org).fetch_all(&c.pool).await?{let b:String=r.get(0);if works::permit(c,&u,&b,"contracts.read").await?{buildings.push(json!({"id":b,"name":r.get::<String,_>(1),"can_write":works::permit(c,&u,&b,"contracts.write").await?,"cost_visible":works::permit(c,&u,&b,"contracts.cost.read").await?}))}}
    let mut records = vec![];
    let mut providers = vec![];
    for id in sqlx::query_scalar::<_, String>("SELECT id FROM contracts ORDER BY reference")
        .fetch_all(&c.pool)
        .await?
    {
        if permitted(c, &u, &id, false).await? {
            let mut v = load(c, &id, false).await?;
            redact(&mut v, cost(c, &u, &id).await?);
            v["can_write"] = json!(permitted(c, &u, &id, true).await?);
            records.push(v)
        }
    }
    for id in sqlx::query_scalar::<_,String>("SELECT p.id FROM providers p JOIN entities e ON e.id=p.id WHERE e.organisation_id=? ORDER BY name").bind(&org).fetch_all(&c.pool).await?{if provider_access(c,&u,&id,false).await?{let mut p=load(c,&id,true).await?;p["can_write"]=json!(provider_access(c,&u,&id,true).await?);providers.push(p)}}
    let mut accounts = vec![];
    if preferences::global(c, &u, "contracts.write").await? {
        for r in sqlx::query("SELECT DISTINCT u.id,u.display_name FROM users u JOIN role_grants g ON g.user_id=u.id WHERE u.organisation_id=? AND u.active=1 AND g.role_code='prestataire' AND (g.expires_at IS NULL OR g.expires_at>?) ORDER BY u.display_name").bind(&org).bind(db::now()).fetch_all(&c.pool).await?{accounts.push(json!({"id":r.get::<String,_>(0),"name":r.get::<String,_>(1)}));}
        for p in &mut providers {
            let ids = sqlx::query_scalar::<_, String>(
                "SELECT user_id FROM provider_users WHERE provider_id=?",
            )
            .bind(p["id"].as_str().unwrap())
            .fetch_all(&c.pool)
            .await?;
            p["user_ids"] = json!(ids);
        }
    }
    let mut equipment = vec![];
    for b in &buildings {
        for r in sqlx::query("SELECT q.id,q.reference,q.designation FROM equipment q JOIN entities e ON e.id=q.id WHERE q.building_id=? AND e.archived_at IS NULL ORDER BY q.reference").bind(b["id"].as_str().unwrap()).fetch_all(&c.pool).await?{equipment.push(json!({"id":r.get::<String,_>(0),"reference":r.get::<String,_>(1),"name":r.get::<String,_>(2),"building_id":b["id"]}))}
    }
    Ok(
        json!({"records":records,"providers":providers,"accounts":accounts,"buildings":buildings,"equipment":equipment,"can_manage_providers":preferences::global(c,&u,"contracts.write").await?,"today":works::today()}),
    )
}
async fn revision(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    key: &[u8],
    id: &str,
    u: &str,
    provider: bool,
    why: &str,
) -> Result<()> {
    let table = if provider { "providers" } else { "contracts" };
    sqlx::query(&format!(
        "UPDATE {table} SET revision=revision+1 WHERE id=?"
    ))
    .bind(id)
    .execute(&mut **tx)
    .await?;
    let v = raw(&mut **tx, id, provider).await?;
    sqlx::query("INSERT INTO entity_versions(entity_id,version,payload_json,recorded_at,recorded_by,reason) VALUES(?,?,?,?,?,?)").bind(id).bind(v["version"].as_i64().unwrap()).bind(v.to_string()).bind(db::now()).bind(u).bind(why).execute(&mut **tx).await?;
    audit::append(
        tx,
        key,
        Some(u),
        if provider {
            "provider.changed"
        } else {
            "contract.changed"
        },
        json!({"id":id,"reason":why,"version":v["version"]}),
    )
    .await
}
async fn active(c: &Core, id: &str, provider: bool, version: i64) -> Result<()> {
    let table = if provider { "providers" } else { "contracts" };
    let r = sqlx::query(&format!(
        "SELECT x.revision,e.archived_at FROM {table} x JOIN entities e ON e.id=x.id WHERE x.id=?"
    ))
    .bind(id)
    .fetch_one(&c.pool)
    .await?;
    if r.get::<i64, _>(0) != version {
        bail!("La fiche a changé. Actualisez avant de recommencer.")
    };
    Ok(())
}
async fn deadlines(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>, id: &str) -> Result<()> {
    sqlx::query("UPDATE deadlines SET completed_at=? WHERE entity_id=? AND kind LIKE 'contract:%' AND completed_at IS NULL").bind(db::now()).bind(id).execute(&mut **tx).await?;
    let Some(r)=sqlx::query("SELECT c.reference,c.subject,c.ends_on,c.notice_days,e.archived_at,c.provider_id FROM contracts c JOIN entities e ON e.id=c.id WHERE c.id=?").bind(id).fetch_optional(&mut **tx).await? else{return Ok(())};
    if r.get::<Option<String>, _>(4).is_some() {
        return Ok(());
    };
    let bs = sqlx::query_scalar::<_, String>(
        "SELECT building_id FROM contract_buildings WHERE contract_id=?",
    )
    .bind(id)
    .fetch_all(&mut **tx)
    .await?;
    let mut dates = vec![];
    if let Some(end) = r.get::<Option<String>, _>(2) {
        dates.push((end.clone(), "Fin de contrat".to_string()));
        if r.get::<i64, _>(3) > 0 {
            let d = chrono::NaiveDate::parse_from_str(&end, "%Y-%m-%d")?
                .checked_sub_signed(chrono::Duration::days(r.get(3)))
                .context("Préavis hors limites")?;
            dates.push((d.to_string(), "Préavis à examiner".into()))
        }
    }
    for cred in sqlx::query(
        "SELECT title,expires_on,id FROM provider_credentials WHERE provider_id=? AND active=1",
    )
    .bind(r.get::<String, _>(5))
    .fetch_all(&mut **tx)
    .await?
    {
        dates.push((
            cred.get::<String, _>(1),
            format!("Justificatif {}", cred.get::<String, _>(0)),
        ))
    }
    for v in sqlx::query("SELECT planned_at,building_id FROM contract_visits WHERE contract_id=? AND status='planned'").bind(id).fetch_all(&mut **tx).await?{let b:String=v.get(1);let day:String=v.get(0);let kind=format!("contract:Passage · {} · {}|scope:{}",r.get::<String,_>(0),day,b);sqlx::query("INSERT INTO deadlines(id,building_id,entity_id,kind,due_on) VALUES(?,?,?,?,?) ON CONFLICT(entity_id,kind,due_on) DO UPDATE SET completed_at=NULL").bind(db::id()).bind(b).bind(id).bind(kind).bind(day).execute(&mut **tx).await?;}
    for b in bs {
        for (d, label) in &dates {
            sqlx::query("INSERT INTO deadlines(id,building_id,entity_id,kind,due_on) VALUES(?,?,?,?,?) ON CONFLICT(entity_id,kind,due_on) DO UPDATE SET completed_at=NULL").bind(db::id()).bind(&b).bind(id).bind(format!("contract:{label} · {}|scope:{}",r.get::<String,_>(0),b)).bind(d).execute(&mut **tx).await?;
        }
    }
    Ok(())
}
pub async fn save(c: &Core, t: &str, provider: bool, v: Value) -> Result<String> {
    let why = reason(&v)?;
    if text(&v, "notes").len() > 10000 {
        bail!("Compte rendu limité à 10 000 caractères.")
    }
    let u = auth::require(c, t, None).await?;
    let org = preferences::org(c, &u).await?;
    let old = v["id"].as_str();
    let id = old.map(str::to_owned).unwrap_or_else(db::id);
    if provider {
        if !preferences::global(c, &u, "contracts.write").await? {
            bail!("Le répertoire des prestataires est géré par un responsable au périmètre global.")
        };
        if old.is_some() {
            if !provider_access(c, &u, &id, true).await? {
                bail!("Prestataire inaccessible")
            };
            active(c, &id, true, num(&v, "base_version")).await?;
        }
    } else if old.is_some() {
        access(c, t, &id, true).await?;
        active(c, &id, false, num(&v, "base_version")).await?;
    }
    if old.is_some()
        && sqlx::query_scalar::<_, Option<String>>("SELECT archived_at FROM entities WHERE id=?")
            .bind(&id)
            .fetch_one(&c.pool)
            .await?
            .is_some()
    {
        bail!("Réactivez la fiche avant de la modifier.")
    }
    let mut bs = vec![];
    let mut eq = vec![];
    if provider {
        for k in ["name", "registration_number", "phone", "email"] {
            if text(&v, k).len() > 250 {
                bail!("Champ trop long")
            }
        }
        if text(&v, "name").trim().is_empty()
            || text(&v, "address").len() > 2000
            || text(&v, "notes").len() > 10000
        {
            bail!("Vérifiez la fiche prestataire")
        }
    } else {
        bs = list(&v, "building_ids")?;
        eq = list(&v, "equipment_ids")?;
        if bs.is_empty() {
            bail!("Sélectionnez un bâtiment")
        };
        for b in &bs {
            if !works::permit(c, &u, b, "contracts.write").await? {
                bail!("Bâtiment inaccessible")
            };
            let active: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM entities WHERE id=? AND archived_at IS NULL",
            )
            .bind(b)
            .fetch_one(&c.pool)
            .await?;
            if active != 1 {
                bail!("Bâtiment archivé")
            }
        }
        let p = text(&v, "provider_id");
        if !provider_access(c, &u, p, false).await? {
            bail!("Prestataire inaccessible")
        };
        if sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM entities WHERE id=? AND archived_at IS NULL",
        )
        .bind(p)
        .fetch_one(&c.pool)
        .await?
            != 1
        {
            bail!("Prestataire archivé")
        };
        date(text(&v, "starts_on"))?;
        if !text(&v, "ends_on").is_empty() {
            date(text(&v, "ends_on"))?;
            if text(&v, "ends_on") < text(&v, "starts_on") {
                bail!("Fin antérieure au début")
            }
        }
        if text(&v, "subject").trim().is_empty()
            || text(&v, "subject").len() > 250
            || text(&v, "terms").len() > 20000
            || !(0..=3650).contains(&num(&v, "notice_days"))
            || !(0..=1_000_000_000_000).contains(&num(&v, "cost_cents"))
            || !["one_off", "monthly", "annual", "per_visit"].contains(&text(&v, "cost_period"))
        {
            bail!("Vérifiez l’objet, le montant, la périodicité et le préavis")
        };
        for (k, max) in [
            ("intervention_interval_months", 120),
            ("intervention_interval_days", 3650),
        ] {
            if v[k].as_i64().is_some_and(|n| n < 1 || n > max) {
                bail!("Fréquence invalide")
            }
        }
        if v["intervention_interval_months"].as_i64().is_some()
            && v["intervention_interval_days"].as_i64().is_some()
        {
            bail!("Choisissez jours ou mois")
        };
        for e in &eq {
            let b: String = sqlx::query_scalar("SELECT building_id FROM equipment WHERE id=?")
                .bind(e)
                .fetch_one(&c.pool)
                .await?;
            if !bs.contains(&b) {
                bail!("Équipement extérieur aux bâtiments du contrat")
            }
        }
        if old.is_some() {
            let previous=sqlx::query_scalar::<_,String>("SELECT building_id FROM contract_visits WHERE contract_id=? UNION SELECT building_id FROM contract_equipment WHERE contract_id=?").bind(&id).bind(&id).fetch_all(&c.pool).await?;
            if previous.iter().any(|b| !bs.contains(b)) {
                bail!("Conservez les bâtiments ayant des passages ou équipements historiques.")
            }
        }
    }
    let users = if provider && v["user_ids"].is_array() {
        Some(list(&v, "user_ids")?)
    } else {
        None
    };
    if let Some(users) = &users {
        for user in users {
            let n:i64=sqlx::query_scalar("SELECT count(*) FROM users u JOIN role_grants g ON g.user_id=u.id WHERE u.id=? AND u.organisation_id=? AND u.active=1 AND g.role_code='prestataire' AND (g.expires_at IS NULL OR g.expires_at>?)").bind(user).bind(&org).bind(db::now()).fetch_one(&c.pool).await?;
            if n == 0 {
                bail!("Choisissez un compte prestataire actif de cette organisation.")
            }
        }
    }
    let mut tx = c.pool.begin().await?;
    if old.is_none() {
        sqlx::query(
            "INSERT INTO entities(id,organisation_id,kind,created_at,created_by) VALUES(?,?,?,?,?)",
        )
        .bind(&id)
        .bind(&org)
        .bind(if provider { "provider" } else { "contract" })
        .bind(db::now())
        .bind(&u)
        .execute(&mut *tx)
        .await?;
        if provider {
            sqlx::query("INSERT INTO providers(id,name) VALUES(?,?)")
                .bind(&id)
                .bind(text(&v, "name"))
                .execute(&mut *tx)
                .await?;
        } else {
            sqlx::query("INSERT INTO contracts(id,provider_id,reference,subject,starts_on,cost_cents,cost_period) VALUES(?,?,?,?,?,0,'annual')").bind(&id).bind(text(&v,"provider_id")).bind(format!("CTR-{}",&id[..8].to_uppercase())).bind(text(&v,"subject")).bind(text(&v,"starts_on")).execute(&mut *tx).await?;
        }
    }
    if provider {
        sqlx::query("UPDATE providers SET name=?,registration_number=?,address=?,email=?,phone=?,notes=? WHERE id=?").bind(text(&v,"name").trim()).bind(text(&v,"registration_number")).bind(text(&v,"address")).bind(text(&v,"email")).bind(text(&v,"phone")).bind(text(&v,"notes")).bind(&id).execute(&mut *tx).await?;
    } else {
        sqlx::query("UPDATE contracts SET intervention_interval_days=NULL WHERE id=?")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE contracts SET provider_id=?,subject=?,starts_on=?,ends_on=?,notice_days=?,cost_cents=?,cost_period=?,intervention_interval_months=?,intervention_interval_days=?,terms=? WHERE id=?").bind(text(&v,"provider_id")).bind(text(&v,"subject").trim()).bind(text(&v,"starts_on")).bind(v["ends_on"].as_str().filter(|s|!s.is_empty())).bind(num(&v,"notice_days")).bind(num(&v,"cost_cents")).bind(text(&v,"cost_period")).bind(v["intervention_interval_months"].as_i64()).bind(v["intervention_interval_days"].as_i64()).bind(text(&v,"terms")).bind(&id).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM contract_equipment WHERE contract_id=?")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        let prior = sqlx::query_scalar::<_, String>(
            "SELECT building_id FROM contract_buildings WHERE contract_id=?",
        )
        .bind(&id)
        .fetch_all(&mut *tx)
        .await?;
        for b in prior {
            if !bs.contains(&b) {
                sqlx::query("DELETE FROM contract_buildings WHERE contract_id=? AND building_id=?")
                    .bind(&id)
                    .bind(b)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        for b in &bs {
            sqlx::query("INSERT OR IGNORE INTO contract_buildings VALUES(?,?)")
                .bind(&id)
                .bind(b)
                .execute(&mut *tx)
                .await?;
        }
        for e in eq {
            sqlx::query("INSERT INTO contract_equipment VALUES(?,?,(SELECT building_id FROM equipment WHERE id=?))").bind(&id).bind(&e).bind(&e).execute(&mut *tx).await?;
        }
        deadlines(&mut tx, &id).await?;
    }
    if let Some(users) = users {
        audit::append(
            &mut tx,
            &c.key,
            Some(&u),
            "provider.accounts",
            json!({"provider":id,"accounts":users}),
        )
        .await?;
        sqlx::query("DELETE FROM provider_users WHERE provider_id=?")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        for user in users {
            sqlx::query("INSERT INTO provider_users VALUES(?,?)")
                .bind(&id)
                .bind(user)
                .execute(&mut *tx)
                .await?;
        }
    }
    revision(&mut tx, &c.key, &id, &u, provider, why).await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await?;
    Ok(id)
}
pub async fn action(
    c: &Core,
    t: &str,
    id: &str,
    provider: bool,
    version: i64,
    op: &str,
    v: Value,
) -> Result<()> {
    let why = reason(&v)?;
    if text(&v, "notes").len() > 10000 {
        bail!("Compte rendu limité à 10 000 caractères.")
    }
    let u = auth::require(c, t, None).await?;
    if provider {
        if !provider_access(c, &u, id, true).await? {
            bail!("Prestataire non modifiable")
        }
    } else {
        access(c, t, id, true).await?;
    }
    active(c, id, provider, version).await?;
    let archived: Option<String> =
        sqlx::query_scalar("SELECT archived_at FROM entities WHERE id=?")
            .bind(id)
            .fetch_one(&c.pool)
            .await?;
    if archived.is_some() && op != "archive" {
        bail!("Réactivez d’abord la fiche")
    };
    let mut tx = c.pool.begin().await?;
    match op {
        "archive" => {
            sqlx::query("UPDATE entities SET archived_at=? WHERE id=?")
                .bind(if v["archived"] == true {
                    Some(db::now())
                } else {
                    None
                })
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        "credential" if provider => {
            date(text(&v, "expires_on"))?;
            let issued = v["issued_on"].as_str().filter(|s| !s.is_empty());
            if let Some(d) = issued {
                date(d)?;
                if d > works::today().as_str() || d > text(&v, "expires_on") {
                    bail!("Dates du justificatif incohérentes")
                }
            }
            if !["insurance", "qualification", "authorisation"].contains(&text(&v, "kind"))
                || text(&v, "title").is_empty()
                || text(&v, "title").len() > 250
            {
                bail!("Justificatif invalide")
            };
            sqlx::query("INSERT INTO provider_credentials(id,provider_id,kind,title,issued_on,expires_on) VALUES(?,?,?,?,?,?)").bind(db::id()).bind(id).bind(text(&v,"kind")).bind(text(&v,"title")).bind(issued).bind(text(&v,"expires_on")).execute(&mut *tx).await?;
        }
        "credential_cancel" if provider => {
            if sqlx::query("UPDATE provider_credentials SET active=0 WHERE id=? AND provider_id=? AND active=1").bind(text(&v,"credential")).bind(id).execute(&mut *tx).await?.rows_affected()!=1{bail!("Justificatif absent")}
        }
        "visit_plan" if !provider => {
            let building = text(&v, "building_id");
            if sqlx::query_scalar::<_,i64>("SELECT count(*) FROM contract_buildings cb JOIN entities e ON e.id=cb.building_id WHERE cb.contract_id=? AND cb.building_id=? AND e.archived_at IS NULL").bind(id).bind(building).fetch_one(&mut *tx).await?!=1{bail!("Bâtiment non couvert ou archivé")};
            let start = chrono::NaiveDate::parse_from_str(text(&v, "day"), "%Y-%m-%d")?;
            let count = num(&v, "count");
            if !(1..=60).contains(&count) {
                bail!("Planifiez de 1 à 60 passages")
            };
            let r=sqlx::query("SELECT starts_on,ends_on,intervention_interval_months,intervention_interval_days FROM contracts WHERE id=?").bind(id).fetch_one(&mut *tx).await?;
            let months: Option<i64> = r.get(2);
            let days: Option<i64> = r.get(3);
            if count > 1 && months.is_none() && days.is_none() {
                bail!("Renseignez la fréquence avant de générer plusieurs passages")
            };
            for i in 0..count {
                let d = if let Some(m) = months {
                    start.checked_add_months(chrono::Months::new((m * i).try_into()?))
                } else {
                    start.checked_add_signed(chrono::Duration::days(days.unwrap_or(0) * i))
                }
                .context("Date hors limites")?
                .to_string();
                if d < r.get::<String, _>(0) || r.get::<Option<String>, _>(1).is_some_and(|e| d > e)
                {
                    bail!("Un passage sort des dates du contrat. Aucun passage ajouté.")
                };
                if sqlx::query_scalar::<_,i64>("SELECT count(*) FROM contract_visits WHERE contract_id=? AND building_id=? AND planned_at=? AND status<>'cancelled'").bind(id).bind(building).bind(&d).fetch_one(&mut *tx).await?>0{bail!("Un passage existe déjà à cette date. Aucun doublon ajouté.")};
                sqlx::query("INSERT INTO contract_visits(id,contract_id,building_id,planned_at,status,notes) VALUES(?,?,?,?,'planned',?)").bind(db::id()).bind(id).bind(building).bind(d).bind(text(&v,"notes")).execute(&mut *tx).await?;
            }
        }
        "visit_status" if !provider => {
            let vid = text(&v, "visit_id");
            let status = text(&v, "status");
            if !["completed", "cancelled", "missed"].contains(&status) {
                bail!("État du passage invalide")
            };
            let r = sqlx::query(
                "SELECT status,planned_at FROM contract_visits WHERE id=? AND contract_id=?",
            )
            .bind(vid)
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            if r.get::<String, _>(0) != "planned" {
                bail!("Le passage est déjà traité")
            };
            let done = if status == "completed" {
                date(text(&v, "completed_at"))?;
                if text(&v, "completed_at") > works::today().as_str() {
                    bail!("La réalisation ne peut être future")
                };
                Some(text(&v, "completed_at"))
            } else {
                None
            };
            if status == "missed" && r.get::<String, _>(1) > works::today() {
                bail!("Le passage n’est pas encore échu")
            };
            sqlx::query("UPDATE contract_visits SET status=?,completed_at=?,notes=?,revision=revision+1 WHERE id=?").bind(status).bind(done).bind(text(&v,"notes")).bind(vid).execute(&mut *tx).await?;
        }
        _ => bail!("Action non disponible"),
    };
    if !provider {
        deadlines(&mut tx, id).await?
    } else {
        for cid in sqlx::query_scalar::<_, String>("SELECT id FROM contracts WHERE provider_id=?")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?
        {
            deadlines(&mut tx, &cid).await?
        }
    };
    revision(&mut tx, &c.key, id, &u, provider, why).await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}
pub async fn history(c: &Core, t: &str, id: &str, provider: bool) -> Result<Value> {
    let u = auth::require(c, t, None).await?;
    if provider {
        if !provider_access(c, &u, id, false).await? {
            bail!("Prestataire inaccessible")
        }
    } else {
        access(c, t, id, false).await?;
    };
    let mut out = vec![];
    for r in sqlx::query("SELECT v.*,u.display_name FROM entity_versions v JOIN users u ON u.id=v.recorded_by WHERE entity_id=? ORDER BY version DESC LIMIT 100").bind(id).fetch_all(&c.pool).await?{let mut payload:Value=serde_json::from_str(&r.get::<String,_>("payload_json"))?;if !provider{redact(&mut payload,cost(c,&u,id).await?)}out.push(json!({"version":r.get::<i64,_>("version"),"date":r.get::<String,_>("recorded_at"),"actor":r.get::<String,_>("display_name"),"reason":r.get::<String,_>("reason"),"payload":payload}))}
    Ok(json!(out))
}
pub async fn attach(
    c: &Core,
    t: &str,
    id: &str,
    provider: bool,
    version: i64,
    target: Option<&str>,
    p: &std::path::Path,
) -> Result<()> {
    let u = auth::require(c, t, None).await?;
    if provider {
        if !provider_access(c, &u, id, true).await? {
            bail!("Justificatif non modifiable")
        }
    } else {
        access(c, t, id, true).await?;
    };
    active(c, id, provider, version).await?;
    if sqlx::query_scalar::<_, Option<String>>("SELECT archived_at FROM entities WHERE id=?")
        .bind(id)
        .fetch_one(&c.pool)
        .await?
        .is_some()
    {
        bail!("Réactivez la fiche avant de joindre un document.")
    }
    let org = preferences::org(c, &u).await?;
    if std::fs::metadata(p)?.len() > 20 * 1024 * 1024 {
        bail!("Fichier limité à 20 Mio")
    };
    let bytes = std::fs::read(p)?;
    let mime = if bytes.starts_with(b"%PDF-") {
        "application/pdf"
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[255, 216, 255]) {
        "image/jpeg"
    } else {
        bail!("Choisissez un PDF, PNG ou JPEG")
    };
    if let Some(target) = target {
        let n: i64 = sqlx::query_scalar(if provider {
            "SELECT count(*) FROM provider_credentials WHERE id=? AND provider_id=?"
        } else {
            "SELECT count(*) FROM contract_visits WHERE id=? AND contract_id=?"
        })
        .bind(target)
        .bind(id)
        .fetch_one(&c.pool)
        .await?;
        if n != 1 {
            bail!("Lien de pièce jointe invalide")
        }
    } else if provider {
        bail!("Choisissez le justificatif à joindre")
    };
    let fid = vault::put(
        c,
        &org,
        &u,
        p.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("document.pdf"),
        mime,
        &bytes,
    )
    .await?;
    let mut tx = c.pool.begin().await?;
    if provider {
        sqlx::query("UPDATE provider_credentials SET file_id=? WHERE id=? AND provider_id=?")
            .bind(&fid)
            .bind(target)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query("INSERT INTO contract_files VALUES(?,?,?,?,?)")
            .bind(id)
            .bind(&fid)
            .bind(target)
            .bind(if target.is_some() {
                "Compte rendu"
            } else {
                "Pièce du contrat"
            })
            .bind(db::now())
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("INSERT INTO entity_files(entity_id,file_id,organisation_id,purpose) VALUES(?,?,?,'contract')")
        .bind(id)
        .bind(fid)
        .bind(&org)
        .execute(&mut *tx)
        .await?;
    revision(&mut tx, &c.key, id, &u, provider, "Pièce jointe ajoutée").await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}
pub async fn file(
    c: &Core,
    t: &str,
    id: &str,
    provider: bool,
    f: &str,
) -> Result<(String, Vec<u8>)> {
    let u = auth::require(c, t, None).await?;
    if provider {
        if !provider_access(c, &u, id, false).await? {
            bail!("Prestataire inaccessible")
        }
    } else {
        access(c, t, id, false).await?;
        if !cost(c, &u, id).await? {
            bail!("Les pièces contractuelles peuvent contenir des montants ; accès restreint.")
        }
    };
    if sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM entity_files WHERE entity_id=? AND file_id=?",
    )
    .bind(id)
    .bind(f)
    .fetch_one(&c.pool)
    .await?
        != 1
    {
        bail!("Fichier non lié")
    };
    let org = preferences::org(c, &u).await?;
    let name: String =
        sqlx::query_scalar("SELECT original_name FROM files WHERE id=? AND organisation_id=?")
            .bind(f)
            .bind(&org)
            .fetch_one(&c.pool)
            .await?;
    Ok((name, vault::read(c, f, &org).await?.to_vec()))
}

async fn load(c: &Core, id: &str, provider: bool) -> Result<Value> {
    let mut conn = c.pool.acquire().await?;
    raw(&mut conn, id, provider).await
}
