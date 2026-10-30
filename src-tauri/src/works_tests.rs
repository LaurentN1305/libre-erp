use super::*;
fn input(building: &str) -> works::Input {
    works::Input {
        id: None,
        building_id: building.into(),
        title: "Réparation porte".into(),
        description: "Réglage de la fermeture".into(),
        kind: "corrective".into(),
        origin: "failure".into(),
        observation_id: None,
        priority: 2,
        planned_start: Some(works::today()),
        planned_end: Some(works::today()),
        actual_start: None,
        actual_end: None,
        target_due_on: None,
        estimated_budget_cents: 15000,
        progress_percent: 0,
        project_manager: "Responsable technique".into(),
        architect: String::new(),
        commission_authorisation: String::new(),
        safety_impact: "Accès maintenu".into(),
        compensatory_measures: "Balisage".into(),
        equipment_ids: vec![],
        predecessors: vec![],
        participants: vec![],
        contacts: vec![],
        base_version: 0,
        reason: "Création test".into(),
    }
}
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
            types: vec!["X".into(), "L".into()],
            manager: None,
            opened_on: None,
        },
    )
    .await
    .unwrap()
}
#[tokio::test]
async fn workflow_costs_scope_and_documents() {
    let root = std::env::temp_dir().join(format!("erp-m4-{}", db::id()));
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
    let b = building(&c, &a.token, "Gymnase").await;
    let other = building(&c, &a.token, "Autre").await;
    let id = works::save(&c, &a.token, input(&b)).await.unwrap();
    assert!(
        works::transition(&c, &a.token, &id, 1, "closed", "Clôture directe interdite")
            .await
            .is_err()
    );
    let mut edit = input(&b);
    edit.id = Some(id.clone());
    edit.base_version = 0;
    assert!(works::save(&c, &a.token, edit).await.is_err());
    works::transition(&c, &a.token, &id, 1, "scheduled", "Programmé")
        .await
        .unwrap();
    works::transition(&c, &a.token, &id, 2, "in_progress", "Démarrage")
        .await
        .unwrap();
    works::cost(
        &c,
        &a.token,
        &id,
        3,
        "Facture test",
        12550,
        &works::today(),
        None,
        "Dépense réelle",
    )
    .await
    .unwrap();
    let s = works::snapshot(&c, &a.token).await.unwrap();
    assert_eq!(s["records"][0]["actual_budget_cents"], 12550);
    let cost = s["records"][0]["costs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    works::reverse_cost(&c, &a.token, &id, 4, &cost, "Erreur de facture")
        .await
        .unwrap();
    assert!(
        works::reverse_cost(&c, &a.token, &id, 5, &cost, "Deuxième annulation")
            .await
            .is_err()
    );
    assert!(sqlx::query("DELETE FROM work_costs WHERE work_id=?")
        .bind(&id)
        .execute(&c.pool)
        .await
        .is_err());
    works::transition(
        &c,
        &a.token,
        &id,
        5,
        "pending_validation",
        "Terminé, à vérifier",
    )
    .await
    .unwrap();
    works::transition(&c, &a.token, &id, 6, "closed", "Vérification réalisée")
        .await
        .unwrap();
    works::transition(&c, &a.token, &id, 7, "archived", "Dossier complet")
        .await
        .unwrap();
    works::transition(&c, &a.token, &id, 8, "closed", "Réactivation dossier")
        .await
        .unwrap();
    let s = works::snapshot(&c, &a.token).await.unwrap();
    assert_eq!(s["records"][0]["actual_budget_cents"], 0);
    assert_eq!(s["records"][0]["progress_percent"], 100);
    auth::create_user(
        &c,
        &a.token,
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
    let view = works::snapshot(&c, &reader.token).await.unwrap();
    assert!(view["records"][0].get("estimated_budget_cents").is_none());
    assert_eq!(view["records"][0]["can_write"], false);
    assert!(
        works::transition(&c, &reader.token, &id, 9, "in_progress", "Interdit")
            .await
            .is_err()
    );
    let path = root.join("facture.pdf");
    std::fs::write(&path, b"%PDF-1.7\nTest").unwrap();
    works::attach(&c, &a.token, &id, 9, "invoice", &path)
        .await
        .unwrap();
    let s = works::snapshot(&c, &a.token).await.unwrap();
    let file = s["records"][0]["files"][0]["id"].as_str().unwrap();
    assert!(works::file(&c, &reader.token, &id, file).await.is_err());
    dashboard::set_scope(&c, &a.token, &reader.user.id, vec![other.clone()])
        .await
        .unwrap();
    let reader = auth::login(&c, "commission", "Phrase commission test", None)
        .await
        .unwrap();
    assert!(works::history(&c, &reader.token, &id).await.is_err());
    assert_eq!(
        works::search(&c, &reader.token, "porte")
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        0
    );
    auth::create_user(
        &c,
        &a.token,
        "Entreprise",
        "prestataire",
        "Phrase prestataire test",
        "prestataire",
    )
    .await
    .unwrap();
    let provider = auth::login(&c, "prestataire", "Phrase prestataire test", None)
        .await
        .unwrap();
    assert!(
        works::snapshot(&c, &provider.token).await.unwrap()["records"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut assigned = input(&other);
    assigned.title = "Mission affectée".into();
    assigned.participants.push(works::Participant {
        user_id: Some(provider.user.id.clone()),
        provider_id: None,
        role: "Intervenant".into(),
    });
    let assigned_id = works::save(&c, &a.token, assigned).await.unwrap();
    let view = works::snapshot(&c, &provider.token).await.unwrap();
    assert_eq!(view["records"].as_array().unwrap().len(), 1);
    assert_eq!(view["records"][0]["id"], assigned_id);
    assert!(view["records"][0].get("costs").is_none());
    assert!(works::access(&c, &provider.token, &id, false)
        .await
        .is_err());
    audit::verify(&c.pool, &c.key, &c.root).await.unwrap();
    c.pool.close().await;
    drop(c);
    db::ensure_within(&root, &std::env::temp_dir()).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn dependency_cycles_and_cross_building_are_rejected() {
    let root = std::env::temp_dir().join(format!("erp-m4-deps-{}", db::id()));
    let c = initialise(root.clone()).await.unwrap();
    let a = auth::bootstrap(&c, "Org", "Admin", "admin", "Phrase administrateur test")
        .await
        .unwrap();
    let b = building(&c, &a.token, "A").await;
    let other = building(&c, &a.token, "B").await;
    let first = works::save(&c, &a.token, input(&b)).await.unwrap();
    let mut next = input(&b);
    next.predecessors = vec![first.clone()];
    let second = works::save(&c, &a.token, next).await.unwrap();
    let mut cycle = input(&b);
    cycle.id = Some(first.clone());
    cycle.base_version = 1;
    cycle.predecessors = vec![second];
    assert!(works::save(&c, &a.token, cycle).await.is_err());
    let mut cross = input(&other);
    cross.predecessors = vec![first];
    assert!(works::save(&c, &a.token, cross).await.is_err());
    c.pool.close().await;
    drop(c);
    db::ensure_within(&root, &std::env::temp_dir()).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn commission_sync_is_idempotent_and_system_identity_is_disabled() {
    let root = std::env::temp_dir().join(format!("erp-m4-sync-{}", db::id()));
    let c = initialise(root.clone()).await.unwrap();
    let a = auth::bootstrap(&c, "Org", "Admin", "admin", "Phrase administrateur test")
        .await
        .unwrap();
    let b = building(&c, &a.token, "Salle").await;
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
        .bind(&a.user.id)
        .fetch_one(&c.pool)
        .await
        .unwrap();
    let visit = db::id();
    let obs = db::id();
    sqlx::query("INSERT INTO commission_visits(id,building_id,visited_on,commission_name,opinion) VALUES(?,?,'2026-10-29','Commission','Prescription')").bind(&visit).bind(&b).execute(&c.pool).await.unwrap();
    sqlx::query("INSERT INTO entities(id,organisation_id,building_id,kind,created_at,created_by) VALUES(?,?,?,'observation',?,?)").bind(&obs).bind(&org).bind(&b).bind(db::now()).bind(&a.user.id).execute(&c.pool).await.unwrap();
    sqlx::query("INSERT INTO observations(id,building_id,visit_id,reference,description,severity,issued_on,due_on,status) VALUES(?,?,?,'OBS-1','Réparer le ferme-porte',4,'2026-10-29','2026-11-15','open')").bind(&obs).bind(&b).bind(visit).execute(&c.pool).await.unwrap();
    works::sync_legacy(&c.pool, &c.key).await.unwrap();
    works::sync_legacy(&c.pool, &c.key).await.unwrap();
    let s = works::snapshot(&c, &a.token).await.unwrap();
    assert_eq!(s["records"].as_array().unwrap().len(), 1);
    assert_eq!(s["records"][0]["priority"], 4);
    assert_eq!(s["records"][0]["target_due_on"], "2026-11-15");
    assert_eq!(s["records"][0]["status"], "identified");
    let auto_id = s["records"][0]["id"].as_str().unwrap();
    let mut detach = input(&b);
    detach.id = Some(auto_id.into());
    detach.base_version = 1;
    detach.priority = 4;
    assert!(works::save(&c, &a.token, detach).await.is_err());
    assert!(
        works::abandon(&c, &a.token, auto_id, 1, "Pas de suite", false)
            .await
            .is_err()
    );
    let system = sqlx::query("SELECT id,active FROM users WHERE organisation_id=? AND is_system=1")
        .bind(&org)
        .fetch_one(&c.pool)
        .await
        .unwrap();
    let uid: String = system.get(0);
    assert_eq!(system.get::<i64, _>(1), 0);
    assert!(auth::list_users(&c, &a.token)
        .await
        .unwrap()
        .iter()
        .all(|u| u.id != uid));
    assert!(auth::set_active(&c, &a.token, &uid, true).await.is_err());
    assert!(sqlx::query("UPDATE users SET active=1 WHERE id=?")
        .bind(&uid)
        .execute(&c.pool)
        .await
        .is_err());
    let snap = works::snapshot(&c, &a.token).await.unwrap();
    let work = snap["records"][0]["id"].as_str().unwrap();
    let mut low = input(&b);
    low.id = Some(work.into());
    low.base_version = 1;
    low.observation_id = Some(obs.clone());
    low.origin = "commission_observation".into();
    low.priority = 2;
    assert!(works::save(&c, &a.token, low).await.is_err());

    sqlx::query("UPDATE observations SET visit_id=NULL,severity=3,due_on='2026-11-20' WHERE id=?")
        .bind(&obs)
        .execute(&c.pool)
        .await
        .unwrap();
    works::sync_legacy(&c.pool, &c.key).await.unwrap();
    let reclassified = works::snapshot(&c, &a.token).await.unwrap();
    let current = &reclassified["records"][0];
    assert_eq!(current["target_due_on"], "2026-11-20");
    assert_eq!(current["priority"], 4);
    assert_eq!(current["observation"]["commission"], false);
    let mut corrected = input(&b);
    corrected.id = Some(current["id"].as_str().unwrap().into());
    corrected.base_version = current["version"].as_i64().unwrap();
    corrected.observation_id = Some(obs.clone());
    corrected.origin = "commission_observation".into();
    corrected.priority = 4;
    works::save(&c, &a.token, corrected).await.unwrap();
    let mut fake = input(&b);
    fake.observation_id = Some(obs.clone());
    fake.origin = "commission_observation".into();
    fake.priority = 4;
    assert!(works::save(&c, &a.token, fake).await.is_err());
    audit::checkpoint(&c.pool, &c.root).await.unwrap();
    audit::verify(&c.pool, &c.key, &c.root).await.unwrap();
    c.pool.close().await;
    drop(c);
    let c = initialise(root.clone()).await.unwrap();
    assert_eq!(
        works::snapshot(
            &c,
            &auth::login(&c, "admin", "Phrase administrateur test", None)
                .await
                .unwrap()
                .token
        )
        .await
        .unwrap()["records"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    c.pool.close().await;
    drop(c);
    db::ensure_within(&root, &std::env::temp_dir()).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn abandoned_work_preserves_the_real_execution_history() {
    let root = std::env::temp_dir().join(format!("erp-m4-abandon-{}", db::id()));
    let c = initialise(root.clone()).await.unwrap();
    let a = auth::bootstrap(&c, "Org", "Admin", "admin", "Phrase administrateur test")
        .await
        .unwrap();
    let b = building(&c, &a.token, "Bâtiment").await;
    let id = works::save(&c, &a.token, input(&b)).await.unwrap();
    works::abandon(&c, &a.token, &id, 1, "Demande devenue inutile", false)
        .await
        .unwrap();
    let s = works::snapshot(&c, &a.token).await.unwrap();
    let w = &s["records"][0];
    assert_eq!(w["status"], "archived");
    assert_eq!(w["progress_percent"], 0);
    assert!(w["actual_start"].is_null());
    assert!(w["actual_end"].is_null());
    assert_eq!(w["abandoned_from"], "identified");
    assert!(works::transition(
        &c,
        &a.token,
        &id,
        2,
        "closed",
        "Fausse réalisation interdite"
    )
    .await
    .is_err());
    works::abandon(&c, &a.token, &id, 2, "Demande à reprendre", true)
        .await
        .unwrap();
    let s = works::snapshot(&c, &a.token).await.unwrap();
    assert_eq!(s["records"][0]["status"], "identified");
    assert!(s["records"][0]["abandoned_reason"].is_null());
    assert!(s["records"][0]["validated_at"].is_null());
    assert_eq!(
        works::history(&c, &a.token, &id)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        3
    );
    audit::verify(&c.pool, &c.key, &c.root).await.unwrap();
    c.pool.close().await;
    drop(c);
    db::ensure_within(&root, &std::env::temp_dir()).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
