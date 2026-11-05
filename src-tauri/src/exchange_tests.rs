use super::*;
use crate::{auth, dashboard, initialise, works};
async fn setup() -> (Core, String) {
    let r = std::env::temp_dir().join(format!("erp-echange-{}", db::id()));
    let c = initialise(r).await.unwrap();
    let s = auth::bootstrap(
        &c,
        "Commune test",
        "Admin test",
        "admin",
        "Phrase administrateur test",
    )
    .await
    .unwrap();
    (c, s.token)
}
async fn building(c: &Core, t: &str, name: &str) -> String {
    dashboard::save(
        c,
        t,
        dashboard::BuildingInput {
            location: Some(crate::maps::Location {
                lat: 48.8566,
                lon: 2.3522,
                source_address: "Place centrale".into(),
                plan: Some(crate::maps::Plan {
                    source: "ign".into(),
                    bounds: crate::maps::bounds(48.8566, 2.3522),
                    features: vec![crate::maps::Feature {
                        road_class: None,
                        holes: vec![],
                        kind: "road".into(),
                        name: "Rue exemple".into(),
                        points: vec![[2.3521, 48.8565], [2.3523, 48.8567]],
                    }],
                    obtained_at: db::now(),
                    truncated: false,
                }),
            }),
            id: None,
            name: name.into(),
            address: "Place centrale".into(),
            category: 5,
            capacity: 80,
            types: vec!["X".into(), "L".into()],
            manager: None,
            opened_on: Some("2001-09-01".into()),
        },
    )
    .await
    .unwrap()
}
async fn work(c: &Core, t: &str, b: &str) -> String {
    let input=serde_json::from_value(json!({"id":null,"building_id":b,"title":"Changer la chasse d’eau","description":"Plomberie","kind":"corrective","origin":"failure","observation_id":null,"priority":2,"planned_start":null,"planned_end":null,"actual_start":null,"actual_end":null,"target_due_on":null,"estimated_budget_cents":10000,"progress_percent":0,"project_manager":"","architect":"","commission_authorisation":"","safety_impact":"","compensatory_measures":"","equipment_ids":[],"predecessors":[],"participants":[],"contacts":[],"base_version":0,"reason":"Création test"})).unwrap();
    works::save(c, t, input).await.unwrap()
}
async fn revision(c: &Core, id: &str) -> i64 {
    sqlx::query_scalar("SELECT revision FROM works WHERE id=?")
        .bind(id)
        .fetch_one(&c.pool)
        .await
        .unwrap()
}
#[tokio::test]
async fn encrypted_building_exchange_preserves_history_files_and_conflicts() {
    let (source, st) = setup().await;
    let b = building(&source, &st, "Gymnase partagé").await;
    let other = building(&source, &st, "Ne pas partager").await;
    let w = work(&source, &st, &b).await;
    let fake = source.root.join("fixture.pdf");
    std::fs::write(&fake, b"%PDF-1.4\nDocument test").unwrap();
    works::attach(
        &source,
        &st,
        &w,
        revision(&source, &w).await,
        "report",
        &fake,
    )
    .await
    .unwrap();
    works::simple_transition(
        &source,
        &st,
        &w,
        revision(&source, &w).await,
        false,
        "Début intervention",
    )
    .await
    .unwrap();
    works::cost(
        &source,
        &st,
        &w,
        revision(&source, &w).await,
        "Pièce de réparation",
        5500,
        &works::today(),
        None,
        "Dépense test",
    )
    .await
    .unwrap();
    works::simple_transition(
        &source,
        &st,
        &w,
        revision(&source, &w).await,
        true,
        "Intervention réalisée",
    )
    .await
    .unwrap();
    let fields = crate::register::schema()[0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let key = f["key"].as_str().unwrap().to_owned();
            let val = match f["kind"].as_str().unwrap() {
                "date" => "2026-10-29",
                "number" => "1",
                _ => "Identification test",
            };
            (key, val.to_owned())
        })
        .collect();
    let entry = crate::register::Input {
        id: None,
        building_id: b.clone(),
        block: "A".into(),
        title: "Identification partagée".into(),
        effective_on: "2026-10-29".into(),
        applicability: "applicable".into(),
        justification: String::new(),
        fields,
        files: vec![],
        base_version: 0,
        reason: "Création test".into(),
    };
    let entry_id = crate::register::save(&source, &st, entry.clone())
        .await
        .unwrap();
    crate::register::attach(&source, &st, &entry_id, &fake)
        .await
        .unwrap();
    let path = std::env::temp_dir().join(format!("{}.erp-partage", db::id()));
    let pw = "Partage bâtiment test 123!";
    export(&source, &st, &b, pw, &path).await.unwrap();
    let package = read(&path, pw).unwrap();
    assert!(!package.tables["entities"].iter().any(|r| r["id"] == other));
    assert!(!String::from_utf8_lossy(&std::fs::read(&path).unwrap()).contains("Gymnase"));
    assert!(read(&path, "Mauvais mot de passe").is_err());
    assert_eq!(package.files.len(), 2);
    let (target, tt) = setup().await;
    let unrelated = building(&target, &tt, "Bâtiment local").await;
    let p = preview(&target, &tt, &path, pw).await.unwrap();
    assert!(p.existing.is_none());
    let imported = import(&target, &tt, &path, pw, &p.digest, None, false)
        .await
        .unwrap();
    let names = sqlx::query_scalar::<_, String>("SELECT name FROM buildings")
        .fetch_all(&target.pool)
        .await
        .unwrap();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"Bâtiment local".into()));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT current_version FROM register_entries WHERE building_id=?"
        )
        .bind(&imported)
        .fetch_one(&target.pool)
        .await
        .unwrap(),
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM building_erp_types WHERE building_id=?")
            .bind(&imported)
            .fetch_one(&target.pool)
            .await
            .unwrap(),
        2
    );
    let loc: String = sqlx::query_scalar("SELECT location_json FROM buildings WHERE id=?")
        .bind(&imported)
        .fetch_one(&target.pool)
        .await
        .unwrap();
    let loc: crate::maps::Location = serde_json::from_str(&loc).unwrap();
    assert_eq!(loc.lat, 48.8566);
    assert_eq!(loc.plan.as_ref().unwrap().source, "ign");
    assert_eq!(loc.plan.unwrap().features[0].name, "Rue exemple");
    let incoming =
        sqlx::query("SELECT id,status,validated_by,revision FROM works WHERE building_id=?")
            .bind(&imported)
            .fetch_one(&target.pool)
            .await
            .unwrap();
    assert_eq!(incoming.get::<String, _>(1), "closed");
    let validator: String = incoming.get(2);
    let active: i64 = sqlx::query_scalar("SELECT active FROM users WHERE id=?")
        .bind(&validator)
        .fetch_one(&target.pool)
        .await
        .unwrap();
    assert_eq!(active, 0);
    assert!(auth::set_active(&target, &tt, &validator, true)
        .await
        .is_err());
    assert!(!auth::list_users(&target, &tt)
        .await
        .unwrap()
        .iter()
        .any(|u| u.id == validator));
    let file = sqlx::query_scalar::<_, String>("SELECT file_id FROM work_documents")
        .fetch_one(&target.pool)
        .await
        .unwrap();
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM buildings WHERE id=?")
        .bind(&imported)
        .fetch_one(&target.pool)
        .await
        .unwrap();
    assert_eq!(
        &*vault::read(&target, &file, &org).await.unwrap(),
        b"%PDF-1.4\nDocument test"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT sum(amount_cents) FROM work_costs")
            .fetch_one(&target.pool)
            .await
            .unwrap(),
        5500
    );
    let again = preview(&target, &tt, &path, pw).await.unwrap();
    assert!(again.already_received);
    assert!(import(
        &target,
        &tt,
        &path,
        pw,
        &again.digest,
        again.local_fingerprint.as_deref(),
        true
    )
    .await
    .is_err());
    // Une nouvelle version de l’émetteur ne remplace jamais silencieusement la version locale.
    sqlx::query("UPDATE buildings SET name='Gymnase mis à jour' WHERE id=?")
        .bind(&b)
        .execute(&source.pool)
        .await
        .unwrap();
    export(&source, &st, &b, pw, &path).await.unwrap();
    let p = preview(&target, &tt, &path, pw).await.unwrap();
    assert!(!p.already_received);
    assert!(!p.local_changed);
    sqlx::query("UPDATE buildings SET address='Modification locale' WHERE id=?")
        .bind(&imported)
        .execute(&target.pool)
        .await
        .unwrap();
    assert!(import(
        &target,
        &tt,
        &path,
        pw,
        &p.digest,
        p.local_fingerprint.as_deref(),
        true
    )
    .await
    .is_err());
    let p = preview(&target, &tt, &path, pw).await.unwrap();
    assert!(p.local_changed);
    assert!(import(
        &target,
        &tt,
        &path,
        pw,
        &p.digest,
        p.local_fingerprint.as_deref(),
        false
    )
    .await
    .is_err());
    let latest = import(
        &target,
        &tt,
        &path,
        pw,
        &p.digest,
        p.local_fingerprint.as_deref(),
        true,
    )
    .await
    .unwrap();
    assert_ne!(latest, imported);
    let old: Option<String> = sqlx::query_scalar("SELECT archived_at FROM entities WHERE id=?")
        .bind(&imported)
        .fetch_one(&target.pool)
        .await
        .unwrap();
    assert!(old.is_some());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM exchange_receipts")
            .fetch_one(&target.pool)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM buildings WHERE id=?")
            .bind(unrelated)
            .fetch_one(&target.pool)
            .await
            .unwrap(),
        1
    );
    db::check(&target.pool).await.unwrap();
    audit::verify(&target.pool, &target.key, &target.root)
        .await
        .unwrap();
    let sr = source.root.clone();
    let tr = target.root.clone();
    source.pool.close().await;
    target.pool.close().await;
    drop(source);
    drop(target);
    std::fs::remove_dir_all(sr).unwrap();
    std::fs::remove_dir_all(tr).unwrap();
    std::fs::remove_file(path).unwrap();
}
#[tokio::test]
async fn simple_actions_keep_validation_permissions() {
    let (c, t) = setup().await;
    let b = building(&c, &t, "Atelier").await;
    let w = work(&c, &t, &b).await;
    assert!(
        works::simple_transition(&c, &t, &w, revision(&c, &w).await, true, "Fin trop tôt")
            .await
            .is_err()
    );
    works::simple_transition(
        &c,
        &t,
        &w,
        revision(&c, &w).await,
        false,
        "Début sans planning",
    )
    .await
    .unwrap();
    assert!(
        works::simple_transition(&c, &t, &w, 0, true, "Version périmée")
            .await
            .is_err()
    );
    // Retirer la validation au rôle technique pour vérifier le chemin de réalisation sans clôture.
    auth::create_user(
        &c,
        &t,
        "Agent technique",
        "tech",
        "Phrase technique test 123",
        "technique",
    )
    .await
    .unwrap();
    sqlx::query("DELETE FROM role_permissions WHERE role_code='technique' AND permission_code='work.validate'").execute(&c.pool).await.unwrap();
    let s = auth::login(&c, "tech", "Phrase technique test 123", None)
        .await
        .unwrap();
    works::simple_transition(
        &c,
        &s.token,
        &w,
        revision(&c, &w).await,
        true,
        "Réparation réalisée",
    )
    .await
    .unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM works WHERE id=?")
        .bind(&w)
        .fetch_one(&c.pool)
        .await
        .unwrap();
    assert_eq!(status, "pending_validation");
    assert!(works::simple_transition(
        &c,
        &s.token,
        &w,
        revision(&c, &w).await,
        true,
        "Validation interdite"
    )
    .await
    .is_err());
    works::simple_transition(
        &c,
        &t,
        &w,
        revision(&c, &w).await,
        true,
        "Validation responsable",
    )
    .await
    .unwrap();
    let r = c.root.clone();
    c.pool.close().await;
    drop(c);
    std::fs::remove_dir_all(r).unwrap();
}
