use super::*;
use serde_json::json;
async fn building(c: &Core, t: &str, n: &str) -> String {
    dashboard::save(
        c,
        t,
        dashboard::BuildingInput {
            location: None,
            id: None,
            name: n.into(),
            address: "Place centrale".into(),
            category: 5,
            capacity: 80,
            types: vec!["X".into()],
            manager: None,
            opened_on: None,
        },
    )
    .await
    .unwrap()
}
#[tokio::test]
async fn contracts_scope_visits_versions_preferences_exchange() {
    let root = std::env::temp_dir().join(format!("libre-m5-{}", db::id()));
    let c = initialise(root.clone()).await.unwrap();
    let a = auth::bootstrap(
        &c,
        "Commune",
        "Admin",
        "admin",
        "Phrase administrateur test",
    )
    .await
    .unwrap();
    let t = &a.token;
    let b = building(&c, t, "Gymnase").await;
    let other = building(&c, t, "École").await;
    let prefs = preferences::get(&c, t).await.unwrap();
    assert_eq!(prefs["features"]["qr"], false);
    let mut f = preferences::Features::default();
    f.qr = true;
    f.planning = true;
    preferences::save(&c, t, f).await.unwrap();
    assert_eq!(
        preferences::get(&c, t).await.unwrap()["features"]["qr"],
        true
    );
    let logo = root.join("logo.png");
    std::fs::write(&logo, b"\x89PNG\r\n\x1a\nlogo test").unwrap();
    preferences::logo(&c, t, Some(&logo)).await.unwrap();
    assert!(preferences::get(&c, t).await.unwrap()["logo"]
        .as_str()
        .unwrap()
        .starts_with("data:image/png;base64,"));
    let p=contracts::save(&c,t,true,json!({"name":"Entreprise test","registration_number":"123","email":"contact@example.test","phone":"0123","notes":"Contact maintenance","reason":"Création prestataire"})).await.unwrap();
    let mut input = json!({"subject":"Maintenance incendie","provider_id":p,"building_ids":[b],"equipment_ids":[],"starts_on":"2024-01-01","ends_on":"2028-12-31","notice_days":60,"cost_cents":12500,"cost_period":"annual","intervention_interval_months":1,"intervention_interval_days":null,"terms":"Contrat test","reason":"Création contrat"});
    let id = contracts::save(&c, t, false, input.clone()).await.unwrap();
    input["id"] = json!(id);
    input["base_version"] = json!(0);
    assert!(contracts::save(&c, t, false, input.clone()).await.is_err());
    contracts::action(&c,t,&p,true,1,"credential",json!({"kind":"insurance","title":"Responsabilité civile","issued_on":"2024-01-01","expires_on":"2026-01-01","reason":"Justificatif test"})).await.unwrap();
    contracts::action(
        &c,
        t,
        &id,
        false,
        1,
        "visit_plan",
        json!({"building_id":b,"day":"2024-01-31","count":3,"reason":"Planification test"}),
    )
    .await
    .unwrap();
    let s = contracts::snapshot(&c, t).await.unwrap();
    let row = &s["records"][0];
    assert_eq!(row["visits"][1]["planned_at"], "2024-02-29");
    assert_eq!(row["visits"][2]["planned_at"], "2024-03-31");
    assert_eq!(row["cost_cents"], 12500);
    let visit = row["visits"][0]["id"].as_str().unwrap().to_owned();
    assert!(contracts::action(
        &c,
        t,
        &id,
        false,
        2,
        "visit_plan",
        json!({"building_id":b,"day":"2024-02-29","count":1,"reason":"Doublon interdit"})
    )
    .await
    .is_err());
    contracts::action(&c,t,&id,false,2,"visit_status",json!({"visit_id":visit,"status":"completed","completed_at":"2024-01-31","notes":"Contrôle réalisé, aucune anomalie.","reason":"Réalisation passage"})).await.unwrap();
    let s = contracts::snapshot(&c, t).await.unwrap();
    assert_eq!(s["records"][0]["visits"][0]["status"], "completed");
    input["base_version"] = json!(3);
    contracts::save(&c, t, false, input.clone()).await.unwrap();
    let pdf = root.join("compte-rendu.pdf");
    std::fs::write(&pdf, b"%PDF-1.4\nTest").unwrap();
    contracts::attach(&c, t, &id, false, 4, Some(&visit), &pdf)
        .await
        .unwrap();
    let s = contracts::snapshot(&c, t).await.unwrap();
    let fid = s["records"][0]["files"][0]["id"].as_str().unwrap();
    assert_eq!(
        contracts::file(&c, t, &id, false, fid).await.unwrap().1,
        b"%PDF-1.4\nTest"
    );
    auth::create_user(
        &c,
        t,
        "Technique limité",
        "tech",
        "Phrase technique test",
        "technique",
    )
    .await
    .unwrap();
    let tech = auth::login(&c, "tech", "Phrase technique test", None)
        .await
        .unwrap();
    dashboard::set_scope(&c, t, &tech.user.id, vec![other.clone()])
        .await
        .unwrap();
    let tech = auth::login(&c, "tech", "Phrase technique test", None)
        .await
        .unwrap();
    assert!(
        contracts::snapshot(&c, &tech.token).await.unwrap()["records"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(contracts::file(&c, &tech.token, &id, false, fid)
        .await
        .is_err());
    assert!(
        preferences::save(&c, &tech.token, preferences::Features::default())
            .await
            .is_err()
    );
    auth::create_user(
        &c,
        t,
        "Prestataire",
        "prest",
        "Phrase prestataire test",
        "prestataire",
    )
    .await
    .unwrap();
    let prest = auth::login(&c, "prest", "Phrase prestataire test", None)
        .await
        .unwrap();
    assert!(
        contracts::snapshot(&c, &prest.token).await.unwrap()["records"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    contracts::save(&c,t,true,json!({"id":p,"base_version":2,"name":"Entreprise test","user_ids":[prest.user.id],"reason":"Association compte prestataire"})).await.unwrap();
    let view = contracts::snapshot(&c, &prest.token).await.unwrap();
    assert_eq!(view["records"][0]["cost_visible"], false);
    assert!(view["records"][0]["cost_cents"].is_null());
    assert!(view["records"][0]["terms"].is_null());
    assert!(contracts::action(
        &c,
        &prest.token,
        &id,
        false,
        5,
        "archive",
        json!({"archived":true,"reason":"Interdit prestataire"})
    )
    .await
    .is_err());
    assert!(contracts::history(&c, &prest.token, &id, false)
        .await
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .all(|h| h["payload"]["cost_cents"].is_null()));
    let share = std::env::temp_dir().join(format!("m5-{}.erp-partage", db::id()));
    exchange::export(&c, t, &b, "Partage contrats test 123!", &share)
        .await
        .unwrap();
    let tr = std::env::temp_dir().join(format!("m5-recu-{}", db::id()));
    let target = initialise(tr.clone()).await.unwrap();
    let ta = auth::bootstrap(
        &target,
        "Commune reçue",
        "Admin",
        "admin",
        "Phrase administrateur test",
    )
    .await
    .unwrap();
    let preview = exchange::preview(&target, &ta.token, &share, "Partage contrats test 123!")
        .await
        .unwrap();
    exchange::import(
        &target,
        &ta.token,
        &share,
        "Partage contrats test 123!",
        &preview.digest,
        None,
        false,
    )
    .await
    .unwrap();
    let s = contracts::snapshot(&target, &ta.token).await.unwrap();
    assert_eq!(s["records"].as_array().unwrap().len(), 1);
    assert_eq!(s["records"][0]["visits"].as_array().unwrap().len(), 3);
    assert_eq!(s["records"][0]["files"].as_array().unwrap().len(), 1);
    assert_eq!(
        s["providers"][0]["credentials"].as_array().unwrap().len(),
        1
    );
    input["base_version"] = json!(5);
    input["building_ids"] = json!([b, other]);
    contracts::save(&c, t, false, input.clone()).await.unwrap();
    assert!(
        contracts::snapshot(&c, &tech.token).await.unwrap()["records"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let refused = std::env::temp_dir().join(format!("m5-multiple-{}.erp-partage", db::id()));
    assert!(
        exchange::export(&c, t, &b, "Partage contrats test 123!", &refused)
            .await
            .is_err()
    );
    assert!(!refused.exists());
    input["base_version"] = json!(6);
    input["building_ids"] = json!([b]);
    contracts::save(&c, t, false, input.clone()).await.unwrap();
    preferences::logo(&c, t, None).await.unwrap();
    assert!(preferences::get(&c, t).await.unwrap()["logo"].is_null());
    audit::verify(&c.pool, &c.key, &c.root).await.unwrap();
    target.pool.close().await;
    std::fs::remove_dir_all(tr).unwrap();
    c.pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_file(share).unwrap();
}
