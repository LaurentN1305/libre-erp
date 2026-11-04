//! Évaluation locale déterministe, sans réseau et sans texte sensible dans les toasts.
use crate::{audit, auth, db, preferences, works, Core};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use std::collections::{HashMap, HashSet};
fn txt<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn reason(v: &Value) -> Result<&str> {
    let s = txt(v, "reason");
    if s.trim().chars().count() < 3 || s.len() > 2000 {
        bail!("Indiquez un commentaire de 3 à 2 000 caractères.")
    };
    Ok(s)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub desktop: bool,
    pub scheduled: bool,
    pub time: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            desktop: true,
            scheduled: false,
            time: "08:00".into(),
        }
    }
}
pub async fn settings(c: &Core, org: &str) -> Result<Settings> {
    let v = sqlx::query_scalar::<_, String>("SELECT value_json FROM settings WHERE key=?")
        .bind(format!("alerts.settings:{org}"))
        .fetch_optional(&c.pool)
        .await?;
    Ok(v.map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_default())
}
#[derive(Clone)]
struct Rule {
    id: String,
    org: String,
    building: Option<String>,
    family: String,
    enabled: bool,
    severity: i64,
    delay: i64,
    days: Vec<i64>,
}
async fn rules(c: &Core) -> Result<Vec<Rule>> {
    let mut out = vec![];
    for r in
        sqlx::query("SELECT * FROM alert_rules WHERE managed=1 AND retired_at IS NULL ORDER BY id")
            .fetch_all(&c.pool)
            .await?
    {
        let id: String = r.get("id");
        let days=sqlx::query_scalar("SELECT days_before FROM alert_rule_steps WHERE rule_id=? AND escalation_level=0 ORDER BY days_before").bind(&id).fetch_all(&c.pool).await?;
        out.push(Rule {
            id,
            org: r.get("organisation_id"),
            building: r.get("building_id"),
            family: r.get("deadline_kind"),
            enabled: r.get::<i64, _>("enabled") == 1,
            severity: r.get("severity"),
            delay: r.get("escalation_days"),
            days,
        })
    }
    Ok(out)
}
async fn event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    actor: Option<&str>,
    at: &str,
    kind: &str,
    note: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO alert_events VALUES(?,?,?,?,?,?)")
        .bind(db::id())
        .bind(id)
        .bind(actor)
        .bind(at)
        .bind(kind)
        .bind(note)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
