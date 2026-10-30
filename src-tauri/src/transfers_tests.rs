use super::*;
use serde_json::json;
use std::io::Read;
async fn setup() -> (Core, String, String, String) {
    let c = initialise(std::env::temp_dir().join(format!("libre-m8-{}", db::id())))
        .await
        .unwrap();
    let s = auth::bootstrap(
        &c,
        "Commune",
        "Admin",
        "admin",
        "Phrase administrateur test",
    )
    .await
    .unwrap();
    let mut bs = vec![];
    for name in ["Gymnase", "École"] {
        bs.push(
            dashboard::save(
                &c,
                &s.token,
                dashboard::BuildingInput {
                    id: None,
                    name: name.into(),
                    address: "Place centrale".into(),
                    category: 5,
                    capacity: 60,
                    types: vec!["X".into(), "L".into()],
                    manager: None,
                    opened_on: None,
                    location: None,
                },
            )
            .await
            .unwrap(),
        );
    }
    (c, s.token, bs[0].clone(), bs[1].clone())
}
async fn count(c: &Core) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM equipment")
        .fetch_one(&c.pool)
        .await
        .unwrap()
}
async fn cleanup(c: Core) {
    let p = c.root.clone();
    c.pool.close().await;
    drop(c);
    std::fs::remove_dir_all(p).unwrap()
}
#[test]
fn csv_quotes_formulas_and_xlsx_types() {
    let data=csv_import::parse("\u{feff}designation;categorie;notes\r\n\"Équipement; A\";autre;\"Ligne 1\nLigne \"\"2\"\"\"\r\n").unwrap();
    assert_eq!(
        data[1],
        vec!["Équipement; A", "autre", "Ligne 1\nLigne \"2\""]
    );
    assert!(csv_import::parse("designation,categorie\n\"inachevé,autre").is_err());
    assert!(csv_import::parse("designation;categorie\n\"a\"x;b").is_err());
    let table = reports::Table {
        fingerprint: "test".into(),
        title: "Test".into(),
        generated_at: db::now(),
        note: "Essai".into(),
        columns: vec![
            reports::Column {
                key: "name".into(),
                label: "Nom".into(),
                kind: "text".into(),
            },
            reports::Column {
                key: "cost".into(),
                label: "Coût".into(),
                kind: "money".into(),
            },
        ],
        rows: vec![
            vec![
                json!("=HYPERLINK(\"https://example.invalid\")"),
                json!(-12.5),
            ],
            vec![json!("001234"), Value::Null],
            vec![json!("\t@SUM(1)"), json!(0)],
        ],
    };
    let csv = String::from_utf8(reports::csv(&table)).unwrap();
    assert!(csv.contains("\"'=HYPERLINK"));
    assert!(csv.contains("\"-12,5\""));
    assert!(csv.contains("'\t@SUM"));
    let xlsx = reports::xlsx(&table).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(&xlsx)).unwrap();
    let mut sheet = String::new();
    z.by_name("xl/worksheets/sheet1.xml")
        .unwrap()
        .read_to_string(&mut sheet)
        .unwrap();
    assert!(!sheet.contains("<f>"));
    assert!(sheet.contains("t=\"inlineStr\""));
    assert!(sheet.contains("001234"));
    assert!(sheet.contains("<v>-12.5</v>"));
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut xml = String::new();
        f.read_to_string(&mut xml).unwrap();
        assert!(!xml.is_empty())
    }
    if let Some(path) = std::env::var_os("ERP_TEST_XLSX") {
        std::fs::write(path, xlsx).unwrap()
    }
}
use serde_json::Value;
#[tokio::test]
async fn import_preview_atomicity_duplicates_and_permissions() {
    let (c, t, b, other) = setup().await;
    let category: String =
        sqlx::query_scalar("SELECT code FROM equipment_categories ORDER BY code LIMIT 1")
            .fetch_one(&c.pool)
            .await
            .unwrap();
    let header="designation;categorie;fabricant;numero_serie;cout_achat_euros;date_mise_service;entretien_mois;notes";
    let invalid=format!("{header}\nExtincteur;{category};Test;A;12,50;2020-01-01;12;Notes\nAutre;{category};Test;B;4,99;2999-01-01;12;Notes");
    let p = csv_import::preview(&c, &t, &b, &invalid).await.unwrap();
    assert_eq!(p.errors, 1);
    assert!(csv_import::apply(&c, &t, &b, &invalid, &p.seal)
        .await
        .is_err());
    assert_eq!(count(&c).await, 0);
    let good = invalid.replace("2999-01-01", "2020-01-02");
    let p = csv_import::preview(&c, &t, &b, &good).await.unwrap();
    assert_eq!(p.ready, 2);
    let r = csv_import::apply(&c, &t, &b, &good, &p.seal).await.unwrap();
    assert_eq!(r["added"], 2);
    assert_eq!(count(&c).await, 2);
    assert!(csv_import::apply(&c, &t, &b, &good, &p.seal).await.is_err());
    let p = csv_import::preview(&c, &t, &b, &good).await.unwrap();
    assert_eq!(p.ready, 0);
    assert_eq!(p.skipped, 2);
    let cost: i64 =
        sqlx::query_scalar("SELECT purchase_cost_cents FROM equipment WHERE serial_number='A'")
            .fetch_one(&c.pool)
            .await
            .unwrap();
    assert_eq!(cost, 1250);
    sqlx::query("CREATE TRIGGER reject_bad_import BEFORE INSERT ON equipment WHEN NEW.designation='Refuser' BEGIN SELECT RAISE(ABORT,'test panne');END").execute(&c.pool).await.unwrap();
    let fail = format!("designation;categorie\nNouveau;{category}\nRefuser;{category}");
    let p = csv_import::preview(&c, &t, &b, &fail).await.unwrap();
    assert_eq!(p.ready, 2);
    assert!(csv_import::apply(&c, &t, &b, &fail, &p.seal).await.is_err());
    assert_eq!(count(&c).await, 2);
    sqlx::query("DROP TRIGGER reject_bad_import")
        .execute(&c.pool)
        .await
        .unwrap();
    auth::create_user(
        &c,
        &t,
        "Technique",
        "tech",
        "Phrase technique test",
        "technique",
    )
    .await
    .unwrap();
    let s = auth::login(&c, "tech", "Phrase technique test", None)
        .await
        .unwrap();
    dashboard::set_scope(&c, &t, &s.user.id, vec![other.clone()])
        .await
        .unwrap();
    let s = auth::login(&c, "tech", "Phrase technique test", None)
        .await
        .unwrap();
    assert!(csv_import::preview(&c, &s.token, &b, &good).await.is_err());
    assert!(reports::table(&c, &s.token, "inventory", None, false)
        .await
        .unwrap()
        .rows
        .is_empty());
    auth::create_user(&c, &t, "Agent", "agent", "Phrase agent exemple", "agent")
        .await
        .unwrap();
    let s = auth::login(&c, "agent", "Phrase agent exemple", None)
        .await
        .unwrap();
    assert!(reports::table(&c, &s.token, "inventory", Some(&b), false)
        .await
        .is_err());
    assert!(csv_import::preview(&c, &s.token, &b, &good).await.is_err());
    auth::create_user(
        &c,
        &t,
        "Sécurité",
        "sec",
        "Phrase securite test",
        "securite",
    )
    .await
    .unwrap();
    sqlx::query("DELETE FROM role_permissions WHERE role_code='securite' AND permission_code='inventory.cost.read'").execute(&c.pool).await.unwrap();
    let sec = auth::login(&c, "sec", "Phrase securite test", None)
        .await
        .unwrap();
    dashboard::set_scope(&c, &t, &sec.user.id, vec![b.clone()])
        .await
        .unwrap();
    let sec = auth::login(&c, "sec", "Phrase securite test", None)
        .await
        .unwrap();
    let export = reports::table(&c, &sec.token, "inventory", Some(&b), false)
        .await
        .unwrap();
    assert_eq!(export.rows.len(), 2);
    assert!(!export.columns.iter().any(|c| c.kind == "money"));
    let eq = inventory::snapshot(&c, &t).await.unwrap()["records"][0].clone();
    inventory::reading(
        &c,
        &t,
        inventory::ReadingInput {
            equipment_id: eq["id"].as_str().unwrap().into(),
            base_version: eq["version"].as_i64().unwrap(),
            date: works::today(),
            quantity: 1234567,
            unit: "kWh".into(),
            mode: "cumulative".into(),
            period_start: None,
            reset: false,
            reason: "Relevé de test".into(),
        },
    )
    .await
    .unwrap();
    let cons = reports::table(&c, &t, "consumption", Some(&b), false)
        .await
        .unwrap();
    assert_eq!(cons.rows.len(), 1);
    assert!(cons.rows[0].iter().any(|v| v == "Index cumulatif"));
    assert!(cons.rows[0].iter().any(|v| v == &json!(1234.567)));
    for kind in ["works", "register", "contracts", "documents", "alerts"] {
        reports::table(&c, &t, kind, Some(&b), false).await.unwrap();
    }
    let bs = reports::table(&c, &t, "buildings", Some(&b), false)
        .await
        .unwrap();
    assert_eq!(bs.rows.len(), 1);
    assert!(bs.rows[0].iter().any(|v| v == "L / X" || v == "X / L"));
    assert!(audit::verify(&c.pool, &c.key, &c.root).await.is_ok());
    cleanup(c).await;
}
