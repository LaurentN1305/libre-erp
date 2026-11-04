use crate::{audit, crypto, db, Core};
use anyhow::{bail, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use totp_rs::{Algorithm, TOTP};
use zeroize::Zeroizing;

#[derive(Serialize, Clone)]
pub struct User {
    pub id: String,
    pub login: String,
    pub display_name: String,
    pub roles: Vec<String>,
    pub active: bool,
    pub mfa: bool,
    pub scope: String,
}
#[derive(Serialize)]
pub struct Session {
    pub token: String,
    pub user: User,
    pub permissions: Vec<String>,
}
pub async fn user(pool: &SqlitePool, id: &str) -> Result<User> {
    let r=sqlx::query("SELECT id,login,display_name,active,totp_secret_encrypted IS NOT NULL FROM users WHERE id=?").bind(id).fetch_one(pool).await?;
    let roles=sqlx::query_scalar("SELECT DISTINCT role_code FROM role_grants WHERE user_id=? AND (expires_at IS NULL OR expires_at>?) ORDER BY role_code").bind(id).bind(db::now()).fetch_all(pool).await?;
    let scopes:Vec<String>=sqlx::query_scalar("SELECT DISTINCT CASE WHEN g.building_id IS NULL THEN 'Toute l’organisation' ELSE b.name END FROM role_grants g LEFT JOIN buildings b ON b.id=g.building_id WHERE g.user_id=? AND (g.expires_at IS NULL OR g.expires_at>?)").bind(id).bind(db::now()).fetch_all(pool).await?;
    Ok(User {
        id: r.get(0),
        login: r.get(1),
        display_name: r.get(2),
        roles,
        active: r.get::<i64, _>(3) == 1,
        mfa: r.get::<i64, _>(4) == 1,
        scope: scopes.join(", "),
    })
}
pub async fn permissions(pool: &SqlitePool, id: &str) -> Result<Vec<String>> {
    // Les opérations de socle sont réservées aux attributions globales.
    Ok(sqlx::query_scalar("SELECT DISTINCT rp.permission_code FROM role_grants rg JOIN role_permissions rp ON rp.role_code=rg.role_code WHERE rg.user_id=? AND rg.building_id IS NULL AND (rg.expires_at IS NULL OR rg.expires_at>?) ORDER BY rp.permission_code").bind(id).bind(db::now()).fetch_all(pool).await?)
}
pub async fn require(core: &Core, token: &str, permission: Option<&str>) -> Result<String> {
    let hash = Sha256::digest(token.as_bytes()).to_vec();
    let current = db::now();
    let r=sqlx::query("SELECT s.user_id FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=? AND s.revoked_at IS NULL AND s.expires_at>? AND s.last_activity_at>? AND s.mfa_verified=1 AND u.active=1")
        .bind(hash).bind(&current).bind((chrono::Utc::now()-chrono::Duration::minutes(30)).to_rfc3339_opts(chrono::SecondsFormat::Millis,true)).fetch_optional(&core.pool).await?;
    let id: String = r
        .ok_or_else(|| anyhow::anyhow!("Session expirée. Veuillez vous reconnecter."))?
        .get(0);
    if let Some(p) = permission {
        if !permissions(&core.pool, &id).await?.iter().any(|x| x == p) {
            bail!("Votre rôle ne permet pas cette opération.");
        }
    }
    sqlx::query("UPDATE sessions SET last_activity_at=? WHERE token_hash=?")
        .bind(current)
        .bind(Sha256::digest(token.as_bytes()).to_vec())
        .execute(&core.pool)
        .await?;
    Ok(id)
}
async fn issue(core: &Core, id: &str) -> Result<Session> {
    let token = hex::encode(crypto::random_key().as_slice());
    let hash = Sha256::digest(token.as_bytes()).to_vec();
    let mut tx = core.pool.begin().await?;
    sqlx::query("INSERT INTO sessions(id,user_id,token_hash,created_at,expires_at,last_activity_at,mfa_verified) VALUES(?,?,?,?,?,?,1)")
        .bind(db::id()).bind(id).bind(hash).bind(db::now()).bind((chrono::Utc::now()+chrono::Duration::hours(8)).to_rfc3339_opts(chrono::SecondsFormat::Millis,true)).bind(db::now()).execute(&mut *tx).await?;
    audit::append(
        &mut tx,
        &core.key,
        Some(id),
        "session.open",
        serde_json::json!({}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await?;
    Ok(Session {
        token,
        user: user(&core.pool, id).await?,
        permissions: permissions(&core.pool, id).await?,
    })
}
pub async fn bootstrap(
    core: &Core,
    organisation: &str,
    display_name: &str,
    login: &str,
    password: &str,
) -> Result<Session> {
    crate::shared_workspace::require_write(core)?;
    if sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users")
        .fetch_one(&core.pool)
        .await?
        != 0
    {
        bail!("Le compte initial existe déjà.");
    }
    validate_fields(display_name, login)?;
    crypto::validate_password(password)?;
    if organisation.trim().is_empty() || organisation.len() > 200 {
        bail!("Renseignez le nom de votre organisation.");
    }
    let org = db::id();
    let uid = db::id();
    let hash = crypto::password_hash(password)?;
    let mut tx = core.pool.begin().await?;
    sqlx::query("INSERT INTO organisations(id,name,created_at) VALUES(?,?,?)")
        .bind(&org)
        .bind(organisation.trim())
        .bind(db::now())
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO users(id,organisation_id,login,display_name,password_hash,created_at) VALUES(?,?,?,?,?,?)").bind(&uid).bind(&org).bind(login.trim()).bind(display_name.trim()).bind(hash).bind(db::now()).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO role_grants(id,user_id,organisation_id,role_code) VALUES(?,?,?,'exploitant')",
    )
    .bind(db::id())
    .bind(&uid)
    .bind(org)
    .execute(&mut *tx)
    .await?;
    audit::append(
        &mut tx,
        &core.key,
        Some(&uid),
        "organisation.initialise",
        serde_json::json!({}),
    )
    .await?;
    tx.commit().await?;
    issue(core, &uid).await
}
pub fn totp(secret: &[u8]) -> Result<TOTP> {
    TOTP::new(Algorithm::SHA1, 6, 1, 30, secret.to_vec())
        .map_err(|_| anyhow::anyhow!("Configuration TOTP invalide"))
}
fn totp_step(secret: &[u8], code: &str, last: Option<i64>) -> Result<Option<i64>> {
    let step = chrono::Utc::now().timestamp() / 30;
    let t = totp(secret)?;
    for n in [step - 1, step, step + 1] {
        if n > last.unwrap_or(-1) && t.generate((n * 30) as u64) == code {
            return Ok(Some(n));
        }
    }
    Ok(None)
}
pub async fn login(core: &Core, login: &str, password: &str, otp: Option<&str>) -> Result<Session> {
    if login.len() > 100 || password.len() > 1024 || otp.unwrap_or("").len() > 100 {
        bail!("Identifiants invalides.");
    }
    let r=sqlx::query("SELECT id,password_hash,active,failed_attempts,locked_until,totp_secret_encrypted,totp_last_step FROM users WHERE login=? COLLATE NOCASE LIMIT 1").bind(login.trim()).fetch_optional(&core.pool).await?;
    // Même travail Argon2 pour un compte absent (hash de remplissage persistant en mémoire).
    let Some(r) = r else {
        let _ = crypto::password_matches(password, &core.dummy_hash);
        bail!("Identifiants ou code de sécurité incorrects.");
    };
    let id: String = r.get(0);
    let hash: String = r.get(1);
    let active: i64 = r.get(2);
    let failed: i64 = r.get(3);
    let lock: Option<String> = r.get(4);
    if lock.as_deref().is_some_and(|v| v > db::now().as_str()) {
        bail!("Compte temporairement verrouillé. Réessayez dans 15 minutes.");
    }
    let mut valid = crypto::password_matches(password, &hash) && active == 1;
    let secret: Option<Vec<u8>> = r.get(5);
    let last: Option<i64> = r.get(6);
    let mut next_step = None;
    let mut recovery_hash = None;
    if let Some(secret) = secret {
        if valid {
            let secret = crypto::unseal(&core.key, &secret, format!("totp:{id}").as_bytes())?;
            next_step = totp_step(&secret, otp.unwrap_or(""), last)?;
            if next_step.is_none() {
                let candidate = hex::encode(Sha256::digest(otp.unwrap_or("").as_bytes()));
                let exists:i64=sqlx::query_scalar("SELECT count(*) FROM recovery_codes WHERE user_id=? AND code_hash=? AND used_at IS NULL").bind(&id).bind(&candidate).fetch_one(&core.pool).await?;
                if exists == 1 && !core.sharing.enabled.load(std::sync::atomic::Ordering::SeqCst) {
                    recovery_hash = Some(candidate);
                } else {
                    valid = false;
                }
            }
        }
    }
    let mut tx = core.pool.begin().await?;
    if !valid {
        let until = if failed + 1 >= 5 {
            Some(
                (chrono::Utc::now() + chrono::Duration::minutes(15))
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            )
        } else {
            None
        };
        sqlx::query("UPDATE users SET failed_attempts=?,locked_until=? WHERE id=?")
            .bind(if until.is_some() { 0 } else { failed + 1 })
            .bind(until)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        audit::append(
            &mut tx,
            &core.key,
            Some(&id),
            "session.denied",
            serde_json::json!({}),
        )
        .await?;
        tx.commit().await?;
        audit::checkpoint(&core.pool, &core.root).await?;
        bail!("Identifiants ou code de sécurité incorrects.");
    }
    sqlx::query("UPDATE users SET failed_attempts=0,locked_until=NULL,totp_last_step=coalesce(?,totp_last_step) WHERE id=?").bind(next_step).bind(&id).execute(&mut *tx).await?;
    if let Some(hash) = recovery_hash {
        sqlx::query("UPDATE recovery_codes SET used_at=? WHERE user_id=? AND code_hash=?")
            .bind(db::now())
            .bind(&id)
            .bind(hash)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    issue(core, &id).await
}
pub async fn logout(core: &Core, token: &str) -> Result<()> {
    let id = require(core, token, None).await?;
    if core.sharing.enabled.load(std::sync::atomic::Ordering::SeqCst) {core.sharing.writable.store(false,std::sync::atomic::Ordering::SeqCst);}
    let mut tx = core.pool.begin().await?;
    sqlx::query("UPDATE sessions SET revoked_at=? WHERE token_hash=?")
        .bind(db::now())
        .bind(Sha256::digest(token.as_bytes()).to_vec())
        .execute(&mut *tx)
        .await?;
    audit::append(
        &mut tx,
        &core.key,
        Some(&id),
        "session.close",
        serde_json::json!({}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await
}
pub async fn list_users(core: &Core, token: &str) -> Result<Vec<User>> {
    require(core, token, Some("users.manage")).await?;
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM users WHERE is_system=0 AND is_external=0 ORDER BY display_name",
    )
    .fetch_all(&core.pool)
    .await?;
    let mut out = vec![];
    for id in ids {
        out.push(user(&core.pool, &id).await?);
    }
    Ok(out)
}
fn validate_fields(name: &str, login: &str) -> Result<()> {
    if name.trim().is_empty()
        || name.len() > 200
        || login.is_empty()
        || login.len() > 100
        || !login
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    {
        bail!("Nom requis ; identifiant composé de lettres, chiffres, point, tiret ou souligné.");
    }
    Ok(())
}
pub async fn create_user(
    core: &Core,
    token: &str,
    name: &str,
    login: &str,
    password: &str,
    role: &str,
) -> Result<()> {
    crate::shared_workspace::require_write(core)?;
    let actor = require(core, token, Some("users.manage")).await?;
    validate_fields(name, login)?;
    crypto::validate_password(password)?;
    if ![
        "exploitant",
        "securite",
        "technique",
        "agent",
        "prestataire",
        "commission",
    ]
    .contains(&role)
    {
        bail!("Rôle inconnu.");
    }
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
        .bind(&actor)
        .fetch_one(&core.pool)
        .await?;
    let uid = db::id();
    let hash = crypto::password_hash(password)?;
    let mut tx = core.pool.begin().await?;
    sqlx::query("INSERT INTO users(id,organisation_id,login,display_name,password_hash,created_at) VALUES(?,?,?,?,?,?)").bind(&uid).bind(&org).bind(login).bind(name).bind(hash).bind(db::now()).execute(&mut *tx).await.map_err(|_|anyhow::anyhow!("Cet identifiant existe déjà ou n’est pas valide."))?;
    sqlx::query("INSERT INTO role_grants(id,user_id,organisation_id,role_code) VALUES(?,?,?,?)")
        .bind(db::id())
        .bind(&uid)
        .bind(org)
        .bind(role)
        .execute(&mut *tx)
        .await?;
    audit::append(
        &mut tx,
        &core.key,
        Some(&actor),
        "user.create",
        serde_json::json!({"user_id":uid,"role":role}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await
}
pub async fn set_active(core: &Core, token: &str, user_id: &str, active: bool) -> Result<()> {
    crate::shared_workspace::require_write(core)?;
    let actor = require(core, token, Some("users.manage")).await?;
    if actor == user_id && !active {
        bail!("Vous ne pouvez pas désactiver votre propre compte.");
    }
    let mut tx = core.pool.begin().await?;
    let result =
        sqlx::query("UPDATE users SET active=? WHERE id=? AND is_system=0 AND is_external=0")
            .bind(active)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
    if result.rows_affected() != 1 {
        bail!("Utilisateur inconnu.");
    }
    if !active {
        sqlx::query("UPDATE sessions SET revoked_at=? WHERE user_id=?")
            .bind(db::now())
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
    }
    audit::append(
        &mut tx,
        &core.key,
        Some(&actor),
        "user.status",
        serde_json::json!({"user_id":user_id,"active":active}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await
}
pub async fn password_change(core: &Core, token: &str, current: &str, new: &str) -> Result<()> {
    crate::shared_workspace::require_write(core)?;
    let id = require(core, token, None).await?;
    crypto::validate_password(new)?;
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id=?")
        .bind(&id)
        .fetch_one(&core.pool)
        .await?;
    if !crypto::password_matches(current, &hash) {
        bail!("Mot de passe actuel incorrect.");
    }
    let hash = crypto::password_hash(new)?;
    let mut tx = core.pool.begin().await?;
    sqlx::query("UPDATE users SET password_hash=? WHERE id=?")
        .bind(hash)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE sessions SET revoked_at=? WHERE user_id=? AND token_hash<>?")
        .bind(db::now())
        .bind(&id)
        .bind(Sha256::digest(token.as_bytes()).to_vec())
        .execute(&mut *tx)
        .await?;
    audit::append(
        &mut tx,
        &core.key,
        Some(&id),
        "user.password",
        serde_json::json!({}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await
}
pub async fn begin_mfa(core: &mut Core, token: &str, password: &str) -> Result<serde_json::Value> {
    crate::shared_workspace::require_write(core)?;
    let uid = require(core, token, None).await?;
    let r = sqlx::query("SELECT password_hash,totp_secret_encrypted FROM users WHERE id=?")
        .bind(&uid)
        .fetch_one(&core.pool)
        .await?;
    if r.get::<Option<Vec<u8>>, _>(1).is_some() {
        bail!("La double authentification est déjà activée.");
    }
    if !crypto::password_matches(password, &r.get::<String, _>(0)) {
        bail!("Mot de passe incorrect.");
    }
    let secret = crypto::random_key();
    let raw = totp(&secret)?.get_secret_base32();
    core.pending_mfa.insert(
        uid,
        (
            Zeroizing::new(secret.to_vec()),
            chrono::Utc::now().timestamp() + 600,
        ),
    );
    Ok(
        serde_json::json!({"secret":raw,"uri":format!("otpauth://totp/SecuriteERP?secret={raw}&issuer=SecuriteERP&algorithm=SHA1&digits=6&period=30")}),
    )
}
pub async fn confirm_mfa(core: &mut Core, token: &str, code: &str) -> Result<Vec<String>> {
    crate::shared_workspace::require_write(core)?;
    let uid = require(core, token, None).await?;
    let (secret, expires) = core
        .pending_mfa
        .remove(&uid)
        .ok_or_else(|| anyhow::anyhow!("Recommencez l’activation."))?;
    if expires < chrono::Utc::now().timestamp() {
        bail!("Activation expirée.");
    }
    let step = totp_step(&secret, code, None)?
        .ok_or_else(|| anyhow::anyhow!("Code incorrect. Recommencez l’activation."))?;
    let sealed = crypto::seal(&core.key, &secret, format!("totp:{uid}").as_bytes())?;
    let mut tx = core.pool.begin().await?;
    sqlx::query("UPDATE users SET totp_secret_encrypted=?,totp_last_step=? WHERE id=?")
        .bind(sealed)
        .bind(step)
        .bind(&uid)
        .execute(&mut *tx)
        .await?;
    let mut codes = vec![];
    for _ in 0..8 {
        let code = hex::encode(&crypto::random_key()[..10]);
        sqlx::query("INSERT INTO recovery_codes(user_id,code_hash) VALUES(?,?)")
            .bind(&uid)
            .bind(hex::encode(Sha256::digest(code.as_bytes())))
            .execute(&mut *tx)
            .await?;
        codes.push(code);
    }
    sqlx::query("UPDATE sessions SET revoked_at=? WHERE user_id=? AND token_hash<>?")
        .bind(db::now())
        .bind(&uid)
        .bind(Sha256::digest(token.as_bytes()).to_vec())
        .execute(&mut *tx)
        .await?;
    audit::append(
        &mut tx,
        &core.key,
        Some(&uid),
        "user.mfa.enable",
        serde_json::json!({}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await?;
    Ok(codes)
}