async fn seed(c: &Core) -> Result<()> {
    let orgs =
        sqlx::query_scalar::<_, String>("SELECT id FROM organisations WHERE archived_at IS NULL")
            .fetch_all(&c.pool)
            .await?;
    for org in orgs {
        if sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM alert_rules WHERE organisation_id=? AND managed=1",
        )
        .bind(&org)
        .fetch_one(&c.pool)
        .await?
            == 0
        {
            let mut tx = c.pool.begin().await?;
            let id = db::id();
            sqlx::query("INSERT INTO alert_rules(id,organisation_id,deadline_kind,enabled,severity,managed,name,created_at) VALUES(?,?,'*',1,1,1,'Rappels par défaut',?)").bind(&id).bind(&org).bind(db::now()).execute(&mut *tx).await?;
            for day in [0, 7, 30, 60, 90] {
                for (level, role) in [(0, "technique"), (1, "securite"), (2, "exploitant")] {
                    sqlx::query("INSERT INTO alert_rule_steps VALUES(?,?,?,?)")
                        .bind(&id)
                        .bind(day)
                        .bind(level)
                        .bind(role)
                        .execute(&mut *tx)
                        .await?;
                }
            }
            audit::append(
                &mut tx,
                &c.key,
                None,
                "alerts.rules.default",
                json!({"rule":id,"organisation":org}),
            )
            .await?;
            tx.commit().await?;
            audit::checkpoint(&c.pool, &c.root).await?;
        }
    }
    Ok(())
}
fn family(kind: &str, manual: bool) -> &str {
    if manual {
        "manual"
    } else {
        match kind {
            "work" => "work",
            "equipment" => "equipment",
            "contract" => "contract",
            "document" => "document",
            _ => "register",
        }
    }
}
fn choose<'a>(all: &'a [Rule], org: &str, b: &str, f: &str) -> Option<&'a Rule> {
    all.iter()
        .filter(|r| {
            r.org == org
                && (r.building.is_none() || r.building.as_deref() == Some(b))
                && (r.family == "*" || r.family == f)
        })
        .max_by_key(|r| i32::from(r.building.is_some()) * 2 + i32::from(r.family != "*"))
}
fn stage(days: &[i64], remaining: i64) -> Option<i64> {
    days.iter().copied().find(|d| remaining <= *d)
}
fn severity(day: i64, level: i64, min: i64) -> i64 {
    let base = if day == 0 {
        4
    } else if day < 30 {
        3
    } else if day < 60 {
        2
    } else {
        1
    };
    (base.max(min) + level).min(4)
}
/// Les droits sont recontrôlés pour l'affichage ET pour chaque livraison de notification.
pub(crate) async fn visible(c: &Core, u: &str, deadline: &str, permission: &str) -> Result<bool> {
    let Some(r)=sqlx::query("SELECT d.building_id,d.entity_id,e.kind,e.organisation_id,m.restricted AS manual_restricted FROM deadlines d JOIN entities e ON e.id=d.entity_id LEFT JOIN manual_deadlines m ON m.deadline_id=d.id WHERE d.id=?").bind(deadline).fetch_optional(&c.pool).await? else{return Ok(false)};
    let Some(b) = r.get::<Option<String>, _>(0) else {
        return Ok(false);
    };
    if r.get::<String, _>(3) != preferences::org(c, u).await?
        || !works::permit(c, u, &b, "alerts.read").await?
        || !works::permit(c, u, &b, permission).await?
    {
        return Ok(false);
    };
    if r.get::<Option<i64>, _>(4) == Some(1)
        && !works::permit(c, u, &b, "documents.financial.read").await?
    {
        return Ok(false);
    };
    let id: String = r.get(1);
    Ok(match r.get::<String, _>(2).as_str() {
        "work" => works::permit(c, u, &b, "work.read").await?,
        "equipment" => works::permit(c, u, &b, "inventory.read").await?,
        "contract" => crate::contracts::permitted(c, u, &id, false).await?,
        "document" => document_visible(c, u, &id).await?,
        "building" => works::permit(c, u, &b, "buildings.read").await?,
        _ => works::permit(c, u, &b, "register.read").await?,
    })
}
async fn document_visible(c: &Core, u: &str, id: &str) -> Result<bool> {
    let r=sqlx::query("SELECT e.building_id,d.restricted FROM documents d JOIN entities e ON e.id=d.id WHERE d.id=?").bind(id).fetch_one(&c.pool).await?;
    let b: String = r.get(0);
    if !works::permit(c, u, &b, "documents.read").await?
        || r.get::<i64, _>(1) == 1 && !works::permit(c, u, &b, "documents.financial.read").await?
    {
        return Ok(false);
    };
    for r in sqlx::query("SELECT e.id,e.kind FROM document_links l JOIN entities e ON e.id=l.entity_id WHERE l.document_id=?").bind(id).fetch_all(&c.pool).await?{let id:String=r.get(0);let ok=match r.get::<String,_>(1).as_str(){"contract"=>crate::contracts::permitted(c,u,&id,false).await?,"provider"=>crate::contracts::provider_access(c,u,&id,false).await?,"work"=>works::permit(c,u,&b,"work.read").await?,"equipment"=>works::permit(c,u,&b,"inventory.read").await?,"register_entry"=>works::permit(c,u,&b,"register.read").await?,_=>true};if !ok{return Ok(false)}}
    Ok(true)
}
pub async fn evaluate(c: &Core) -> Result<usize> {
    if !c.sharing.can_write() { return Ok(0); }
    let n = evaluate_at(c, Utc::now()).await?;
    sqlx::query("INSERT INTO settings(key,value_json,modified_at) VALUES('alerts.last_checked',?,?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,modified_at=excluded.modified_at").bind(json!(db::now()).to_string()).bind(db::now()).execute(&c.pool).await?;
    sqlx::query("DELETE FROM settings WHERE key='alerts.last_error'")
        .execute(&c.pool)
        .await?;
    Ok(n)
}
pub(crate) async fn evaluate_at(c: &Core, moment: DateTime<Utc>) -> Result<usize> {
    seed(c).await?;
    let all = rules(c).await?;
    let day = moment.with_timezone(&chrono_tz::Europe::Paris).date_naive();
    let at = moment.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let ds=sqlx::query("SELECT d.*,e.organisation_id,e.kind AS entity_kind,e.archived_at AS entity_archived,b.archived_at AS building_archived,m.deadline_id AS manual FROM deadlines d JOIN entities e ON e.id=d.entity_id JOIN entities b ON b.id=d.building_id LEFT JOIN manual_deadlines m ON m.deadline_id=d.id").fetch_all(&c.pool).await?;
    let prior = sqlx::query("SELECT * FROM alerts WHERE managed=1")
        .fetch_all(&c.pool)
        .await?;
    let mut by: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, a) in prior.iter().enumerate() {
        by.entry(a.get("deadline_id")).or_default().push(i)
    }
    let mut desired = HashSet::new();
    let mut changed = 0;
    let mut reset = vec![];
    let mut tx = c.pool.begin().await?;
    for d in ds {
        if d.get::<Option<String>, _>("completed_at").is_some()
            || d.get::<Option<String>, _>("entity_archived").is_some()
            || d.get::<Option<String>, _>("building_archived").is_some()
        {
            continue;
        };
        let due: String = d.get("due_on");
        let parsed_due = NaiveDate::parse_from_str(&due, "%Y-%m-%d");
        let id: String = d.get("id");
        let b: String = d.get("building_id");
        let org: String = d.get("organisation_id");
        let k: String = d.get("entity_kind");
        let Some(rule) = choose(
            &all,
            &org,
            &b,
            family(&k, d.get::<Option<String>, _>("manual").is_some()),
        ) else {
            continue;
        };
        if !rule.enabled {
            continue;
        };
        let Ok(due) = parsed_due else {
            // Une date source invalide ne doit pas clore un rappel déjà ouvert.
            for a in prior.iter().filter(|a| a.get::<String, _>("deadline_id") == id
                && a.get::<String, _>("rule_id") == rule.id
                && a.get::<Option<String>, _>("resolved_at").is_none()) {
                desired.insert(a.get::<String, _>("id"));
            }
            continue;
        };
        let Some(step) = stage(&rule.days, (due - day).num_days()) else {
            continue;
        };
        let group: Vec<_> = by
            .get(&id)
            .into_iter()
            .flatten()
            .map(|i| &prior[*i])
            .filter(|a| {
                a.get::<String, _>("rule_id") == rule.id && a.get::<i64, _>("days_before") == step
            })
            .collect();
        let active = group
            .iter()
            .filter(|a| a.get::<Option<String>, _>("resolved_at").is_none())
            .max_by_key(|a| a.get::<i64, _>("escalation_level"));
        let level = if let Some(active) = active {
            let current = active.get::<i64, _>("escalation_level");
            if active.get::<Option<String>, _>("acknowledged_at").is_some() {
                current
            } else {
                let base = group
                    .iter()
                    .find(|a| a.get::<i64, _>("escalation_level") == 0)
                    .copied()
                    .unwrap_or(active);
                let start = base
                    .get::<Option<String>, _>("opened_at")
                    .unwrap_or(base.get("created_at"));
                let start = DateTime::parse_from_rfc3339(&start)?.with_timezone(&Utc);
                current.max(((moment - start).num_days().max(0) / rule.delay).min(2))
            }
        } else {
            let previous = by
                .get(&id)
                .into_iter()
                .flatten()
                .map(|i| &prior[*i])
                .filter(|a| {
                    a.get::<String, _>("rule_id") == rule.id
                        && a.get::<Option<String>, _>("resolved_at").is_none()
                        && a.get::<Option<String>, _>("acknowledged_at").is_none()
                })
                .max_by_key(|a| a.get::<i64, _>("escalation_level"));
            if let Some(previous) = previous {
                let previous_step = previous.get::<i64, _>("days_before");
                let base = by
                    .get(&id)
                    .into_iter()
                    .flatten()
                    .map(|i| &prior[*i])
                    .find(|a| {
                        a.get::<String, _>("rule_id") == rule.id
                            && a.get::<i64, _>("days_before") == previous_step
                            && a.get::<i64, _>("escalation_level") == 0
                    })
                    .unwrap_or(previous);
                let start = base
                    .get::<Option<String>, _>("opened_at")
                    .unwrap_or(base.get("created_at"));
                let start = DateTime::parse_from_rfc3339(&start)?.with_timezone(&Utc);
                previous
                    .get::<i64, _>("escalation_level")
                    .max(((moment - start).num_days().max(0) / rule.delay).min(2))
            } else {
                0
            }
        };
        let sev = severity(step, level, rule.severity);
        let existing = group
            .iter()
            .find(|a| a.get::<i64, _>("escalation_level") == level);
        let aid = existing
            .map(|a| a.get::<String, _>("id"))
            .unwrap_or_else(db::id);
        if let Some(a) = existing {
            if a.get::<Option<String>, _>("resolved_at").is_some() {
                sqlx::query("UPDATE alerts SET resolved_at=NULL,acknowledged_at=NULL,acknowledged_by=NULL,opened_at=?,severity=?,resolution_kind=NULL WHERE id=?").bind(&at).bind(sev).bind(&aid).execute(&mut *tx).await?;
                event(
                    &mut tx,
                    &aid,
                    None,
                    &at,
                    "reopened",
                    "Rappel réactivé après modification ou réouverture de l’échéance",
                )
                .await?;
                reset.push(aid.clone());
                changed += 1
            }
        } else {
            sqlx::query("INSERT INTO alerts(id,deadline_id,rule_id,days_before,escalation_level,created_at,managed,severity,opened_at) VALUES(?,?,?,?,?,?,1,?,?)").bind(&aid).bind(&id).bind(&rule.id).bind(step).bind(level).bind(&at).bind(sev).bind(&at).execute(&mut *tx).await?;
            event(
                &mut tx,
                &aid,
                None,
                &at,
                if level == 0 { "created" } else { "escalated" },
                if level == 0 {
                    "Rappel créé au palier courant"
                } else {
                    "Escalade automatique : rappel non pris en charge"
                },
            )
            .await?;
            changed += 1
        }
        desired.insert(aid);
    }
    for a in &prior {
        let id: String = a.get("id");
        if a.get::<Option<String>, _>("resolved_at").is_none() && !desired.contains(&id) {
            sqlx::query(
                "UPDATE alerts SET resolved_at=?,resolution_kind='source_or_rule' WHERE id=?",
            )
            .bind(&at)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
            event(&mut tx,&id,None,&at,"closed","Rappel clôturé : échéance terminée/archivée, règle désactivée/remplacée ou nouveau palier").await?;
            changed += 1
        }
    }
    for id in reset {
        sqlx::query("UPDATE notifications SET state='pending',attempts=0,next_attempt_at=NULL,delivered_at=NULL,last_error=NULL WHERE alert_id=?").bind(id).execute(&mut *tx).await?;
    }
    if changed > 0 {
        audit::append(
            &mut tx,
            &c.key,
            None,
            "alerts.evaluated",
            json!({"changes":changed,"date":day.to_string()}),
        )
        .await?;
    }
    tx.commit().await?;
    if changed > 0 {
        audit::checkpoint(&c.pool, &c.root).await?;
    }
    enqueue(c).await?;
    Ok(changed)
}
async fn enqueue(c: &Core) -> Result<()> {
    let rows=sqlx::query("SELECT a.id,a.deadline_id,s.recipient_role,d.building_id,r.organisation_id FROM alerts a JOIN alert_rule_steps s ON s.rule_id=a.rule_id AND s.days_before=a.days_before AND s.escalation_level=a.escalation_level JOIN alert_rules r ON r.id=a.rule_id JOIN deadlines d ON d.id=a.deadline_id WHERE a.managed=1 AND a.resolved_at IS NULL AND a.acknowledged_at IS NULL").fetch_all(&c.pool).await?;
    for a in rows {
        let id: String = a.get(0);
        let deadline: String = a.get(1);
        let role: String = a.get(2);
        let b: String = a.get(3);
        let org: String = a.get(4);
        let mut recipients = vec![];
        for candidate in [&role, "exploitant"] {
            for u in sqlx::query_scalar::<_,String>("SELECT DISTINCT u.id FROM users u JOIN role_grants g ON g.user_id=u.id WHERE u.organisation_id=? AND u.active=1 AND g.role_code=? AND (g.building_id IS NULL OR g.building_id=?) AND (g.expires_at IS NULL OR g.expires_at>?)").bind(&org).bind(candidate).bind(&b).bind(db::now()).fetch_all(&c.pool).await?{if visible(c,&u,&deadline,"alerts.read").await?{recipients.push(u)}}
            if !recipients.is_empty() {
                break;
            }
        }
        for u in recipients {
            sqlx::query("INSERT OR IGNORE INTO notifications(id,alert_id,user_id,state) VALUES(?,?,?,'pending')").bind(db::id()).bind(&id).bind(u).execute(&c.pool).await?;
        }
    }
    Ok(())
}
pub async fn rule_save(c: &Core, t: &str, v: Value) -> Result<String> {
    crate::shared_workspace::require_write(c)?;
    let u = auth::require(c, t, None).await?;
    let org = preferences::org(c, &u).await?;
    let why = reason(&v)?;
    let b = v["building_id"].as_str().filter(|s| !s.is_empty());
    if let Some(b) = b {
        if !works::permit(c, &u, b, "alerts.manage").await? {
            bail!("Règle non modifiable sur ce bâtiment")
        }
    } else if !preferences::global(c, &u, "alerts.manage").await? {
        bail!("Le périmètre global est nécessaire pour une règle générale")
    };
    if ![
        "*",
        "work",
        "equipment",
        "contract",
        "document",
        "register",
        "manual",
    ]
    .contains(&txt(&v, "family"))
        || txt(&v, "name").trim().is_empty()
        || txt(&v, "name").len() > 200
    {
        bail!("Nom ou famille de règle invalide")
    };
    let delay = v["escalation_days"].as_i64().unwrap_or(0);
    let severity = v["severity"].as_i64().unwrap_or(0);
    if !(1..=365).contains(&delay) || !(1..=4).contains(&severity) {
        bail!("Délai d’escalade ou importance invalide")
    };
    let mut days = vec![0];
    for d in v["days"].as_array().context("Paliers manquants")? {
        let n = d.as_i64().context("Palier entier attendu")?;
        if !(0..=365).contains(&n) {
            bail!("Un palier doit être compris entre J-365 et J-0")
        };
        if !days.contains(&n) {
            days.push(n)
        }
    }
    if days.len() > 12 {
        bail!("12 paliers maximum")
    };
    let roles = [txt(&v, "role"), txt(&v, "role1"), txt(&v, "role2")];
    if roles
        .iter()
        .any(|r| !["exploitant", "securite", "technique", "agent"].contains(r))
    {
        bail!("Choisissez un rôle interne autorisé aux alertes")
    };
    let old = v["id"].as_str();
    if let Some(id)=old{let r=sqlx::query("SELECT organisation_id,building_id,deadline_kind,retired_at FROM alert_rules WHERE id=? AND managed=1").bind(id).fetch_one(&c.pool).await?;if r.get::<String,_>(0)!=org||r.get::<Option<String>,_>(1).as_deref()!=b||r.get::<String,_>(2)!=txt(&v,"family")||r.get::<Option<String>,_>(3).is_some(){bail!("La règle a changé ou son périmètre est différent. Actualisez.")}}else if sqlx::query_scalar::<_,i64>("SELECT count(*) FROM alert_rules WHERE organisation_id=? AND building_id IS ? AND deadline_kind=? AND managed=1 AND retired_at IS NULL").bind(&org).bind(b).bind(txt(&v,"family")).fetch_one(&c.pool).await?>0{bail!("Une règle existe pour ce périmètre et cette famille : modifiez-la.")};
    let id = db::id();
    let mut tx = c.pool.begin().await?;
    if let Some(old) = old {
        sqlx::query(
            "UPDATE alert_rules SET enabled=0,retired_at=? WHERE id=? AND retired_at IS NULL",
        )
        .bind(db::now())
        .bind(old)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("INSERT INTO alert_rules(id,organisation_id,building_id,deadline_kind,enabled,severity,managed,name,escalation_days,created_at) VALUES(?,?,?,?,?,?,1,?,?,?)").bind(&id).bind(&org).bind(b).bind(txt(&v,"family")).bind(i64::from(v["enabled"]==true)).bind(severity).bind(txt(&v,"name").trim()).bind(delay).bind(db::now()).execute(&mut *tx).await?;
    for day in days {
        for (level, role) in roles.iter().enumerate() {
            sqlx::query("INSERT INTO alert_rule_steps VALUES(?,?,?,?)")
                .bind(&id)
                .bind(day)
                .bind(level as i64)
                .bind(*role)
                .execute(&mut *tx)
                .await?;
        }
    }
    audit::append(
        &mut tx,
        &c.key,
        Some(&u),
        "alerts.rule.saved",
        json!({"new":id,"old":old,"settings":v,"reason":why}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await?;
    evaluate(c).await?;
    Ok(id)
}
pub async fn manual_save(c: &Core, t: &str, v: Value) -> Result<String> {
    crate::shared_workspace::require_write(c)?;
    let u = auth::require(c, t, None).await?;
    let org = preferences::org(c, &u).await?;
    let b = txt(&v, "building_id");
    let why = reason(&v)?;
    if !works::permit(c, &u, b, "alerts.manage").await? {
        bail!("Échéance non modifiable dans ce périmètre")
    };
    if sqlx::query_scalar::<_, Option<String>>(
        "SELECT archived_at FROM entities WHERE id=? AND organisation_id=? AND kind='building'",
    )
    .bind(b)
    .bind(&org)
    .fetch_one(&c.pool)
    .await?
    .is_some()
    {
        bail!("Bâtiment archivé")
    };
    if txt(&v, "title").trim().is_empty()
        || txt(&v, "title").len() > 250
        || txt(&v, "notes").len() > 2000
    {
        bail!("Vérifiez le titre et les notes")
    };
    let day = NaiveDate::parse_from_str(txt(&v, "due_on"), "%Y-%m-%d")?;
    if day.to_string() != txt(&v, "due_on") {
        bail!("Date invalide")
    };
    let old = v["id"].as_str();
    let mut restricted = v["restricted"] == true;
    if let Some(old) = old {
        if !visible(c, &u, old, "alerts.manage").await? {
            bail!("Échéance inaccessible")
        };
        let r=sqlx::query("SELECT d.building_id,d.completed_at,m.restricted FROM manual_deadlines m JOIN deadlines d ON d.id=m.deadline_id WHERE m.deadline_id=?").bind(old).fetch_one(&c.pool).await?;
        if r.get::<String, _>(0) != b || r.get::<Option<String>, _>(1).is_some() {
            bail!("L’échéance a changé ou est terminée")
        };
        restricted |= r.get::<i64, _>(2) == 1;
    }
    if restricted && !works::permit(c, &u, b, "documents.financial.read").await? {
        bail!("Échéance financière non autorisée")
    };
    if sqlx::query_scalar::<_,i64>("SELECT count(*) FROM manual_deadlines m JOIN deadlines d ON d.id=m.deadline_id WHERE d.building_id=? AND m.title=? AND d.due_on=? AND d.completed_at IS NULL AND (? IS NULL OR d.id<>?)").bind(b).bind(txt(&v,"title").trim()).bind(txt(&v,"due_on")).bind(old).bind(old).fetch_one(&c.pool).await?>0{bail!("Une échéance identique est déjà active")};
    let id = db::id();
    let mut tx = c.pool.begin().await?;
    if let Some(old) = old {
        sqlx::query("UPDATE deadlines SET completed_at=? WHERE id=?")
            .bind(db::now())
            .bind(old)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE manual_deadlines SET completed_by=?,completion_note='Remplacée par une nouvelle version' WHERE deadline_id=?").bind(&u).bind(old).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO deadlines(id,entity_id,building_id,kind,due_on) VALUES(?,?,?,?,?)")
        .bind(&id)
        .bind(b)
        .bind(b)
        .bind(format!("manual:{}|scope:{id}", txt(&v, "title").trim()))
        .bind(txt(&v, "due_on"))
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO manual_deadlines(deadline_id,title,notes,restricted,created_by,created_at,previous_id) VALUES(?,?,?,?,?,?,?)").bind(&id).bind(txt(&v,"title").trim()).bind(txt(&v,"notes")).bind(i64::from(restricted)).bind(&u).bind(db::now()).bind(old).execute(&mut *tx).await?;
    audit::append(
        &mut tx,
        &c.key,
        Some(&u),
        "alerts.manual.saved",
        json!({"deadline":id,"previous":old,"values":v,"reason":why}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await?;
    evaluate(c).await?;
    Ok(id)
}
pub async fn manual_complete(c: &Core, t: &str, id: &str, why: &str) -> Result<()> {
    crate::shared_workspace::require_write(c)?;
    let u = auth::require(c, t, None).await?;
    reason(&json!({"reason":why}))?;
    if !visible(c, &u, id, "alerts.manage").await? {
        bail!("Échéance non modifiable")
    };
    if sqlx::query_scalar::<_, i64>("SELECT count(*) FROM manual_deadlines WHERE deadline_id=?")
        .bind(id)
        .fetch_one(&c.pool)
        .await?
        != 1
    {
        bail!("Pour terminer cette échéance, utilisez sa fiche d’origine")
    };
    let mut tx = c.pool.begin().await?;
    if sqlx::query("UPDATE deadlines SET completed_at=? WHERE id=? AND completed_at IS NULL")
        .bind(db::now())
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected()
        != 1
    {
        bail!("L’échéance est déjà terminée")
    };
    sqlx::query("UPDATE manual_deadlines SET completed_by=?,completion_note=? WHERE deadline_id=?")
        .bind(&u)
        .bind(why)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::append(
        &mut tx,
        &c.key,
        Some(&u),
        "alerts.manual.completed",
        json!({"deadline":id,"reason":why}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await?;
    evaluate(c).await?;
    Ok(())
}
pub async fn acknowledge(c: &Core, t: &str, id: &str, why: &str) -> Result<()> {
    crate::shared_workspace::require_write(c)?;
    let u = auth::require(c, t, None).await?;
    reason(&json!({"reason":why}))?;
    let d: String =
        sqlx::query_scalar("SELECT deadline_id FROM alerts WHERE id=? AND resolved_at IS NULL")
            .bind(id)
            .fetch_one(&c.pool)
            .await?;
    if !visible(c, &u, &d, "alerts.ack").await? {
        bail!("Prise en charge non autorisée")
    };
    let mut tx = c.pool.begin().await?;
    if sqlx::query("UPDATE alerts SET acknowledged_at=?,acknowledged_by=? WHERE id=? AND acknowledged_at IS NULL AND resolved_at IS NULL").bind(db::now()).bind(&u).bind(id).execute(&mut *tx).await?.rows_affected()!=1{bail!("Ce rappel a déjà changé. Actualisez.")};
    event(&mut tx, id, Some(&u), &db::now(), "acknowledged", why).await?;
    audit::append(
        &mut tx,
        &c.key,
        Some(&u),
        "alerts.acknowledged",
        json!({"alert":id,"reason":why}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}
pub async fn history(c: &Core, t: &str, id: &str) -> Result<Value> {
    let u = auth::require(c, t, None).await?;
    let d: String = sqlx::query_scalar("SELECT deadline_id FROM alerts WHERE id=?")
        .bind(id)
        .fetch_one(&c.pool)
        .await?;
    if !visible(c, &u, &d, "alerts.read").await? {
        bail!("Alerte inaccessible")
    };
    Ok(json!(sqlx::query("SELECT e.*,u.display_name FROM alert_events e LEFT JOIN users u ON u.id=e.actor_id WHERE alert_id=? ORDER BY e.created_at DESC,e.id DESC LIMIT 100").bind(id).fetch_all(&c.pool).await?.iter().map(|r|json!({"id":r.get::<String,_>("id"),"date":r.get::<String,_>("created_at"),"kind":r.get::<String,_>("kind"),"note":r.get::<String,_>("note"),"actor":r.get::<Option<String>,_>("display_name")})).collect::<Vec<_>>()))
}
pub async fn snapshot(c: &Core,t: &str)->Result<Value>{snapshot_inner(c,t,false).await}
pub(crate) async fn snapshot_export(c: &Core,t: &str)->Result<Value>{snapshot_inner(c,t,true).await}
async fn snapshot_inner(c: &Core,t: &str,full:bool)->Result<Value>{
    let u = auth::require(c, t, None).await?;
    evaluate(c).await?;
    let org = preferences::org(c, &u).await?;
    let mut buildings = vec![];
    for r in sqlx::query("SELECT b.id,b.name,e.archived_at FROM buildings b JOIN entities e ON e.id=b.id WHERE b.organisation_id=? ORDER BY b.name").bind(&org).fetch_all(&c.pool).await?{let id:String=r.get(0);if works::permit(c,&u,&id,"alerts.read").await?{buildings.push(json!({"id":id,"name":r.get::<String,_>(1),"can_manage":works::permit(c,&u,&id,"alerts.manage").await?&&r.get::<Option<String>,_>(2).is_none(),"financial":works::permit(c,&u,&id,"documents.financial.read").await?,"archived":r.get::<Option<String>,_>(2).is_some()}))}}
    let mut records = vec![];
    for r in sqlx::query(if full {"SELECT a.*,d.kind,d.due_on,d.building_id,d.entity_id,e.kind AS source_kind,b.name AS building,u.display_name AS actor,s.recipient_role,m.title AS manual FROM alerts a JOIN deadlines d ON d.id=a.deadline_id JOIN entities e ON e.id=d.entity_id JOIN buildings b ON b.id=d.building_id JOIN alert_rule_steps s ON s.rule_id=a.rule_id AND s.days_before=a.days_before AND s.escalation_level=a.escalation_level LEFT JOIN users u ON u.id=a.acknowledged_by LEFT JOIN manual_deadlines m ON m.deadline_id=d.id WHERE e.organisation_id=? AND (a.resolved_at IS NULL OR a.id IN(SELECT id FROM alerts WHERE resolved_at IS NOT NULL ORDER BY resolved_at DESC)) ORDER BY (a.resolved_at IS NULL) DESC,a.severity DESC,d.due_on,a.created_at DESC"} else {"SELECT a.*,d.kind,d.due_on,d.building_id,d.entity_id,e.kind AS source_kind,b.name AS building,u.display_name AS actor,s.recipient_role,m.title AS manual FROM alerts a JOIN deadlines d ON d.id=a.deadline_id JOIN entities e ON e.id=d.entity_id JOIN buildings b ON b.id=d.building_id JOIN alert_rule_steps s ON s.rule_id=a.rule_id AND s.days_before=a.days_before AND s.escalation_level=a.escalation_level LEFT JOIN users u ON u.id=a.acknowledged_by LEFT JOIN manual_deadlines m ON m.deadline_id=d.id WHERE e.organisation_id=? AND (a.resolved_at IS NULL OR a.id IN(SELECT id FROM alerts WHERE resolved_at IS NOT NULL ORDER BY resolved_at DESC LIMIT 3000)) ORDER BY (a.resolved_at IS NULL) DESC,a.severity DESC,d.due_on,a.created_at DESC"}).bind(&org).fetch_all(&c.pool).await?{let did:String=r.get("deadline_id");if !visible(c,&u,&did,"alerts.read").await?{continue};let id:String=r.get("id");let deliveries=sqlx::query("SELECT state,count(*) AS n FROM notifications WHERE alert_id=? AND user_id=? GROUP BY state").bind(&id).bind(&u).fetch_all(&c.pool).await?.iter().map(|r|json!({"state":r.get::<String,_>(0),"count":r.get::<i64,_>(1)})).collect::<Vec<_>>();records.push(json!({"id":id,"deadline_id":did,"building_id":r.get::<String,_>("building_id"),"building":r.get::<String,_>("building"),"title":r.get::<String,_>("kind"),"due_on":r.get::<String,_>("due_on"),"severity":r.get::<i64,_>("severity"),"days_before":r.get::<i64,_>("days_before"),"level":r.get::<i64,_>("escalation_level"),"created_at":r.get::<String,_>("created_at"),"acknowledged_at":r.get::<Option<String>,_>("acknowledged_at"),"actor":r.get::<Option<String>,_>("actor"),"resolved_at":r.get::<Option<String>,_>("resolved_at"),"can_ack":visible(c,&u,&did,"alerts.ack").await?,"can_manage":visible(c,&u,&did,"alerts.manage").await?,"manual":r.get::<Option<String>,_>("manual").is_some(),"source_kind":r.get::<String,_>("source_kind"),"source_id":r.get::<String,_>("entity_id"),"recipient_role":r.get::<String,_>("recipient_role"),"notifications":deliveries}));}
    let mut policy = vec![];
    for r in sqlx::query("SELECT * FROM alert_rules WHERE organisation_id=? AND managed=1 AND retired_at IS NULL ORDER BY building_id,deadline_kind").bind(&org).fetch_all(&c.pool).await?{let b:Option<String>=r.get("building_id");if let Some(b)=&b{if !works::permit(c,&u,b,"alerts.read").await?{continue}}else if buildings.is_empty(){continue};let id:String=r.get("id");let steps=sqlx::query("SELECT days_before,escalation_level,recipient_role FROM alert_rule_steps WHERE rule_id=? ORDER BY days_before DESC,escalation_level").bind(&id).fetch_all(&c.pool).await?.iter().map(|r|json!({"day":r.get::<i64,_>(0),"level":r.get::<i64,_>(1),"role":r.get::<String,_>(2)})).collect::<Vec<_>>();policy.push(json!({"id":id,"building_id":b,"name":r.get::<String,_>("name"),"family":r.get::<String,_>("deadline_kind"),"enabled":r.get::<i64,_>("enabled")==1,"severity":r.get::<i64,_>("severity"),"escalation_days":r.get::<i64,_>("escalation_days"),"steps":steps,"can_manage":if let Some(b)=b{works::permit(c,&u,&b,"alerts.manage").await?}else{preferences::global(c,&u,"alerts.manage").await?}}))}
    let mut manuals = vec![];
    for r in sqlx::query(if full {"SELECT m.*,d.building_id,d.due_on,d.completed_at,b.name AS building FROM manual_deadlines m JOIN deadlines d ON d.id=m.deadline_id JOIN buildings b ON b.id=d.building_id WHERE b.organisation_id=? AND (d.completed_at IS NULL OR d.id IN(SELECT deadline_id FROM manual_deadlines m2 JOIN deadlines d2 ON d2.id=m2.deadline_id WHERE d2.completed_at IS NOT NULL ORDER BY d2.completed_at DESC)) ORDER BY d.due_on"} else {"SELECT m.*,d.building_id,d.due_on,d.completed_at,b.name AS building FROM manual_deadlines m JOIN deadlines d ON d.id=m.deadline_id JOIN buildings b ON b.id=d.building_id WHERE b.organisation_id=? AND (d.completed_at IS NULL OR d.id IN(SELECT deadline_id FROM manual_deadlines m2 JOIN deadlines d2 ON d2.id=m2.deadline_id WHERE d2.completed_at IS NOT NULL ORDER BY d2.completed_at DESC LIMIT 3000)) ORDER BY d.due_on"}).bind(&org).fetch_all(&c.pool).await?{let id:String=r.get("deadline_id");if visible(c,&u,&id,"alerts.read").await?{manuals.push(json!({"id":id,"title":r.get::<String,_>("title"),"notes":r.get::<String,_>("notes"),"restricted":r.get::<i64,_>("restricted")==1,"building_id":r.get::<String,_>("building_id"),"building":r.get::<String,_>("building"),"due_on":r.get::<String,_>("due_on"),"completed_at":r.get::<Option<String>,_>("completed_at"),"can_manage":visible(c,&u,&id,"alerts.manage").await?}))}}
    let last = sqlx::query_scalar::<_, String>(
        "SELECT value_json FROM settings WHERE key='alerts.last_checked'",
    )
    .fetch_optional(&c.pool)
    .await?
    .and_then(|s| serde_json::from_str::<String>(&s).ok());
    let error = sqlx::query_scalar::<_, String>(
        "SELECT value_json FROM settings WHERE key='alerts.last_error'",
    )
    .fetch_optional(&c.pool)
    .await?
    .and_then(|s| serde_json::from_str::<String>(&s).ok());
    Ok(
        json!({"records":records,"rules":policy,"manuals":manuals,"buildings":buildings,"today":works::today(),"settings":settings(c,&org).await?,"can_global":preferences::global(c,&u,"alerts.manage").await?,"portable":crate::portable::active(),"last_checked":last,"engine_error":error}),
    )
}
pub async fn save_settings(c: &Core, t: &str, v: Settings) -> Result<()> {
    crate::shared_workspace::require_write(c)?;
    let u = auth::require(c, t, None).await?;
    if !preferences::global(c, &u, "alerts.manage").await? {
        bail!("Réglage réservé à un responsable au périmètre global")
    };
    let parsed = chrono::NaiveTime::parse_from_str(&v.time, "%H:%M")?;
    if parsed.format("%H:%M").to_string() != v.time {
        bail!("Heure attendue au format HH:MM")
    };
    if v.scheduled && crate::portable::active() {
        bail!("La tâche Windows nécessite la version installée")
    };
    #[cfg(not(test))]
    crate::scheduler::configure_alerts(&c.root, v.scheduled, &v.time)?;
    let org = preferences::org(c, &u).await?;
    let mut tx = c.pool.begin().await?;
    sqlx::query("INSERT INTO settings(key,value_json,modified_at,modified_by) VALUES(?,?,?,?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,modified_at=excluded.modified_at,modified_by=excluded.modified_by").bind(format!("alerts.settings:{org}")).bind(serde_json::to_string(&v)?).bind(db::now()).bind(&u).execute(&mut *tx).await?;
    audit::append(&mut tx, &c.key, Some(&u), "alerts.settings", json!(v)).await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}
fn toast(c: &Core, text: &str) -> Result<()> {
    use tauri_plugin_notification::NotificationExt;
    let app = c
        .app
        .as_ref()
        .context("Notifications disponibles uniquement dans l’application Windows")?;
    app.notification()
        .builder()
        .title("Libre ERP — Alertes")
        .body(text)
        .show()?;
    Ok(())
}
async fn expected_recipient(c: &Core, u: &str, alert: &str) -> Result<bool> {
    let r=sqlx::query("SELECT s.recipient_role,r.organisation_id,d.building_id,a.deadline_id FROM alerts a JOIN alert_rule_steps s ON s.rule_id=a.rule_id AND s.days_before=a.days_before AND s.escalation_level=a.escalation_level JOIN alert_rules r ON r.id=a.rule_id JOIN deadlines d ON d.id=a.deadline_id WHERE a.id=?").bind(alert).fetch_one(&c.pool).await?;
    let role: String = r.get(0);
    let org: String = r.get(1);
    let b: String = r.get(2);
    let d: String = r.get(3);
    for role in [&role, "exploitant"] {
        let mut eligible = vec![];
        for candidate in sqlx::query_scalar::<_,String>("SELECT DISTINCT u.id FROM users u JOIN role_grants g ON g.user_id=u.id WHERE u.organisation_id=? AND u.active=1 AND g.role_code=? AND (g.building_id IS NULL OR g.building_id=?) AND (g.expires_at IS NULL OR g.expires_at>?)").bind(&org).bind(role).bind(&b).bind(db::now()).fetch_all(&c.pool).await?{if visible(c,&candidate,&d,"alerts.read").await?{eligible.push(candidate)}}
        if !eligible.is_empty() {
            return Ok(eligible.iter().any(|id| id == u));
        }
    }
    Ok(false)
}
pub async fn notify(c: &Core, t: &str) -> Result<usize> {
    let u = auth::require(c, t, None).await?;
    let org = preferences::org(c, &u).await?;
    if !settings(c, &org).await?.desktop || !centre_enabled(c, &org).await? {
        return Ok(0);
    };
    let last = sqlx::query_scalar::<_, String>("SELECT value_json FROM settings WHERE key=?")
        .bind(format!("alerts.last_toast:{u}"))
        .fetch_optional(&c.pool)
        .await?;
    if last
        .and_then(|s| serde_json::from_str::<String>(&s).ok())
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .is_some_and(|d| (Utc::now() - d.with_timezone(&Utc)).num_seconds() < 600)
    {
        return Ok(0);
    };
    let mut ids = vec![];
    for r in sqlx::query("SELECT n.id,n.alert_id,a.deadline_id FROM notifications n JOIN alerts a ON a.id=n.alert_id WHERE n.user_id=? AND n.state IN('pending','failed') AND n.attempts<3 AND (n.next_attempt_at IS NULL OR n.next_attempt_at<=?) AND a.resolved_at IS NULL AND a.acknowledged_at IS NULL ORDER BY n.id LIMIT 100").bind(&u).bind(db::now()).fetch_all(&c.pool).await?{if visible(c,&u,&r.get::<String,_>(2),"alerts.read").await?&&expected_recipient(c,&u,&r.get::<String,_>(1)).await?{ids.push(r.get::<String,_>(0));}}
    if ids.is_empty() {
        return Ok(0);
    }
    let sent=toast(c,"Des alertes vous concernent. Ouvrez Libre ERP pour les consulter et les prendre en charge.");
    let ok = sent.is_ok();
    let error = sent.err().map(|e| e.to_string()).unwrap_or_default();
    let mut tx = c.pool.begin().await?;
    for id in &ids {
        sqlx::query("UPDATE notifications SET state=?,attempts=attempts+1,next_attempt_at=?,delivered_at=?,last_error=? WHERE id=?").bind(if ok{"delivered"}else{"failed"}).bind(if ok{None}else{Some((Utc::now()+chrono::Duration::minutes(10)).to_rfc3339_opts(chrono::SecondsFormat::Millis,true))}).bind(if ok{Some(db::now())}else{None}).bind(if ok{None}else{Some(error.clone())}).bind(id).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO notification_attempts VALUES(?,?,?,?,?)")
            .bind(db::id())
            .bind(id)
            .bind(db::now())
            .bind(if ok { "delivered" } else { "failed" })
            .bind(if ok {
                "Transmis à Windows (pas une confirmation de lecture)"
            } else {
                &error
            })
            .execute(&mut *tx)
            .await?;
    }
    if ok {
        sqlx::query("INSERT INTO settings(key,value_json,modified_at) VALUES(?,?,?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,modified_at=excluded.modified_at").bind(format!("alerts.last_toast:{u}")).bind(json!(db::now()).to_string()).bind(db::now()).execute(&mut *tx).await?;
    }
    audit::append(
        &mut tx,
        &c.key,
        Some(&u),
        "alerts.notification",
        json!({"count":ids.len(),"accepted":ok}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await?;
    Ok(if ok { ids.len() } else { 0 })
}
pub async fn retry(c: &Core, t: &str) -> Result<()> {
    crate::shared_workspace::require_write(c)?;
    let u = auth::require(c, t, None).await?;
    let rows=sqlx::query("SELECT n.id,a.deadline_id FROM notifications n JOIN alerts a ON a.id=n.alert_id WHERE n.user_id=? AND n.state='failed' AND a.resolved_at IS NULL AND a.acknowledged_at IS NULL").bind(&u).fetch_all(&c.pool).await?;
    let mut ids = vec![];
    for r in rows {
        if visible(c, &u, &r.get::<String, _>(1), "alerts.read").await? {
            ids.push(r.get::<String, _>(0))
        }
    }
    let mut tx = c.pool.begin().await?;
    for id in &ids {
        sqlx::query("UPDATE notifications SET state='pending',attempts=0,next_attempt_at=NULL,last_error=NULL WHERE id=? AND user_id=?").bind(id).bind(&u).execute(&mut *tx).await?;
    }
    audit::append(
        &mut tx,
        &c.key,
        Some(&u),
        "alerts.notification.retry",
        json!({"count":ids.len()}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}
pub async fn test_notification(c: &Core, t: &str) -> Result<()> {
    let u = auth::require(c, t, None).await?;
    let n:i64=sqlx::query_scalar("SELECT count(*) FROM role_grants g JOIN role_permissions p ON p.role_code=g.role_code WHERE g.user_id=? AND p.permission_code='alerts.read' AND (g.expires_at IS NULL OR g.expires_at>?)").bind(&u).bind(db::now()).fetch_one(&c.pool).await?;
    if n == 0 {
        bail!("Test non autorisé")
    };
    toast(c, "Ceci est une notification de test Libre ERP.")
}
pub async fn scheduled(c: &Core) -> Result<()> {
    evaluate(c).await?;
    let day = works::today();
    for org in
        sqlx::query_scalar::<_, String>("SELECT id FROM organisations WHERE archived_at IS NULL")
            .fetch_all(&c.pool)
            .await?
    {
        let cfg = settings(c, &org).await?;
        if !cfg.scheduled
            || !cfg.desktop
            || crate::portable::active()
            || !centre_enabled(c, &org).await?
        {
            continue;
        };
        let key = format!("alerts.last_scheduled:{org}");
        let last = sqlx::query_scalar::<_, String>("SELECT value_json FROM settings WHERE key=?")
            .bind(&key)
            .fetch_optional(&c.pool)
            .await?;
        if last
            .and_then(|s| serde_json::from_str::<String>(&s).ok())
            .as_deref()
            == Some(&day)
        {
            continue;
        };
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM alerts a JOIN deadlines d ON d.id=a.deadline_id JOIN entities e ON e.id=d.entity_id WHERE a.resolved_at IS NULL AND a.acknowledged_at IS NULL AND e.organisation_id=?").bind(&org).fetch_one(&c.pool).await?;
        if count > 0 {
            toast(c,"Des échéances nécessitent une vérification. Ouvrez Libre ERP pour consulter vos alertes.")?;
        }
        sqlx::query("INSERT INTO settings(key,value_json,modified_at) VALUES(?,?,?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,modified_at=excluded.modified_at").bind(key).bind(json!(&day).to_string()).bind(db::now()).execute(&c.pool).await?;
    }
    Ok(())
}

pub async fn summary(c: &Core, t: &str) -> Result<Value> {
    let u = auth::require(c, t, None).await?;
    let can:i64=sqlx::query_scalar("SELECT count(*) FROM role_grants g JOIN role_permissions p ON p.role_code=g.role_code WHERE g.user_id=? AND p.permission_code='alerts.read' AND (g.expires_at IS NULL OR g.expires_at>?)").bind(&u).bind(db::now()).fetch_one(&c.pool).await?;
    let mut open = 0;
    let mut critical = 0;
    let mut severity = 0;
    for r in sqlx::query("SELECT deadline_id,severity FROM alerts WHERE resolved_at IS NULL")
        .fetch_all(&c.pool)
        .await?
    {
        if visible(c, &u, &r.get::<String, _>(0), "alerts.read").await? {
            open += 1;
            severity = severity.max(r.get::<i64, _>(1));
            if r.get::<i64, _>(1) == 4 {
                critical += 1
            }
        }
    }
    Ok(json!({"open":open,"critical":critical,"severity":severity,"can_read":can>0}))
}
async fn centre_enabled(c: &Core, org: &str) -> Result<bool> {
    let v = sqlx::query_scalar::<_, String>("SELECT value_json FROM settings WHERE key=?")
        .bind(format!("features:{org}"))
        .fetch_optional(&c.pool)
        .await?;
    Ok(v.and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["alerts"].as_bool())
        .unwrap_or(true))
}
