use super::*;
#[tokio::test]
async fn register_versioning_scope_and_multiple_types() {
    let root = std::env::temp_dir().join(format!("erp-m2-{}", db::id()));
    let core = initialise(root.clone()).await.unwrap();
    let admin = auth::bootstrap(
        &core,
        "Mairie",
        "Admin",
        "admin",
        "Phrase administrateur test",
    )
    .await
    .unwrap();
    let building = dashboard::save(
        &core,
        &admin.token,
        dashboard::BuildingInput {
            location: None,
            id: None,
            name: "Salle polyvalente".into(),
            address: "Place centrale".into(),
            category: 5,
            capacity: 100,
            types: vec!["X".into(), "L".into()],
            manager: None,
            opened_on: Some("2001-09-01".into()),
        },
    )
    .await
    .unwrap();
    let d = dashboard::snapshot(&core, &admin.token).await.unwrap();
    assert_eq!(d["buildings"][0]["types"], serde_json::json!(["X", "L"]));
    assert_eq!(d["buildings"][0]["opened_on"], "2001-09-01");
    assert_eq!(
        register::snapshot(&core, &admin.token, &building)
            .await
            .unwrap()["building"]["opened_on"],
        "2001-09-01"
    );
    let mut ids = vec![];
    for block in register::schema().as_array().unwrap() {
        let fields = block["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                let key = f["key"].as_str().unwrap().to_string();
                let val = if key == "type_controle" {
                    "commission"
                } else if key == "suivi" {
                    "none"
                } else if key == "statut" {
                    "actif"
                } else {
                    match f["kind"].as_str().unwrap() {
                        "date" => "2026-10-29",
                        "number" => "1",
                        _ => "Renseignement test",
                    }
                };
                (key, val.into())
            })
            .collect();
        let input = register::Input {
            id: None,
            building_id: building.clone(),
            block: block["code"].as_str().unwrap().into(),
            title: format!("Entrée {}", block["code"]),
            effective_on: "2026-10-29".into(),
            applicability: "applicable".into(),
            justification: String::new(),
            fields,
            files: vec![],
            base_version: 0,
            reason: "Création".into(),
        };
        let id = register::save(&core, &admin.token, input.clone())
            .await
            .unwrap();
        let mut edited = input.clone();
        edited.id = Some(id.clone());
        edited.base_version = 1;
        edited.title = "Entrée corrigée".into();
        edited.reason = "Correction documentée".into();
        register::save(&core, &admin.token, edited.clone())
            .await
            .unwrap();
        assert!(register::save(&core, &admin.token, edited).await.is_err());
        let h = register::history(&core, &admin.token, &id).await.unwrap();
        assert_eq!(h.as_array().unwrap().len(), 2);
        assert_eq!(h[1]["payload"]["title"], input.title);
        ids.push(id);
    }

    let c = &ids[2];
    let history = register::history(&core, &admin.token, c).await.unwrap();
    let mut followup: register::Input =
        serde_json::from_value(history[0]["payload"].clone()).unwrap();
    followup.id = Some(c.clone());
    followup.base_version = 2;
    followup.reason = "Prescription ouverte".into();
    followup.fields.insert("suivi".into(), "open".into());
    followup.fields.insert("gravite".into(), "4".into());
    register::save(&core, &admin.token, followup.clone())
        .await
        .unwrap();
    let observation=sqlx::query("SELECT o.status,o.severity,o.visit_id FROM observations o JOIN register_observations r ON r.observation_id=o.id WHERE r.entry_id=?").bind(c).fetch_one(&core.pool).await.unwrap();
    assert_eq!(observation.get::<String, _>(0), "open");
    assert_eq!(observation.get::<i64, _>(1), 4);
    assert!(observation.get::<Option<String>, _>(2).is_some());
    assert!(register::archive(&core, &admin.token, c, true)
        .await
        .is_err());
    followup.base_version = 3;
    followup.reason = "Prescription levée".into();
    followup.fields.insert("suivi".into(), "resolved".into());
    register::save(&core, &admin.token, followup).await.unwrap();
    let image = root.join("annexe.png");
    use base64::Engine;
    let bytes=base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a1ZkAAAAASUVORK5CYII=").unwrap();
    std::fs::write(&image, &bytes).unwrap();
    register::attach(&core, &admin.token, c, &image)
        .await
        .unwrap();
    let snapshot = register::snapshot(&core, &admin.token, &building)
        .await
        .unwrap();
    let file = snapshot["files"][0]["id"].as_str().unwrap().to_string();
    let attachment = register::attachment(&core, &admin.token, &building, &file)
        .await
        .unwrap();
    assert_eq!(attachment["mime"], "image/png");
    assert_ne!(
        std::fs::read(root.join("files").join(format!("{file}.blob"))).unwrap(),
        bytes
    );
    let s = register::snapshot(&core, &admin.token, &building)
        .await
        .unwrap();
    assert_eq!(s["entries"].as_array().unwrap().len(), 9);
    auth::create_user(
        &core,
        &admin.token,
        "Commission",
        "commission",
        "Phrase commission test",
        "commission",
    )
    .await
    .unwrap();
    let reader = auth::login(&core, "commission", "Phrase commission test", None)
        .await
        .unwrap();
    assert_eq!(
        register::snapshot(&core, &reader.token, &building)
            .await
            .unwrap()["writable"],
        false
    );
    assert!(register::archive(&core, &reader.token, &ids[0], true)
        .await
        .is_err());
    let history = register::history(&core, &reader.token, &ids[0])
        .await
        .unwrap();
    assert_eq!(history.as_array().unwrap().len(), 2);
    assert!(
        sqlx::query("UPDATE entity_versions SET reason='falsifié' WHERE entity_id=?")
            .bind(&ids[0])
            .execute(&core.pool)
            .await
            .is_err()
    );
    register::archive(&core, &admin.token, &ids[0], true)
        .await
        .unwrap();
    let count_before: i64 = sqlx::query_scalar("SELECT count(*) FROM files")
        .fetch_one(&core.pool)
        .await
        .unwrap();
    assert!(register::attach(&core, &admin.token, &ids[0], &image)
        .await
        .is_err());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM files")
            .fetch_one(&core.pool)
            .await
            .unwrap(),
        count_before
    );
    register::archive(&core, &admin.token, &ids[0], false)
        .await
        .unwrap();

    let other = dashboard::save(
        &core,
        &admin.token,
        dashboard::BuildingInput {
            location: None,
            id: None,
            name: "Autre bâtiment".into(),
            address: "Autre adresse".into(),
            category: 5,
            capacity: 20,
            types: vec!["W".into()],
            manager: None,
            opened_on: Some("2001-09-01".into()),
        },
    )
    .await
    .unwrap();
    assert!(register::attachment(&core, &reader.token, &other, &file)
        .await
        .is_err());
    dashboard::set_scope(&core, &admin.token, &reader.user.id, vec![other.clone()])
        .await
        .unwrap();
    let scoped = auth::login(&core, "commission", "Phrase commission test", None)
        .await
        .unwrap();
    assert!(register::snapshot(&core, &scoped.token, &building)
        .await
        .is_err());
    assert!(register::history(&core, &scoped.token, &ids[0])
        .await
        .is_err());
    assert_eq!(
        register::search(&core, &scoped.token, "Renseignement")
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        0
    );
    audit::verify(&core.pool, &core.key, &core.root)
        .await
        .unwrap();
    core.pool.close().await;
    drop(core);
    db::ensure_within(&root, &std::env::temp_dir()).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
