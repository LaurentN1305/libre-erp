use crate::{audit, auth, db, inventory, preferences, Core};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
const HEADERS: &[&str] = &[
    "designation",
    "categorie",
    "localisation",
    "fabricant",
    "modele",
    "numero_serie",
    "etat",
    "date_achat",
    "date_installation",
    "date_mise_service",
    "duree_vie_mois",
    "cout_achat_euros",
    "puissance_w",
    "entretien_mois",
    "entretien_jours",
    "dernier_entretien",
    "prochain_entretien",
    "fin_vie",
    "notes",
];
fn hash(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}
pub fn parse(content: &str) -> Result<Vec<Vec<String>>> {
    if content.len() > 2 * 1024 * 1024 {
        bail!("CSV limité à 2 Mio")
    }
    let content = content.trim_start_matches('\u{feff}');
    let header = content.lines().next().unwrap_or("");
    let sep = if header.contains(';') {
        ';'
    } else if header.contains('\t') {
        '\t'
    } else {
        ','
    };
    let mut rows = vec![];
    let mut row = vec![];
    let mut field = String::new();
    let mut quoted = false;
    let mut after = false;
    let mut chars = content.chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"')
                } else {
                    quoted = false;
                    after = true
                }
            } else {
                field.push(ch)
            }
        } else if ch == sep {
            row.push(std::mem::take(&mut field));
            after = false;
        } else if ch == '\r' || ch == '\n' {
            if ch == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            row.push(std::mem::take(&mut field));
            if row.iter().any(|s| !s.trim().is_empty()) {
                rows.push(std::mem::take(&mut row))
            } else {
                row.clear()
            }
            after = false;
        } else if ch == '"' {
            if field.is_empty() && !after {
                quoted = true
            } else {
                bail!("Guillemet inattendu à la ligne {}", rows.len() + 1)
            }
        } else {
            if after {
                bail!(
                    "Caractère après un champ entre guillemets, ligne {}",
                    rows.len() + 1
                )
            }
            field.push(ch)
        }
        if field.len() > 20000 || row.len() > 40 || rows.len() > 5001 {
            bail!("CSV trop volumineux : 5 000 lignes, 40 colonnes et 20 000 caractères par cellule au maximum")
        }
    }
    if quoted {
        bail!("Champ entre guillemets non terminé")
    }
    row.push(field);
    if row.iter().any(|s| !s.trim().is_empty()) {
        rows.push(row)
    }
    if rows.len() < 2 {
        bail!("Ajoutez au moins une ligne sous les en-têtes")
    }
    Ok(rows)
}
#[derive(Serialize, Deserialize, Clone)]
pub struct PreviewRow {
    pub line: usize,
    pub designation: String,
    pub status: String,
    pub message: String,
}
#[derive(Serialize, Deserialize)]
pub struct Preview {
    pub content: String,
    pub seal: String,
    pub rows: Vec<PreviewRow>,
    pub ready: usize,
    pub skipped: usize,
    pub errors: usize,
}
struct Plan {
    preview: Preview,
    inputs: Vec<(inventory::Input, String)>,
}
fn num(s: &str) -> Result<Option<i64>> {
    if s.is_empty() {
        Ok(None)
    } else {
        Ok(Some(s.parse::<i64>().context("Nombre entier attendu")?))
    }
}
fn euros(s: &str) -> Result<Option<i64>> {
    if s.is_empty() {
        return Ok(None);
    }
    let s = s.replace(',', ".");
    let mut it = s.split('.');
    let whole = it.next().unwrap_or("");
    let dec = it.next().unwrap_or("");
    if it.next().is_some()
        || whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || dec.len() > 2
        || !dec.bytes().all(|b| b.is_ascii_digit())
    {
        bail!("Coût : montant positif avec deux décimales maximum")
    };
    let cents = whole
        .parse::<i64>()?
        .checked_mul(100)
        .and_then(|n| n.checked_add(format!("{dec:0<2}").parse::<i64>().ok()?))
        .context("Montant trop élevé")?;
    Ok(Some(cents))
}
fn date(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.into())
    }
}
async fn plan(c: &Core, t: &str, b: &str, content: &str) -> Result<Plan> {
    let u = auth::require(c, t, None).await?;
    let org = preferences::org(c, &u).await?;
    if !crate::works::permit(c, &u, b, "inventory.read").await?
        || !crate::works::permit(c, &u, b, "inventory.write").await?
    {
        bail!("Import non autorisé dans ce bâtiment")
    }
    let active:i64=sqlx::query_scalar("SELECT count(*) FROM entities WHERE id=? AND organisation_id=? AND kind='building' AND archived_at IS NULL").bind(b).bind(&org).fetch_one(&c.pool).await?;
    if active != 1 {
        bail!("Bâtiment archivé ou inaccessible")
    }
    let mut raw = parse(content)?;
    let headers = raw
        .remove(0)
        .iter()
        .map(|s| s.trim().to_lowercase())
        .collect::<Vec<_>>();
    let mut unique = HashSet::new();
    for h in &headers {
        if !HEADERS.contains(&h.as_str()) || !unique.insert(h.clone()) {
            bail!("Colonne inconnue ou répétée : {h}. Utilisez le modèle proposé.")
        }
    }
    for h in ["designation", "categorie"] {
        if !headers.iter().any(|v| v == h) {
            bail!("Colonne obligatoire absente : {h}")
        }
    }
    let s = inventory::snapshot(c, t).await?;
    let mut categories = HashMap::new();
    for cat in s["categories"].as_array().unwrap() {
        for k in ["code", "name"] {
            let name = cat[k].as_str().unwrap().to_lowercase();
            let id = cat["id"].as_str().unwrap().to_string();
            if categories.get(&name).is_some_and(|p: &String| p != &id) {
                categories.insert(name, String::new());
            } else {
                categories.insert(name, id);
            }
        }
    }
    let locations: Vec<_> = s["locations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["building_id"] == b)
        .collect();
    let existing: Vec<_> = s["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["building_id"] == b)
        .collect();
    let known: HashSet<String> = sqlx::query_scalar(
        "SELECT fingerprint FROM inventory_import_rows WHERE organisation_id=? AND building_id=?",
    )
    .bind(&org)
    .bind(b)
    .fetch_all(&c.pool)
    .await?
    .into_iter()
    .collect();
    let mut seen_serial = HashSet::new();
    let mut seen = HashSet::new();
    let mut inputs = vec![];
    let mut rows = vec![];
    for (idx, raw) in raw.into_iter().enumerate() {
        let line = idx + 2;
        let map: HashMap<_, _> = headers
            .iter()
            .cloned()
            .zip(raw.iter().map(|s| s.trim().to_string()))
            .collect();
        let get = |k: &str| map.get(k).map(String::as_str).unwrap_or("");
        let designation = get("designation").to_string();
        let result:Result<(inventory::Input,String)>=async {
 if raw.len()!=headers.len(){bail!("{} colonnes attendues, {} reçues",headers.len(),raw.len())}
 let category=categories.get(&get("categorie").to_lowercase()).filter(|s|!s.is_empty()).context("Catégorie inconnue ou ambiguë : utilisez son code")?.clone();
 let location_id=if get("localisation").is_empty(){None}else{let candidates:Vec<_>=locations.iter().filter(|l|l["id"]==get("localisation")||l["name"].as_str().is_some_and(|n|n.eq_ignore_ascii_case(get("localisation")))).collect();if candidates.len()!=1{bail!("Localisation inconnue ou ambiguë : créez-la dans l’inventaire et utilisez son identifiant")};Some(candidates[0]["id"].as_str().unwrap().to_string())};
 let condition=match get("etat").to_lowercase().as_str(){""|"bon"|"good"=>"good","neuf"|"new"=>"new","usé"|"use"|"worn"=>"worn","défectueux"|"defectueux"|"faulty"=>"faulty","hors service"|"out_of_service"=>"out_of_service",_=>bail!("État inconnu")}.to_string();
 let input=inventory::Input{building_id:b.into(),designation:designation.clone(),category_id:category,location_id,manufacturer:get("fabricant").into(),model:get("modele").into(),serial_number:get("numero_serie").into(),condition,purchased_on:date(get("date_achat")),installed_on:date(get("date_installation")),commissioned_on:date(get("date_mise_service")),lifespan_months:num(get("duree_vie_mois"))?,purchase_cost_cents:euros(get("cout_achat_euros"))?,rated_power_w:num(get("puissance_w"))?,maintenance_interval_months:num(get("entretien_mois"))?,maintenance_interval_days:num(get("entretien_jours"))?,last_maintenance_on:date(get("dernier_entretien")),maintenance_due_override:date(get("prochain_entretien")),life_due_override:date(get("fin_vie")),notes:get("notes").into(),reason:"Import CSV de l’inventaire".into(),..Default::default()};
 inventory::prepare(c,t,input.clone()).await?;let fingerprint=hash(&serde_json::to_string(&input)?);Ok((input,fingerprint))}.await;
        match result {
            Err(e) => rows.push(PreviewRow {
                line,
                designation,
                status: "error".into(),
                message: e.to_string(),
            }),
            Ok((input, fingerprint)) => {
                let message = if known.contains(&fingerprint) {
                    Some("Ligne déjà importée dans ce bâtiment")
                } else if !seen.insert(fingerprint.clone()) {
                    Some("Ligne identique répétée dans ce fichier")
                } else if !input.serial_number.is_empty()
                    && !seen_serial.insert((
                        input.serial_number.clone(),
                        input.manufacturer.to_lowercase(),
                    ))
                {
                    Some("N° de série et fabricant répétés dans ce fichier")
                } else if !input.serial_number.is_empty()
                    && existing.iter().any(|v| {
                        v["serial_number"].as_str() == Some(input.serial_number.as_str())
                            && v["manufacturer"]
                                .as_str()
                                .unwrap_or("")
                                .eq_ignore_ascii_case(&input.manufacturer)
                    })
                {
                    Some("N° de série et fabricant déjà présents dans ce bâtiment")
                } else {
                    None
                };
                if let Some(msg) = message {
                    rows.push(PreviewRow {
                        line,
                        designation,
                        status: "skip".into(),
                        message: msg.into(),
                    })
                } else {
                    rows.push(PreviewRow {
                        line,
                        designation,
                        status: "ready".into(),
                        message: "Nouvel équipement".into(),
                    });
                    inputs.push((input, fingerprint))
                }
            }
        }
    }
    let ready = inputs.len();
    let errors = rows.iter().filter(|r| r.status == "error").count();
    let skipped = rows.len() - ready - errors;
    let seal = hash(
        &json!({"org":org,"building":b,"content":hash(content),"rows":rows,"inputs":inputs})
            .to_string(),
    );
    Ok(Plan {
        preview: Preview {
            content: content.into(),
            seal,
            rows,
            ready,
            errors,
            skipped,
        },
        inputs,
    })
}
pub async fn preview(c: &Core, t: &str, b: &str, content: &str) -> Result<Preview> {
    Ok(plan(c, t, b, content).await?.preview)
}
pub async fn apply(c: &Core, t: &str, b: &str, content: &str, seal: &str) -> Result<Value> {
    crate::shared_workspace::require_write(c)?;
    let p = plan(c, t, b, content).await?;
    if p.preview.seal != seal {
        bail!("Les données ou l’aperçu ont changé. Relisez un nouvel aperçu avant d’importer.")
    }
    if p.preview.errors > 0 {
        bail!("Corrigez toutes les erreurs avant l’import. Aucun équipement ajouté.")
    }
    let u = auth::require(c, t, None).await?;
    let org = preferences::org(c, &u).await?;
    let mut prepared = vec![];
    for (input, fingerprint) in p.inputs {
        prepared.push((inventory::prepare(c, t, input).await?, fingerprint))
    }
    let count = prepared.len();
    let mut tx = c.pool.begin().await?;
    for (prepared, fingerprint) in prepared {
        let id = inventory::write_prepared(&mut tx, &c.key, prepared).await?;
        sqlx::query("INSERT INTO inventory_import_rows VALUES(?,?,?,?,?,?)")
            .bind(&org)
            .bind(b)
            .bind(fingerprint)
            .bind(id)
            .bind(db::now())
            .bind(&u)
            .execute(&mut *tx)
            .await?;
    }
    audit::append(
        &mut tx,
        &c.key,
        Some(&u),
        "inventory.csv.import",
        json!({"building":b,"added":count,"skipped":p.preview.skipped,"digest":hash(content)}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await?;
    Ok(json!({"added":count,"skipped":p.preview.skipped}))
}
pub fn template() -> Vec<u8> {
    format!("\u{feff}{}\r\n", HEADERS.join(";")).into_bytes()
}
