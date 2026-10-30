use super::*;
async fn building(c: &Core, t: &str, name: &str) -> String {
    dashboard::save(
        c,
        t,
        dashboard::BuildingInput {
            location: None,
            id: None,
            name: name.into(),
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
fn item(b: &str, cat: &str) -> inventory::Input {
    inventory::Input {
        building_id: b.into(),
        category_id: cat.into(),
        designation: "Compteur électrique".into(),
        condition: "good".into(),
        installed_on: Some("2024-01-31".into()),
        lifespan_months: Some(24),
        maintenance_interval_months: Some(1),
        purchase_cost_cents: Some(14500),
        reason: "Création équipement test".into(),
        ..Default::default()
    }
}
async fn read(c: &Core, t: &str, id: &str, date: &str, n: i64, reset: bool) -> anyhow::Result<()> {
    let d = inventory::detail(c, t, id).await?;
    inventory::reading(
        c,
        t,
        inventory::ReadingInput {
            equipment_id: id.into(),
            base_version: d["version"].as_i64().unwrap(),
            date: date.into(),
            quantity: n,
            unit: "kWh".into(),
            mode: "cumulative".into(),
            period_start: None,
            reset,
            reason: "Relevé compteur test".into(),
        },
    )
    .await
}
#[tokio::test]
async fn inventory_scope_history_dates_readings_and_exchange() {
    let root = std::env::temp_dir().join(format!("erp-m3-{}", db::id()));
    let c = initialise(root.clone()).await.unwrap();
    let admin = auth::bootstrap(
        &c,
        "Commune",
        "Admin",
        "admin",
        "Phrase administrateur test",
    )
    .await
    .unwrap();
    let t = &admin.token;
    let b = building(&c, t, "Gymnase").await;
    let other = building(&c, t, "Autre bâtiment").await;
    let cat: String = sqlx::query_scalar("SELECT id FROM equipment_categories WHERE code='ELEC'")
        .fetch_one(&c.pool)
        .await
        .unwrap();
    let level = inventory::location_add(&c, t, &b, None, "level", "RDC")
        .await
        .unwrap();
    let room = inventory::location_add(&c, t, &b, Some(level.clone()), "room", "Local compteur")
        .await
        .unwrap();
    assert!(inventory::location_add(
        &c,
        t,
        &other,
        Some(room.clone()),
        "room",
        "Croisement interdit"
    )
    .await
    .is_err());
    let mut i = item(&b, &cat);
    i.location_id = Some(room);
    let id = inventory::save(&c, t, i.clone()).await.unwrap();
    let d = inventory::detail(&c, t, &id).await.unwrap();
    assert_eq!(d["end_of_life_on"], "2026-01-31");
    assert_eq!(d["next_maintenance_on"], "2024-02-29");
    let asset = d["asset_key"].as_str().unwrap().to_owned();
    assert_eq!(d["purchase_cost_cents"], 14500);
    assert!(d["reference"].as_str().unwrap().starts_with("EQP-"));
    i.id = Some(id.clone());
    i.base_version = 0;
    assert!(inventory::save(&c, t, i.clone()).await.is_err());
    i.base_version = 1;
    i.building_id = other.clone();
    assert!(inventory::save(&c, t, i.clone()).await.is_err());
    read(&c, t, &id, "2024-02-01", 100000, false).await.unwrap();
    read(&c, t, &id, "2024-03-01", 130000, false).await.unwrap();
    assert!(read(&c, t, &id, "2024-04-01", 5000, false).await.is_err());
    read(&c, t, &id, "2024-04-01", 5000, true).await.unwrap();
    read(&c, t, &id, "2024-05-01", 15000, false).await.unwrap();
    let d = inventory::detail(&c, t, &id).await.unwrap();
    let r = d["readings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["reset"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let v = d["version"].as_i64().unwrap();
    assert!(
        inventory::reading_cancel(&c, t, &id, v, &r, "Annulation dangereuse")
            .await
            .is_err()
    );
    let last = d["readings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["date"] == "2024-05-01")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    inventory::reading_cancel(&c, t, &id, v, last, "Correction de relevé")
        .await
        .unwrap();
    read(&c, t, &id, "2024-05-01", 18000, false).await.unwrap();
    let d = inventory::detail(&c, t, &id).await.unwrap();
    inventory::maintenance(
        &c,
        t,
        &id,
        d["version"].as_i64().unwrap(),
        "2024-05-31".into(),
        None,
        None,
        "Entretien réalisé",
    )
    .await
    .unwrap();
    let d = inventory::detail(&c, t, &id).await.unwrap();
    assert_eq!(d["next_maintenance_on"], "2024-06-30");
    auth::create_user(
        &c,
        t,
        "Commission",
        "commission",
        "Phrase commission test",
        "commission",
    )
    .await
    .unwrap();
    let reader = auth::login(&c, "commission", "Phrase commission test", None)
        .await
        .unwrap();
    let red = inventory::detail(&c, &reader.token, &id).await.unwrap();
    assert!(red["purchase_cost_cents"].is_null());
    assert_eq!(red["can_write"], false);
    assert!(inventory::archive(
        &c,
        &reader.token,
        &id,
        d["version"].as_i64().unwrap(),
        true,
        "Interdit commission"
    )
    .await
    .is_err());
    let h = inventory::history(&c, &reader.token, &id, 0).await.unwrap();
    assert!(h["versions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|v| v["payload"]["purchase_cost_cents"].is_null()));
    dashboard::set_scope(&c, t, &reader.user.id, vec![other.clone()])
        .await
        .unwrap();
    assert!(inventory::detail(&c, &reader.token, &id).await.is_err());
    let result = inventory::search(&c, t, &format!("SERP1:{asset}"))
        .await
        .unwrap();
    assert_eq!(result[0]["id"], id);
    assert!(
        sqlx::query("UPDATE consumption_readings SET quantity=0 WHERE equipment_id=?")
            .bind(&id)
            .execute(&c.pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM equipment_events WHERE equipment_id=?")
            .bind(&id)
            .execute(&c.pool)
            .await
            .is_err()
    );
    let h = inventory::history(&c, t, &id, 0).await.unwrap();
    assert!(h["versions"].as_array().unwrap().len() > 5);
    audit::verify(&c.pool, &c.key, &c.root).await.unwrap();

    let wi=serde_json::from_value(serde_json::json!({"id":null,"building_id":b,"title":"Révision compteur","description":"Vérification","kind":"preventive","origin":"other","observation_id":null,"priority":2,"planned_start":null,"planned_end":null,"actual_start":null,"actual_end":null,"target_due_on":null,"estimated_budget_cents":10000,"progress_percent":0,"project_manager":"","architect":"","commission_authorisation":"","safety_impact":"","compensatory_measures":"","equipment_ids":[id],"predecessors":[],"participants":[],"contacts":[],"base_version":0,"reason":"Création intervention liée"})).unwrap();
    let before = inventory::detail(&c, t, &id).await.unwrap()["version"]
        .as_i64()
        .unwrap();
    let wid = works::save(&c, t, wi).await.unwrap();
    let after = inventory::detail(&c, t, &id).await.unwrap();
    assert_eq!(after["version"], before + 1);
    assert_eq!(after["works"][0]["id"], wid);
    let file = root.join("compteur.png");
    std::fs::write(&file, b"\x89PNG\r\n\x1a\nTest photo").unwrap();
    let d = inventory::detail(&c, t, &id).await.unwrap();
    inventory::photo_attach(&c, t, &id, d["version"].as_i64().unwrap(), &file)
        .await
        .unwrap();
    let d = inventory::detail(&c, t, &id).await.unwrap();
    let photo = d["photos"][0]["id"].as_str().unwrap().to_owned();
    inventory::photo_remove(
        &c,
        t,
        &id,
        d["version"].as_i64().unwrap(),
        &photo,
        "Retrait photo test",
    )
    .await
    .unwrap();
    assert!(inventory::photo(&c, t, &id, &photo).await.is_ok());
    let share = std::env::temp_dir().join(format!("m3-partage-{}.erp-partage", db::id()));
    let password = "Partage inventaire test 123!";
    exchange::export(&c, t, &b, password, &share).await.unwrap();
    let targetroot = std::env::temp_dir().join(format!("erp-m3-recu-{}", db::id()));
    let target = initialise(targetroot.clone()).await.unwrap();
    let receiver = auth::bootstrap(
        &target,
        "Autre commune",
        "Admin",
        "admin",
        "Phrase administrateur test",
    )
    .await
    .unwrap();
    let preview = exchange::preview(&target, &receiver.token, &share, password)
        .await
        .unwrap();
    let received = exchange::import(
        &target,
        &receiver.token,
        &share,
        password,
        &preview.digest,
        None,
        false,
    )
    .await
    .unwrap();
    let inv = inventory::snapshot(&target, &receiver.token).await.unwrap();
    let received_id = inv["records"][0]["id"].as_str().unwrap();
    assert_eq!(inv["records"][0]["asset_key"], asset);
    let detail = inventory::detail(&target, &receiver.token, received_id)
        .await
        .unwrap();
    assert_eq!(detail["readings"].as_array().unwrap().len(), 5);
    assert!(detail["photos"].as_array().unwrap().is_empty());
    let history = inventory::history(&target, &receiver.token, received_id, 0)
        .await
        .unwrap();
    let old_photo = history["versions"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|v| v["payload"]["photos"].get(0).and_then(|p| p["id"].as_str()))
        .unwrap();
    assert!(
        inventory::photo(&target, &receiver.token, received_id, old_photo)
            .await
            .is_ok()
    );
    assert_eq!(detail["building_id"], received);

    let d = inventory::detail(&c, t, &id).await.unwrap();
    inventory::maintenance(
        &c,
        t,
        &id,
        d["version"].as_i64().unwrap(),
        inventory::today(),
        None,
        None,
        "Nouvel entretien avant partage",
    )
    .await
    .unwrap();
    exchange::export(&c, t, &b, password, &share).await.unwrap();
    let preview = exchange::preview(&target, &receiver.token, &share, password)
        .await
        .unwrap();
    let received2 = exchange::import(
        &target,
        &receiver.token,
        &share,
        password,
        &preview.digest,
        preview.local_fingerprint.as_deref(),
        true,
    )
    .await
    .unwrap();
    assert_ne!(received, received2);
    let qr = inventory::search(&target, &receiver.token, &format!("SERP1:{asset}"))
        .await
        .unwrap();
    assert_eq!(qr.as_array().unwrap().len(), 1);
    let qid = qr[0]["id"].as_str().unwrap();
    assert_eq!(
        inventory::detail(&target, &receiver.token, qid)
            .await
            .unwrap()["building_id"],
        received2
    );
    std::fs::remove_file(share).unwrap();
    target.pool.close().await;
    std::fs::remove_dir_all(targetroot).unwrap();
    c.pool.close().await;
    std::fs::remove_dir_all(&root).unwrap();
}
