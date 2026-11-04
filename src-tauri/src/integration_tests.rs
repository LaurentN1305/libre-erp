use super::*;
#[tokio::test]
async fn p0_database_upgrade_preserves_data() {
    let root = std::env::temp_dir().join(format!("erp-upgrade-{}", db::id()));
    let key = db::prepare(&root).unwrap();
    let migrations = root.join("migrations");
    std::fs::create_dir(&migrations).unwrap();
    std::fs::write(
        migrations.join("0001_initial.sql"),
        include_str!("../migrations/0001_initial.sql"),
    )
    .unwrap();
    std::fs::write(
        migrations.join("0002_p0_permissions.sql"),
        include_str!("../migrations/0002_p0_permissions.sql"),
    )
    .unwrap();
    let pool = db::open(&root.join("db/registre.db"), &key, true)
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(migrations.as_path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO organisations(id,name,created_at) VALUES('preserved','Commune conservée',?)",
    )
    .bind(db::now())
    .execute(&pool)
    .await
    .unwrap();
    let mut old = Core {
        sharing: Arc::new(shared_workspace::Gate::new(&root)),
        root: root.clone(),
        pool: pool.clone(),
        key: key.clone(),
        dummy_hash: String::new(),
        recovery: None,
        pending_mfa: HashMap::new(),
        update_busy: Arc::new(AtomicBool::new(false)),
        _process_lock: Arc::new(std::fs::File::create(root.join("process.lock")).unwrap()),
        app: None,
    };
    let archive = backup::create(
        &mut old,
        None,
        "Phrase sauvegarde ancienne",
        &root.join("backups"),
        "manual",
    )
    .await
    .unwrap();
    let pool = old.pool.clone();
    let snapshot = db::before_migration(&root, &pool).await.unwrap().unwrap();
    assert!(snapshot.exists());
    db::migrate(&pool).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT name FROM organisations WHERE id='preserved'")
            .fetch_one(&pool)
            .await
            .unwrap(),
        "Commune conservée"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        db::SCHEMA_VERSION
    );
    let rid =
        backup::validate_restore(&root, &PathBuf::from(archive), "Phrase sauvegarde ancienne")
            .await
            .unwrap();
    backup::request_restore(&root, &rid).unwrap();
    pool.close().await;
    drop(old);
    let restored = initialise(root.clone()).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT name FROM organisations WHERE id='preserved'")
            .fetch_one(&restored.pool)
            .await
            .unwrap(),
        "Commune conservée"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&restored.pool)
            .await
            .unwrap(),
        db::SCHEMA_VERSION
    );
    restored.pool.close().await;
    drop(restored);
    db::ensure_within(&root, &std::env::temp_dir()).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn dashboard_scope_aggregates_and_readonly() {
    let root = std::env::temp_dir().join(format!("erp-m1-{}", db::id()));
    let core = initialise(root.clone()).await.unwrap();
    let admin = auth::bootstrap(
        &core,
        "Test",
        "Admin",
        "admin",
        "Phrase administrateur test",
    )
    .await
    .unwrap();
    let mut ids = vec![];
    for name in ["École", "Gymnase"] {
        ids.push(
            dashboard::save(
                &core,
                &admin.token,
                dashboard::BuildingInput {
                    location: None,
                    id: None,
                    name: name.into(),
                    address: "Place de la mairie".into(),
                    category: 5,
                    capacity: 80,
                    types: vec!["R".into()],
                    manager: Some(admin.user.id.clone()),
                    opened_on: Some("1998-09-01".into()),
                },
            )
            .await
            .unwrap(),
        );
    }
    let snap = dashboard::snapshot(&core, &admin.token).await.unwrap();
    assert_eq!(snap["buildings"].as_array().unwrap().len(), 2);
    assert_eq!(snap["buildings"][0]["compliance"]["total"], 0);
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
    dashboard::set_scope(&core, &admin.token, &reader.user.id, vec![ids[0].clone()])
        .await
        .unwrap();
    assert!(dashboard::snapshot(&core, &reader.token).await.is_err());
    let reader = auth::login(&core, "commission", "Phrase commission test", None)
        .await
        .unwrap();
    let snap = dashboard::snapshot(&core, &reader.token).await.unwrap();
    assert_eq!(snap["buildings"].as_array().unwrap().len(), 1);
    assert_eq!(snap["buildings"][0]["id"], ids[0]);
    assert_eq!(snap["can_create"], false);
    assert!(snap["buildings"][0].get("budget").is_none());
    assert!(dashboard::photo(&core, &reader.token, &ids[1])
        .await
        .is_err());
    assert!(dashboard::archive(&core, &reader.token, &ids[0], true)
        .await
        .is_err());
    assert!(dashboard::set_photo(&core, &reader.token, &ids[0], None)
        .await
        .is_err());
    sqlx::query("INSERT INTO compliance_requirements(id,building_id,title,applicable,status) VALUES(?,?,'Contrôle',1,'non_compliant')").bind(db::id()).bind(&ids[0]).execute(&core.pool).await.unwrap();
    sqlx::query("INSERT INTO deadlines(id,entity_id,building_id,kind,due_on) VALUES(?,?,?,'Contrôle','2000-01-01')").bind(db::id()).bind(&ids[0]).bind(&ids[0]).execute(&core.pool).await.unwrap();
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
        .bind(&admin.user.id)
        .fetch_one(&core.pool)
        .await
        .unwrap();
    let work = db::id();
    sqlx::query("INSERT INTO entities(id,organisation_id,building_id,kind,created_at) VALUES(?,?,?,'work',?)").bind(&work).bind(&org).bind(&ids[0]).bind(db::now()).execute(&core.pool).await.unwrap();
    sqlx::query("INSERT INTO works(id,building_id,reference,title,description,type,origin,priority,estimated_budget_cents) VALUES(?,?,'T-001','Travail test','Description','corrective','other',1,10000)").bind(&work).bind(&ids[0]).execute(&core.pool).await.unwrap();
    sqlx::query("INSERT INTO work_costs(id,work_id,description,amount_cents,incurred_on) VALUES(?,?,'Coût test',2500,'2026-01-01')").bind(db::id()).bind(&work).execute(&core.pool).await.unwrap();
    let path = root.join("photo.png");
    use base64::Engine;
    let png=base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a1ZkAAAAASUVORK5CYII=").unwrap();
    std::fs::write(&path, &png).unwrap();
    dashboard::set_photo(&core, &admin.token, &ids[0], Some(path))
        .await
        .unwrap();
    let photo = dashboard::photo(&core, &reader.token, &ids[0])
        .await
        .unwrap()
        .unwrap();
    assert!(photo.starts_with("data:image/png;base64,"));
    let file: String = sqlx::query_scalar("SELECT photo_file_id FROM buildings WHERE id=?")
        .bind(&ids[0])
        .fetch_one(&core.pool)
        .await
        .unwrap();
    assert_ne!(
        std::fs::read(root.join("files").join(format!("{file}.blob"))).unwrap(),
        png
    );
    let snap = dashboard::snapshot(&core, &admin.token).await.unwrap();
    let b = snap["buildings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == ids[0])
        .unwrap();
    assert_eq!(b["compliance"]["non_compliant"], 1);
    assert_eq!(b["deadlines"].as_array().unwrap().len(), 1);
    assert_eq!(b["budget"]["estimated"], 10000);
    assert_eq!(b["budget"]["actual"], 2500);
    dashboard::archive(&core, &admin.token, &ids[0], true)
        .await
        .unwrap();
    audit::verify(&core.pool, &core.key, &core.root)
        .await
        .unwrap();
    core.pool.close().await;
    drop(core);
    db::ensure_within(&root, &std::env::temp_dir()).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn rbac_backup_restore_and_vault() {
    let root = std::env::temp_dir().join(format!("securite-erp-integration-{}", db::id()));
    let mut core = initialise(root.clone()).await.unwrap();
    let admin = auth::bootstrap(
        &core,
        "Mairie test",
        "Exploitant",
        "admin",
        "Phrase administrateur test",
    )
    .await
    .unwrap();
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
    let commission = auth::login(&core, "commission", "Phrase commission test", None)
        .await
        .unwrap();
    assert!(
        auth::require(&core, &commission.token, Some("users.manage"))
            .await
            .is_err()
    );
    assert!(
        auth::require(&core, &commission.token, Some("register.read"))
            .await
            .is_ok()
    );
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
        .bind(&admin.user.id)
        .fetch_one(&core.pool)
        .await
        .unwrap();
    let file = crate::vault::put(
        &core,
        &org,
        &admin.user.id,
        "plan.pdf",
        "application/pdf",
        b"DOCUMENT CONFIDENTIEL",
    )
    .await
    .unwrap();
    assert_eq!(
        &*crate::vault::read(&core, &file, &org).await.unwrap(),
        b"DOCUMENT CONFIDENTIEL"
    );
    let raw = std::fs::read(root.join("files").join(format!("{file}.blob"))).unwrap();
    assert!(!raw
        .windows(b"CONFIDENTIEL".len())
        .any(|s| s == b"CONFIDENTIEL"));
    let dest = root.join("backups");
    let archive = backup::create(
        &mut core,
        Some(&admin.user.id),
        "Phrase sauvegarde test",
        &dest,
        "manual",
    )
    .await
    .unwrap();
    assert!(backup::stage_restore(
        &core,
        &admin.user.id,
        &PathBuf::from(&archive),
        "Mauvaise phrase de test"
    )
    .await
    .is_err());
    auth::create_user(
        &core,
        &admin.token,
        "Après sauvegarde",
        "apres",
        "Phrase compte apres test",
        "agent",
    )
    .await
    .unwrap();
    let rid = backup::stage_restore(
        &core,
        &admin.user.id,
        &PathBuf::from(&archive),
        "Phrase sauvegarde test",
    )
    .await
    .unwrap();
    backup::request_restore(&root, &rid).unwrap();
    core.pool.close().await;
    drop(core);
    let restored = initialise(root.clone()).await.unwrap();
    assert!(restored.recovery.is_some());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users WHERE login='apres'")
            .fetch_one(&restored.pool)
            .await
            .unwrap(),
        0
    );
    assert!(auth::require(&restored, &admin.token, None).await.is_err());
    auth::login(&restored, "admin", "Phrase administrateur test", None)
        .await
        .unwrap();
    audit::verify(&restored.pool, &restored.key, &restored.root)
        .await
        .unwrap();
    restored.pool.close().await;
    drop(restored);
    db::ensure_within(&root, &std::env::temp_dir()).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn audit_tampering_detected() {
    let root = std::env::temp_dir().join(format!("securite-erp-audit-{}", db::id()));
    let core = initialise(root.clone()).await.unwrap();
    sqlx::query("DROP TRIGGER audit_events_update_blocked")
        .execute(&core.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE audit_events SET action='falsified'")
        .execute(&core.pool)
        .await
        .unwrap();
    assert!(audit::verify(&core.pool, &core.key, &root).await.is_err());
    core.pool.close().await;
    drop(core);
    db::ensure_within(&root, &std::env::temp_dir()).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn lockout_and_totp_replay() {
    let root = std::env::temp_dir().join(format!("securite-erp-mfa-{}", db::id()));
    let mut core = initialise(root.clone()).await.unwrap();
    let admin = auth::bootstrap(
        &core,
        "Mairie test",
        "Exploitant",
        "admin",
        "Phrase administrateur test",
    )
    .await
    .unwrap();
    let data = auth::begin_mfa(&mut core, &admin.token, "Phrase administrateur test")
        .await
        .unwrap();
    assert!(data["secret"].as_str().unwrap().len() > 20);
    let (secret, _) = core.pending_mfa.get(&admin.user.id).unwrap();
    let code = auth::totp(secret).unwrap().generate_current().unwrap();
    let codes = auth::confirm_mfa(&mut core, &admin.token, &code)
        .await
        .unwrap();
    assert_eq!(codes.len(), 8);
    assert!(
        auth::login(&core, "admin", "Phrase administrateur test", Some(&code))
            .await
            .is_err()
    );
    auth::login(
        &core,
        "admin",
        "Phrase administrateur test",
        Some(&codes[0]),
    )
    .await
    .unwrap();
    assert!(auth::login(
        &core,
        "admin",
        "Phrase administrateur test",
        Some(&codes[0])
    )
    .await
    .is_err());
    for _ in 0..5 {
        assert!(auth::login(&core, "admin", "incorrect", None)
            .await
            .is_err());
    }
    assert!(auth::login(
        &core,
        "admin",
        "Phrase administrateur test",
        Some(&codes[1])
    )
    .await
    .is_err());
    core.pool.close().await;
    drop(core);
    db::ensure_within(&root, &std::env::temp_dir()).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
