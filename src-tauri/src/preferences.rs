use crate::{audit, auth, db, vault, Core};
use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Serialize, Deserialize, Clone)]
pub struct Features {
    #[serde(default = "default_yes")]
    pub alerts: bool,
    #[serde(default = "default_yes")]
    pub documents: bool,
    pub qr: bool,
    pub inventory: bool,
    pub contracts: bool,
    pub consumption: bool,
    pub budget: bool,
    pub planning: bool,
}
impl Default for Features {
    fn default() -> Self {
        Self {
            alerts: true,
            documents: true,
            qr: false,
            inventory: true,
            contracts: true,
            consumption: true,
            budget: true,
            planning: false,
        }
    }
}
pub async fn global(c: &Core, u: &str, p: &str) -> Result<bool> {
    if !c.sharing.can_write() && (p.ends_with(".write") || p=="work.validate") {return Ok(false);}
    Ok(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM role_grants g JOIN role_permissions p ON p.role_code=g.role_code WHERE g.user_id=? AND g.building_id IS NULL AND g.role_code<>'prestataire' AND p.permission_code=? AND (g.expires_at IS NULL OR g.expires_at>?)").bind(u).bind(p).bind(db::now()).fetch_one(&c.pool).await?>0)
}
pub async fn org(c: &Core, u: &str) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
            .bind(u)
            .fetch_one(&c.pool)
            .await?,
    )
}
pub async fn get(c: &Core, t: &str) -> Result<Value> {
    let u = auth::require(c, t, None).await?;
    let o = org(c, &u).await?;
    let f = sqlx::query_scalar::<_, String>("SELECT value_json FROM settings WHERE key=?")
        .bind(format!("features:{o}"))
        .fetch_optional(&c.pool)
        .await?
        .map(|s| serde_json::from_str::<Features>(&s))
        .transpose()?
        .unwrap_or_default();
    let logo = sqlx::query_scalar::<_, String>("SELECT value_json FROM settings WHERE key=?")
        .bind(format!("logo:{o}"))
        .fetch_optional(&c.pool)
        .await?;
    let image = if let Some(id) = logo {
        let id: String = serde_json::from_str(&id)?;
        let mime: String =
            sqlx::query_scalar("SELECT mime_type FROM files WHERE id=? AND organisation_id=?")
                .bind(&id)
                .bind(&o)
                .fetch_one(&c.pool)
                .await?;
        Some(format!(
            "data:{mime};base64,{}",
            STANDARD.encode(&*vault::read(c, &id, &o).await?)
        ))
    } else {
        None
    };
    Ok(json!({"features":f,"logo":image,"can_manage":global(c,&u,"users.manage").await?}))
}
pub async fn save(c: &Core, t: &str, f: Features) -> Result<()> {
    crate::shared_workspace::require_write(c)?;
    let u = auth::require(c, t, None).await?;
    if !global(c, &u, "users.manage").await? {
        bail!("Gestion des fonctionnalités réservée à l’exploitant global.")
    };
    let o = org(c, &u).await?;
    let mut tx = c.pool.begin().await?;
    sqlx::query("INSERT INTO settings(key,value_json,modified_at) VALUES(?,?,?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,modified_at=excluded.modified_at").bind(format!("features:{o}")).bind(serde_json::to_string(&f)?).bind(db::now()).execute(&mut *tx).await?;
    audit::append(&mut tx, &c.key, Some(&u), "features.changed", json!(f)).await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}
pub async fn logo(c: &Core, t: &str, path: Option<&std::path::Path>) -> Result<()> {
    crate::shared_workspace::require_write(c)?;
    let u = auth::require(c, t, None).await?;
    if !global(c, &u, "users.manage").await? {
        bail!("Modification du logo non autorisée.")
    };
    let o = org(c, &u).await?;
    let file = if let Some(p) = path {
        if std::fs::metadata(p)?.len() > 4 * 1024 * 1024 {
            bail!("Logo limité à 4 Mio.")
        };
        let data = std::fs::read(p)?;
        let mime = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if data.starts_with(&[255, 216, 255]) {
            "image/jpeg"
        } else {
            bail!("Choisissez une image PNG ou JPEG.")
        };
        Some(
            vault::put(
                c,
                &o,
                &u,
                p.file_name().and_then(|s| s.to_str()).unwrap_or("logo.png"),
                mime,
                &data,
            )
            .await?,
        )
    } else {
        None
    };
    let mut tx = c.pool.begin().await?;
    if let Some(f) = file {
        sqlx::query("INSERT INTO settings(key,value_json,modified_at) VALUES(?,?,?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,modified_at=excluded.modified_at").bind(format!("logo:{o}")).bind(json!(f).to_string()).bind(db::now()).execute(&mut *tx).await?;
    } else {
        sqlx::query("DELETE FROM settings WHERE key=?")
            .bind(format!("logo:{o}"))
            .execute(&mut *tx)
            .await?;
    }
    audit::append(
        &mut tx,
        &c.key,
        Some(&u),
        "organisation.logo",
        json!({"removed":path.is_none()}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&c.pool, &c.root).await
}

fn default_yes() -> bool {
    true
}
