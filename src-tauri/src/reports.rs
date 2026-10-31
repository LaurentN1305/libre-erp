use crate::{audit, auth, db, inventory, Core};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use std::io::{Cursor, Write};
#[derive(Serialize, Deserialize, Clone)]
pub struct Column {
    pub key: String,
    pub label: String,
    pub kind: String,
}
#[derive(Serialize, Deserialize, Clone)]
pub struct Table {
    pub fingerprint: String,
    pub title: String,
    pub generated_at: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Value>>,
    pub note: String,
}
fn array(v: &Value, k: &str) -> Vec<Value> {
    v[k].as_array().cloned().unwrap_or_default()
}
fn text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(text).collect::<Vec<_>>().join(" / "),
        Value::Bool(b) => if *b { "Oui" } else { "Non" }.into(),
        _ => v.to_string(),
    }
}
fn label(s: &str) -> &str {
    match s {
        "new" => "Neuf",
        "good" => "Bon",
        "worn" => "Usé",
        "faulty" => "Défectueux",
        "out_of_service" => "Hors service",
        "identified" => "Identifié",
        "scheduled" => "Programmé",
        "in_progress" => "En cours",
        "pending_validation" => "En attente de validation",
        "closed" => "Clôturé",
        "archived" => "Archivé",
        "preventive" => "Préventif",
        "corrective" => "Correctif",
        "applicable" => "Applicable",
        "not_applicable" => "Non applicable",
        "to_confirm" => "À confirmer",
        "compliance" => "Conformité",
        "commission" => "Observation commission",
        _ => s,
    }
}
pub async fn table(
    c: &Core,
    t: &str,
    kind: &str,
    building: Option<&str>,
    archives: bool,
) -> Result<Table> {
    let user = auth::require(c, t, None).await?;
    let any:i64=sqlx::query_scalar("SELECT count(*) FROM role_grants g JOIN role_permissions p ON p.role_code=g.role_code WHERE g.user_id=? AND p.permission_code='exports.create' AND (g.expires_at IS NULL OR g.expires_at>?)").bind(&user).bind(db::now()).fetch_one(&c.pool).await?;
    if any == 0 {
        bail!("Votre rôle ne permet pas les exports")
    }
    let org = crate::preferences::org(c, &user).await?;
    let mut export_buildings = std::collections::HashSet::new();
    for id in sqlx::query_scalar::<_, String>("SELECT id FROM buildings WHERE organisation_id=?")
        .bind(&org)
        .fetch_all(&c.pool)
        .await?
    {
        if crate::works::permit(c, &user, &id, "exports.create").await? {
            export_buildings.insert(id);
        }
    }
    let global_export = crate::preferences::global(c, &user, "exports.create").await?;
    let (title, data, mut spec): (&str, Vec<Value>, Vec<(&str, &str, &str)>) = match kind {
        "buildings" => (
            "Bâtiments",
            array(&crate::dashboard::snapshot(c, t).await?, "buildings"),
            vec![
                ("name", "Bâtiment", "text"),
                ("address", "Adresse", "text"),
                ("types", "Types ERP", "text"),
                ("category", "Catégorie", "number"),
                ("capacity", "Capacité", "number"),
                ("opened_on", "Ouverture", "date"),
                ("manager_name", "Responsable sécurité", "text"),
                ("archived", "Archivé", "text"),
            ],
        ),
        "register" => {
            let bs = crate::dashboard::snapshot(c, t).await?;
            let mut rows = vec![];
            for b in array(&bs, "buildings") {
                if b["register_read"] != true
                    || building.is_some_and(|id| b["id"] != id)
                    || !archives && b["archived"] == true
                {
                    continue;
                }
                let snap = crate::register::snapshot(c, t, b["id"].as_str().unwrap()).await?;
                for mut e in array(&snap, "entries") {
                    e["building_id"] = b["id"].clone();
                    e["building"] = b["name"].clone();
                    let schema = array(&snap, "schema");
                    let block = schema.iter().find(|s| s["code"] == e["block"]);
                    let mut fields = vec![];
                    if let Some(block) = block {
                        for f in array(block, "fields") {
                            if let Some(k) = f["key"].as_str() {
                                fields.push(format!(
                                    "{} : {}",
                                    text(&f["label"]),
                                    text(&e["fields"][k])
                                ));
                            }
                        }
                    }
                    e["details"] = json!(fields.join("\n"));
                    e["attachments"] = json!(array(&e, "files")
                        .iter()
                        .filter_map(|id| array(&snap, "files").into_iter().find(|f| f["id"] == *id))
                        .map(|f| text(&f["name"]))
                        .collect::<Vec<_>>()
                        .join(" / "));
                    e["building_archived"] = b["archived"].clone();
                    rows.push(e)
                }
            }
            (
                "Registre — liste des entrées",
                rows,
                vec![
                    ("building", "Bâtiment", "text"),
                    ("block", "Bloc", "text"),
                    ("title", "Titre", "text"),
                    ("effective_on", "Date", "date"),
                    ("version", "Version", "number"),
                    ("actor", "Saisi par", "text"),
                    ("recorded_at", "Horodatage", "text"),
                    ("applicability", "Applicabilité", "enum"),
                    ("details", "Informations", "text"),
                    ("attachments", "Pièces jointes (noms)", "text"),
                    ("archived", "Archivé", "text"),
                ],
            )
        }
        "works" => (
            "Interventions",
            array(&crate::works::snapshot(c, t).await?, "records"),
            vec![
                ("building", "Bâtiment", "text"),
                ("reference", "Référence", "text"),
                ("title", "Intervention", "text"),
                ("kind", "Type", "enum"),
                ("status", "État", "enum"),
                ("priority", "Priorité", "number"),
                ("target_due_on", "Échéance", "date"),
                ("participants", "Intervenants", "participants"),
                ("planned_start", "Début prévu", "date"),
                ("planned_end", "Fin prévue", "date"),
                ("progress_percent", "Avancement (%)", "number"),
                ("estimated_budget_cents", "Budget estimé (€)", "money"),
                ("actual_budget_cents", "Budget réalisé (€)", "money"),
                ("description", "Description", "text"),
            ],
        ),
        "inventory" => (
            "Inventaire",
            array(&inventory::snapshot(c, t).await?, "records"),
            vec![
                ("building", "Bâtiment", "text"),
                ("reference", "Référence", "text"),
                ("designation", "Désignation", "text"),
                ("category", "Catégorie", "text"),
                ("location", "Localisation", "text"),
                ("manufacturer", "Fabricant", "text"),
                ("model", "Modèle", "text"),
                ("serial_number", "N° série", "text"),
                ("condition", "État", "enum"),
                ("commissioned_on", "Mise en service", "date"),
                ("next_maintenance_on", "Prochain entretien", "date"),
                ("end_of_life_on", "Fin de vie prévue", "date"),
                ("purchase_cost_cents", "Coût achat (€)", "money"),
                ("rated_power_w", "Puissance (W)", "number"),
                ("notes", "Notes", "text"),
            ],
        ),
        "contracts" => {
            let s = crate::contracts::snapshot(c, t).await?;
            let rows = array(&s, "records")
                .into_iter()
                .map(|mut v| {
                    v["building"] = json!(array(&v, "buildings")
                        .iter()
                        .map(|b| text(&b["name"]))
                        .collect::<Vec<_>>()
                        .join(" / "));
                    v
                })
                .collect();
            (
                "Contrats",
                rows,
                vec![
                    ("building", "Bâtiments", "text"),
                    ("reference", "Référence", "text"),
                    ("subject", "Objet", "text"),
                    ("provider", "Prestataire", "text"),
                    ("starts_on", "Début", "date"),
                    ("ends_on", "Fin", "date"),
                    ("cost_cents", "Coût (€)", "money"),
                    ("cost_period", "Périodicité du coût", "text"),
                    ("terms", "Conditions", "text"),
                ],
            )
        }
        "documents" => (
            "Documents",
            array(&crate::documents::snapshot_export(c, t).await?, "records"),
            vec![
                ("building", "Bâtiment", "text"),
                ("title", "Titre", "text"),
                ("category", "Catégorie", "text"),
                ("name", "Fichier", "text"),
                ("version", "Version", "number"),
                ("expires_on", "Expiration", "date"),
                ("date", "Date de version", "text"),
                ("size", "Taille (octets)", "number"),
            ],
        ),
        "alerts" => (
            "Alertes",
            array(&crate::alerts::snapshot_export(c, t).await?, "records"),
            vec![
                ("building", "Bâtiment", "text"),
                ("title", "Échéance", "deadline"),
                ("due_on", "Date limite", "date"),
                ("severity", "Importance (1 à 4)", "number"),
                ("level", "Escalade", "number"),
                ("acknowledged_at", "Pris en charge le", "text"),
                ("actor", "Pris en charge par", "text"),
                ("resolved_at", "Clôturé le", "text"),
            ],
        ),
        "consumption" => {
            let s = inventory::snapshot(c, t).await?;
            let mut rows = vec![];
            for v in array(&s, "records") {
                if !archives && (v["archived"] == true || v["building_archived"] == true) {
                    continue;
                }
                if building.is_some_and(|b| v["building_id"] != b) {
                    continue;
                }
                let id = v["id"].as_str().context("Équipement invalide")?;
                for r in sqlx::query("SELECT r.*,x.recorded_at AS cancelled_at FROM consumption_readings r LEFT JOIN consumption_cancellations x ON x.reading_id=r.id WHERE r.equipment_id=? ORDER BY r.measured_at,r.id").bind(id).fetch_all(&c.pool).await?{rows.push(json!({"building_id":v["building_id"],"building":v["building"],"reference":v["reference"],"designation":v["designation"],"date":r.get::<String,_>("measured_at"),"quantity":r.get::<i64,_>("quantity_milliunits") as f64/1000.,"unit":r.get::<String,_>("unit"),"mode":if r.get::<String,_>("mode")=="cumulative"{"Index cumulatif"}else{"Consommation de période"},"period_start":r.get::<Option<String>,_>("period_start"),"reset":r.get::<i64,_>("reset")==1,"cancelled":r.get::<Option<String>,_>("cancelled_at").is_some()}));}
            }
            (
                "Consommations et relevés",
                rows,
                vec![
                    ("building", "Bâtiment", "text"),
                    ("reference", "Référence", "text"),
                    ("designation", "Équipement", "text"),
                    ("date", "Date du relevé", "date"),
                    ("quantity", "Quantité brute", "decimal"),
                    ("unit", "Unité", "text"),
                    ("mode", "Type de relevé", "text"),
                    ("period_start", "Début de période", "date"),
                    ("reset", "Remise à zéro", "text"),
                    ("cancelled", "Annulé", "text"),
                ],
            )
        }
        _ => bail!("Type d’export inconnu"),
    };
    let data: Vec<_> = data
        .into_iter()
        .filter(|v| {
            let mut scope = vec![];
            if kind == "buildings" {
                if let Some(b) = v["id"].as_str() {
                    scope.push(b.to_string())
                }
            } else {
                if let Some(b) = v["building_id"].as_str() {
                    scope.push(b.to_string())
                }
                for b in array(v, "buildings") {
                    if let Some(b) = b["id"].as_str() {
                        scope.push(b.to_string())
                    }
                }
                for b in array(v, "building_ids") {
                    if let Some(b) = b.as_str() {
                        scope.push(b.to_string())
                    }
                }
            }
            if (scope.is_empty() && !global_export)
                || scope.iter().any(|b| !export_buildings.contains(b))
            {
                return false;
            }

            let matches = building.is_none_or(|b| {
                if kind == "buildings" {
                    v["id"] == b
                } else {
                    v["building_id"] == b
                        || array(v, "buildings").iter().any(|x| x["id"] == b)
                        || array(v, "building_ids").iter().any(|x| x == b)
                }
            });
            matches
                && (archives
                    || !(v["archived"] == true
                        || v["building_archived"] == true
                        || v["status"] == "archived"
                        || (kind == "alerts" && !v["resolved_at"].is_null())))
        })
        .collect();
    if data.len() > 20000 {
        bail!("Plus de 20 000 lignes : filtrez par bâtiment")
    }
    // Aucune colonne financière vide ne doit donner l’illusion d’un budget nul.
    spec.retain(|(k, _, ty)| *ty != "money" || data.iter().any(|v| !v[*k].is_null()));
    let columns = spec
        .iter()
        .map(|(k, l, ty)| Column {
            key: (*k).into(),
            label: (*l).into(),
            kind: if *ty == "enum" || *ty == "deadline" {
                "text".into()
            } else {
                (*ty).into()
            },
        })
        .collect();
    let rows = data
        .iter()
        .map(|v| {
            spec.iter()
                .map(|(k, _, ty)| {
                    let val = &v[*k];
                    if val.is_null() {
                        return Value::Null;
                    }
                    match *ty {
                        "money" => val
                            .as_i64()
                            .map(|n| json!(n as f64 / 100.))
                            .unwrap_or(Value::Null),
                        "number" | "decimal" => val.clone(),
                        "participants" => json!(val.as_array().map(|a|a.iter().filter_map(|p|p["name"].as_str()).collect::<Vec<_>>().join(" / ")).unwrap_or_default()),
                        "enum" => json!(label(val.as_str().unwrap_or(""))),
                        "deadline" => {
                            let s = text(val);
                            let s = s.split("|scope:").next().unwrap_or(&s);
                            json!(s
                                .strip_prefix("work:")
                                .or_else(|| s.strip_prefix("manual:"))
                                .or_else(|| s.strip_prefix("equipment:"))
                                .or_else(|| s.strip_prefix("contract:"))
                                .or_else(|| s.strip_prefix("document:"))
                                .unwrap_or(s))
                        }
                        _ => json!(text(val)),
                    }
                })
                .collect()
        })
        .collect();
    let fingerprint = {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(
            json!({"columns":columns,"rows":rows})
                .to_string()
                .as_bytes(),
        ))
    };
    Ok(Table{fingerprint,title:title.into(),generated_at:db::now(),columns,rows,note:if kind=="consumption"{"Valeurs brutes : les index cumulatifs ne sont pas des consommations à additionner. Les relevés annulés sont signalés."}else{"Export des données accessibles au compte connecté. Une cellule vide peut être non renseignée ou masquée par les droits d’accès. Ce document ne constitue pas une attestation de conformité."}.into()})
}
fn xml(s: &str) -> String {
    s.chars().filter(|c|matches!(*c,'\t'|'\n'|'\r'|'\u{20}'..='\u{d7ff}'|'\u{e000}'..='\u{fffd}'|'\u{10000}'..='\u{10ffff}')).collect::<String>().replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;").replace('\'',"&apos;")
}
fn cell_string(s: &str) -> String {
    format!("<is><t xml:space=\"preserve\">{}</t></is>", xml(s))
}
fn col(mut n: usize) -> String {
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        if n < 26 {
            return s;
        }
        n = n / 26 - 1;
    }
}
pub fn csv(table: &Table) -> Vec<u8> {
    let mut out = String::from("\u{feff}");
    let mut rows = vec![table
        .columns
        .iter()
        .map(|c| json!(c.label))
        .collect::<Vec<_>>()];
    rows.extend(table.rows.clone());
    for row in rows {
        let fields = row
            .iter()
            .map(|v| {
                let mut s = text(v);
                if v.is_number() {
                    s = s.replace('.', ",")
                } else if s
                    .trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
                    .starts_with(['=', '+', '-', '@'])
                {
                    s.insert(0, '\'')
                };
                format!("\"{}\"", s.replace('"', "\"\""))
            })
            .collect::<Vec<_>>();
        out.push_str(&fields.join(";"));
        out.push_str("\r\n")
    }
    out.into_bytes()
}
pub fn xlsx(table: &Table) -> Result<Vec<u8>> {
    let mut z = zip::ZipWriter::new(Cursor::new(vec![]));
    let opt = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let sheet=format!("<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetViews><sheetView workbookViewId=\"0\"><pane ySplit=\"1\" topLeftCell=\"A2\" activePane=\"bottomLeft\" state=\"frozen\"/></sheetView></sheetViews><cols>{}</cols><sheetData>{}</sheetData><autoFilter ref=\"A1:{}{}\"/><pageSetup orientation=\"landscape\" paperSize=\"9\"/></worksheet>",table.columns.iter().enumerate().map(|(i,_)|format!("<col min=\"{}\" max=\"{}\" width=\"24\" customWidth=\"1\"/>",i+1,i+1)).collect::<String>(),std::iter::once(table.columns.iter().map(|c|json!(c.label)).collect::<Vec<_>>()).chain(table.rows.clone()).enumerate().map(|(r,vs)|format!("<row r=\"{}\">{}</row>",r+1,vs.iter().enumerate().map(|(i,v)|{let reference=format!("{}{}",col(i),r+1);if r>0&&v.is_number(){format!("<c r=\"{reference}\" s=\"{}\"><v>{}</v></c>",if table.columns[i].kind=="money"{2}else if table.columns[i].kind=="decimal"{3}else{0},v)}else{format!("<c r=\"{reference}\" t=\"inlineStr\" s=\"{}\">{}</c>",if r==0{1}else{0},cell_string(&text(v)))}}).collect::<String>())).collect::<String>(),col(table.columns.len().saturating_sub(1)),table.rows.len()+1);
    for (path,body) in [
 ("[Content_Types].xml",r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#.to_string()),
 ("_rels/.rels",r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.into()),
 ("xl/workbook.xml",r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Export" sheetId="1" r:id="rId1"/></sheets></workbook>"#.into()),
 ("xl/_rels/workbook.xml.rels",r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#.into()),
 ("xl/styles.xml",r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="1"><numFmt numFmtId="164" formatCode="0.###"/></numFmts><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><color rgb="FFFFFFFF"/><name val="Calibri"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFA51D32"/><bgColor indexed="64"/></patternFill></fill></fills><borders count="1"><border/></borders><cellStyleXfs count="1"><xf/></cellStyleXfs><cellXfs count="4"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"><alignment wrapText="1" vertical="top"/></xf><xf numFmtId="0" fontId="1" fillId="2" borderId="0" xfId="0"><alignment wrapText="1"/></xf><xf numFmtId="4" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="164" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#.into()),("xl/worksheets/sheet1.xml",sheet)]{z.start_file(path,opt)?;z.write_all(body.as_bytes())?;}
    Ok(z.finish()?.into_inner())
}
pub async fn log(c: &Core, t: &str, kind: &str, format: &str, rows: usize) -> Result<()> {
    let u = auth::require(c, t, None).await?;
    let mut tx = c.pool.begin().await?;
    audit::append(
        &mut tx,
        &c.key,
        Some(&u),
        "report.export",
        json!({"kind":kind,"format":format,"rows":rows}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}
