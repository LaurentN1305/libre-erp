use super::*;
use serde_json::json;
use sqlx::Row;
async fn setup() -> (Core, String, String, String) {
    let root = std::env::temp_dir().join(format!("libre-m7-{}", db::id()));
    let c = initialise(root).await.unwrap();
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
    let other = building(&c, &a.token, "École").await;
    (c, a.token, b, other)
}
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
async fn manual(c: &Core, t: &str, b: &str, title: &str, days: i64) -> String {
    let day = (chrono::NaiveDate::parse_from_str(&works::today(), "%Y-%m-%d").unwrap()
        + chrono::Duration::days(days))
    .to_string();
    alerts::manual_save(c,t,json!({"building_id":b,"title":title,"due_on":day,"notes":"Échéance test","restricted":false,"reason":"Saisie de test"})).await.unwrap()
}
async fn cleanup(c: Core) {
    let root = c.root.clone();
    c.pool.close().await;
    drop(c);
    std::fs::remove_dir_all(root).unwrap()
}
#[tokio::test]
async fn alert_stages_idempotence_escalation_ack_and_completion() {
    let (c, t, b, _) = setup().await;
    let d = manual(&c, &t, &b, "Contrôle ponctuel", 20).await;
    let base = chrono::Utc::now();
    let count = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM alerts WHERE deadline_id=?")
        .bind(&d)
        .fetch_one(&c.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(alerts::evaluate_at(&c, base).await.unwrap(), 0);
    alerts::evaluate_at(&c, base + chrono::Duration::days(8))
        .await
        .unwrap();
    let r=sqlx::query("SELECT escalation_level,days_before FROM alerts WHERE deadline_id=? AND resolved_at IS NULL").bind(&d).fetch_one(&c.pool).await.unwrap();
    assert_eq!(r.get::<i64, _>(0), 1);
    assert_eq!(r.get::<i64, _>(1), 30);
    alerts::evaluate_at(&c, base + chrono::Duration::days(15))
        .await
        .unwrap();
    let r=sqlx::query("SELECT escalation_level,days_before FROM alerts WHERE deadline_id=? AND resolved_at IS NULL").bind(&d).fetch_one(&c.pool).await.unwrap();
    assert_eq!(r.get::<i64, _>(0), 2);
    assert_eq!(r.get::<i64, _>(1), 7);
    assert_eq!(
        alerts::evaluate_at(&c, base + chrono::Duration::days(15))
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM alerts WHERE deadline_id=? AND resolved_at IS NULL"
        )
        .bind(&d)
        .fetch_one(&c.pool)
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM alerts WHERE deadline_id=?")
            .bind(&d)
            .fetch_one(&c.pool)
            .await
            .unwrap(),
        3
    );
    alerts::manual_complete(&c, &t, &d, "Vérification réalisée")
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM alerts WHERE deadline_id=? AND resolved_at IS NULL"
        )
        .bind(&d)
        .fetch_one(&c.pool)
        .await
        .unwrap(),
        0
    );
    let d = manual(&c, &t, &b, "Rappel pris en charge", 40).await;
    let id: String =
        sqlx::query_scalar("SELECT id FROM alerts WHERE deadline_id=? AND resolved_at IS NULL")
            .bind(&d)
            .fetch_one(&c.pool)
            .await
            .unwrap();
    alerts::acknowledge(&c, &t, &id, "Intervention commandée")
        .await
        .unwrap();
    alerts::evaluate_at(&c, chrono::Utc::now() + chrono::Duration::days(8))
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT escalation_level FROM alerts WHERE deadline_id=? AND resolved_at IS NULL"
        )
        .bind(&d)
        .fetch_one(&c.pool)
        .await
        .unwrap(),
        0
    );
    alerts::evaluate_at(&c, chrono::Utc::now() + chrono::Duration::days(11))
        .await
        .unwrap();
    let r=sqlx::query("SELECT acknowledged_at,days_before FROM alerts WHERE deadline_id=? AND resolved_at IS NULL").bind(&d).fetch_one(&c.pool).await.unwrap();
    assert!(r.get::<Option<String>, _>(0).is_none());
    assert_eq!(r.get::<i64, _>(1), 30);
    assert!(sqlx::query("DELETE FROM alert_events")
        .execute(&c.pool)
        .await
        .is_err());
    audit::verify(&c.pool, &c.key, &c.root).await.unwrap();
    cleanup(c).await;
}
#[tokio::test]
async fn alert_scope_rules_privacy_notifications_and_share() {
    let (c, t, b, other) = setup().await;
    let d = manual(&c, &t, &b, "Échéance locale", 5).await;
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
    let tech = auth::login(&c, "tech", "Phrase technique test", None)
        .await
        .unwrap();
    dashboard::set_scope(&c, &t, &tech.user.id, vec![other.clone()])
        .await
        .unwrap();
    let tech = auth::login(&c, "tech", "Phrase technique test", None)
        .await
        .unwrap();
    assert!(alerts::snapshot(&c, &tech.token).await.unwrap()["records"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(
        alerts::manual_complete(&c, &tech.token, &d, "Interdit hors périmètre")
            .await
            .is_err()
    );
    let root_rule: String = sqlx::query_scalar(
        "SELECT id FROM alert_rules WHERE managed=1 AND building_id IS NULL AND retired_at IS NULL",
    )
    .fetch_one(&c.pool)
    .await
    .unwrap();
    assert!(alerts::rule_save(&c,&tech.token,json!({"id":root_rule,"building_id":null,"family":"*","name":"Interdit","enabled":true,"severity":1,"escalation_days":7,"days":[7],"role":"technique","role1":"securite","role2":"exploitant","reason":"Tentative hors périmètre"})).await.is_err());
    let override_rule=alerts::rule_save(&c,&t,json!({"building_id":b,"family":"manual","name":"Suspension locale","enabled":false,"severity":1,"escalation_days":7,"days":[90,60,30,7],"role":"technique","role1":"securite","role2":"exploitant","reason":"Désactivation locale"})).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM alerts WHERE deadline_id=? AND resolved_at IS NULL"
        )
        .bind(&d)
        .fetch_one(&c.pool)
        .await
        .unwrap(),
        0
    );
    alerts::rule_save(&c,&t,json!({"id":override_rule,"building_id":b,"family":"manual","name":"Réactivation locale","enabled":true,"severity":2,"escalation_days":2,"days":[30,7],"role":"technique","role1":"securite","role2":"exploitant","reason":"Réactivation locale"})).await.unwrap();
    assert_eq!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&c.pool)
            .await
            .unwrap()
            .len(),
        0
    );
    let pdf = c.root.join("facture.pdf");
    std::fs::write(&pdf, b"%PDF-1.4\nTest").unwrap();
    let secret=documents::save(&c,&t,json!({"building_id":b,"title":"Facture privée","category":"invoice","expires_on":works::today(),"notes":"Confidentiel","restricted":false,"links":[b],"reason":"Facture test"}),Some(&pdf)).await.unwrap();
    auth::create_user(&c, &t, "Agent", "agent", "Phrase agent test", "agent")
        .await
        .unwrap();
    let agent = auth::login(&c, "agent", "Phrase agent test", None)
        .await
        .unwrap();
    let view = alerts::snapshot(&c, &agent.token).await.unwrap();
    assert!(!view.to_string().contains("Facture privée"));
    let deadline: String =
        sqlx::query_scalar("SELECT id FROM deadlines WHERE entity_id=? AND completed_at IS NULL")
            .bind(&secret)
            .fetch_one(&c.pool)
            .await
            .unwrap();
    assert!(
        !alerts::visible(&c, &agent.user.id, &deadline, "alerts.read")
            .await
            .unwrap()
    );
    assert!(!dashboard::snapshot(&c, &agent.token)
        .await
        .unwrap()
        .to_string()
        .contains("Facture privée"));
    alerts::save_settings(
        &c,
        &t,
        alerts::Settings {
            desktop: false,
            scheduled: false,
            time: "08:00".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(alerts::notify(&c, &t).await.unwrap(), 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM notification_attempts")
            .fetch_one(&c.pool)
            .await
            .unwrap(),
        0
    );
    alerts::save_settings(&c, &t, alerts::Settings::default())
        .await
        .unwrap();
    assert_eq!(alerts::notify(&c, &t).await.unwrap(), 0);
    assert!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM notifications WHERE state='failed'")
            .fetch_one(&c.pool)
            .await
            .unwrap()
            > 0
    );
    alerts::retry(&c, &t).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM notifications WHERE state='failed'")
            .fetch_one(&c.pool)
            .await
            .unwrap(),
        0
    );
    assert!(
        alerts::save_settings(&c, &tech.token, alerts::Settings::default())
            .await
            .is_err()
    );
    let old=sqlx::query("SELECT title,due_on FROM manual_deadlines m JOIN deadlines d ON d.id=m.deadline_id WHERE deadline_id=?").bind(&d).fetch_one(&c.pool).await.unwrap();
    let replacement=alerts::manual_save(&c,&t,json!({"id":d,"building_id":b,"title":"Échéance reportée","due_on":old.get::<String,_>(1),"notes":"Report tracé","restricted":false,"reason":"Modification suivie"})).await.unwrap();
    assert_ne!(replacement, d);
    assert!(sqlx::query_scalar::<_, Option<String>>(
        "SELECT completed_at FROM deadlines WHERE id=?"
    )
    .bind(&d)
    .fetch_one(&c.pool)
    .await
    .unwrap()
    .is_some());
    let share = std::env::temp_dir().join(format!("m7-{}.erp-partage", db::id()));
    exchange::export(&c, &t, &b, "Partage alertes test 123!", &share)
        .await
        .unwrap();
    let tr = std::env::temp_dir().join(format!("m7-recu-{}", db::id()));
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
    let p = exchange::preview(&target, &ta.token, &share, "Partage alertes test 123!")
        .await
        .unwrap();
    exchange::import(
        &target,
        &ta.token,
        &share,
        "Partage alertes test 123!",
        &p.digest,
        None,
        false,
    )
    .await
    .unwrap();
    let data = alerts::snapshot(&target, &ta.token).await.unwrap();
    assert_eq!(data["manuals"].as_array().unwrap().len(), 2);
    assert_eq!(data["rules"].as_array().unwrap().len(), 1);
    audit::verify(&c.pool, &c.key, &c.root).await.unwrap();
    cleanup(target).await;
    cleanup(c).await;
    std::fs::remove_file(share).unwrap();
}
