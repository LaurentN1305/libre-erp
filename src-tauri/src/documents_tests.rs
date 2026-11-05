use super::*;
use serde_json::json;
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
#[tokio::test]
async fn documents_versions_scope_financial_archive_and_exchange() {
    let root = std::env::temp_dir().join(format!("libre-m6-{}", db::id()));
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
    let other = building(&c, t, "École").await;
    let pdf = root.join("plan.pdf");
    std::fs::write(&pdf, b"%PDF-1.4\nVersion un").unwrap();
    documents::category_save(&c, t, None, "Plan municipal").await.unwrap();
    let category: String = sqlx::query_scalar("SELECT code FROM document_categories WHERE label='Plan municipal'")
        .fetch_one(&c.pool).await.unwrap();
    documents::category_save(&c, t, Some(&category), "Plan de sécurité").await.unwrap();
    let mut input = json!({"id":null,"building_id":b,"title":"Plan évacuation","category":category,"document_on":"2025-12-10","notes":"RDC","expires_on":"2026-01-01","retention_until":null,"restricted":false,"links":[b],"base_revision":0,"reason":"Création document"});
    let id = documents::save(&c, t, input.clone(), Some(&pdf))
        .await
        .unwrap();
    let d = documents::snapshot(&c, t).await.unwrap();
    assert_eq!(d["records"].as_array().unwrap().len(), 1);
    assert_eq!(d["records"][0]["document_on"], "2025-12-10");
    assert!(d["categories"].as_array().unwrap().iter().any(|c| c["code"] == "commission_report"));
    let f1 = d["records"][0]["file_id"].as_str().unwrap().to_owned();
    assert_eq!(
        documents::file(&c, t, &id, &f1, None).await.unwrap().1,
        b"%PDF-1.4\nVersion un"
    );
    input["id"] = json!(id);
    assert!(documents::save(&c, t, input.clone(), None).await.is_err());
    input["base_revision"] = json!(1);
    input["links"] = json!([other]);
    assert!(documents::save(&c, t, input.clone(), None).await.is_err());
    input["links"] = json!([b]);
    std::fs::write(&pdf, b"%PDF-1.4\nVersion deux").unwrap();
    documents::save(&c, t, input.clone(), Some(&pdf))
        .await
        .unwrap();
    input["base_revision"] = json!(2);
    input["notes"] = json!("Note corrigée");
    documents::save(&c, t, input.clone(), None).await.unwrap();
    let h = documents::history(&c, t, &id).await.unwrap();
    assert_eq!(h["versions"].as_array().unwrap().len(), 2);
    assert_eq!(h["edits"].as_array().unwrap().len(), 3);
    assert_eq!(
        documents::file(&c, t, &id, &f1, None).await.unwrap().1,
        b"%PDF-1.4\nVersion un"
    );
    assert!(
        sqlx::query("DELETE FROM document_versions WHERE document_id=?")
            .bind(&id)
            .execute(&c.pool)
            .await
            .is_err()
    );
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
    assert_eq!(
        documents::snapshot(&c, &reader.token).await.unwrap()["records"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(documents::save(&c, &reader.token, input.clone(), None)
        .await
        .is_err());
    let mut financial = input.clone();
    financial["id"] = json!(null);
    financial["base_revision"] = json!(0);
    financial["category"] = json!("invoice");
    financial["title"] = json!("Facture confidentielle");
    let secret = documents::save(&c, t, financial.clone(), Some(&pdf))
        .await
        .unwrap();
    let rows = documents::snapshot(&c, &reader.token).await.unwrap();
    assert_eq!(rows["records"].as_array().unwrap().len(), 1);
    assert!(documents::history(&c, &reader.token, &secret)
        .await
        .is_err());
    financial["id"] = json!(secret);
    financial["base_revision"] = json!(1);
    financial["category"] = json!("other");
    documents::save(&c, t, financial, None).await.unwrap();
    assert!(documents::access(&c, &reader.token, &secret, false)
        .await
        .is_err());
    dashboard::set_scope(&c, t, &reader.user.id, vec![other])
        .await
        .unwrap();
    let reader = auth::login(&c, "commission", "Phrase commission test", None)
        .await
        .unwrap();
    assert!(documents::file(&c, &reader.token, &id, &f1, None)
        .await
        .is_err());
    documents::archive(&c, t, &id, 3, true, "Archivage test")
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM deadlines WHERE entity_id=? AND completed_at IS NULL"
        )
        .bind(&id)
        .fetch_one(&c.pool)
        .await
        .unwrap(),
        0
    );
    documents::archive(&c, t, &id, 4, false, "Réactivation test")
        .await
        .unwrap();
    assert!(documents::trash(&c, t, &id, 4, true).await.is_err());
    documents::trash(&c, t, &id, 5, true).await.unwrap();
    let library = documents::snapshot(&c, t).await.unwrap();
    assert!(library["records"].as_array().unwrap().iter().any(|r| r["id"] == id && r["deleted"] == true));
    documents::trash(&c, t, &id, 6, false).await.unwrap();
    let org = preferences::org(&c, &admin.user.id).await.unwrap();
    let legacy = vault::put(
        &c,
        &org,
        &admin.user.id,
        "photo-conservee.png",
        "image/png",
        b"ancienne photo",
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO entity_files(entity_id,file_id,organisation_id,purpose) VALUES(?,?,?,'photo')",
    )
    .bind(&b)
    .bind(&legacy)
    .bind(&org)
    .execute(&c.pool)
    .await
    .unwrap();
    let library = documents::snapshot(&c, t).await.unwrap();
    assert!(library["records"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["legacy"] == true && d["file_id"] == legacy));
    assert_eq!(
        documents::file(&c, t, "legacy", &legacy, Some(&b))
            .await
            .unwrap()
            .1,
        b"ancienne photo"
    );
    assert!(
        documents::file(&c, &reader.token, "legacy", &legacy, Some(&b))
            .await
            .is_err()
    );
    auth::create_user(&c, t, "Agent", "agent", "Phrase agent test", "agent")
        .await
        .unwrap();
    let agent = auth::login(&c, "agent", "Phrase agent test", None)
        .await
        .unwrap();
    assert!(!dashboard::snapshot(&c, &agent.token)
        .await
        .unwrap()
        .to_string()
        .contains("Facture confidentielle"));
    let share = std::env::temp_dir().join(format!("m6-{}.erp-partage", db::id()));
    exchange::export(&c, t, &b, "Partage documents test 123!", &share)
        .await
        .unwrap();
    let tr = std::env::temp_dir().join(format!("m6-recu-{}", db::id()));
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
    let preview = exchange::preview(&target, &ta.token, &share, "Partage documents test 123!")
        .await
        .unwrap();
    exchange::import(
        &target,
        &ta.token,
        &share,
        "Partage documents test 123!",
        &preview.digest,
        None,
        false,
    )
    .await
    .unwrap();
    let rows = documents::snapshot(&target, &ta.token).await.unwrap();
    assert_eq!(rows["records"].as_array().unwrap().len(), 3);
    let received = rows["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["title"] == "Plan évacuation")
        .unwrap();
    assert_eq!(
        documents::history(&target, &ta.token, received["id"].as_str().unwrap())
            .await
            .unwrap()["versions"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(received["document_on"], "2025-12-10");
    assert_eq!(received["deleted"], false);
    assert!(rows["categories"].as_array().unwrap().iter().any(|c|
        c["code"] == received["category"] && c["label"] == "Plan de sécurité"));
    audit::verify(&c.pool, &c.key, &c.root).await.unwrap();
    target.pool.close().await;
    std::fs::remove_dir_all(tr).unwrap();
    c.pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_file(share).unwrap();
}
