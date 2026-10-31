use crate::{audit, auth, db, Core};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};

#[cfg(test)]
use crate::release_proof::Envelope;
use crate::release_proof::{authenticate as authenticated_payload, Payload};
use serde::Serialize;
use sqlx::Row;
use std::{path::Path, time::Duration};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Serialize)]
pub struct Status {
    configured: bool,
    channel: String,
    phase: String,
    candidate: Option<String>,
    notes: Option<String>,
    last_check: Option<String>,
    message: String,
    manifest_verified: bool,
    package_verified: bool,
}
fn installer_name() -> Result<&'static str> {
    match tauri::utils::platform::bundle_type() {
        Some(tauri::utils::config::BundleType::Msi) => Ok("msi"),
        Some(tauri::utils::config::BundleType::Nsis) => Ok("nsis"),
        _ => bail!("Les mises à jour nécessitent une version installée par EXE ou MSI"),
    }
}
fn configured() -> bool {
    !crate::portable::active()
        && installer_name().is_ok()
        && option_env!("ERP_MANIFEST_PUBLIC_KEY").is_some()
        && option_env!("ERP_UPDATE_BASE_URL").is_some()
}
#[cfg(test)]
mod signature_tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    #[test]
    fn authenticates_exact_bytes() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let payload=serde_json::to_vec(&serde_json::json!({"channel":"stable","target":"windows-x86_64","sequence":1,"expires_at":"2030-01-01T00:00:00Z","release":{"version":"0.1.1"}})).unwrap();
        let signature = key.sign(&payload);
        let envelope = Envelope {
            payload: B64.encode(&payload),
            signature: B64.encode(signature.to_bytes()),
        };
        let bytes = serde_json::to_vec(&envelope).unwrap();
        assert_eq!(
            authenticated_payload(&bytes, &key.verifying_key().to_bytes())
                .unwrap()
                .sequence,
            1
        );
        let mut tampered = payload.clone();
        tampered[0] ^= 1;
        let envelope = Envelope {
            payload: B64.encode(tampered),
            signature: envelope.signature,
        };
        assert!(authenticated_payload(
            &serde_json::to_vec(&envelope).unwrap(),
            &key.verifying_key().to_bytes()
        )
        .is_err());
    }
}
pub async fn status(core: &Core) -> Result<Status> {
    let r=sqlx::query("SELECT channel,phase,candidate_version,last_checked_at,last_error,manifest_verified_at,artifact_verified_at FROM update_state WHERE singleton=1").fetch_one(&core.pool).await?;
    let error: Option<String> = r.get(4);
    let saved: Option<String> = sqlx::query_scalar("SELECT signed_manifest_json FROM update_state WHERE singleton=1").fetch_one(&core.pool).await?;
    let notes = saved.and_then(|s| validate_envelope(s.as_bytes(), &r.get::<String,_>(0)).ok())
        .and_then(|(_, release)| release["notes"].as_str().map(str::to_owned));
    Ok(Status {
        configured: configured(),
        notes,
        channel: r.get(0),
        phase: if crate::portable::active() {
            "idle".into()
        } else {
            r.get(1)
        },
        candidate: if crate::portable::active() {
            None
        } else {
            r.get(2)
        },
        last_check: r.get(3),
        message: if crate::portable::active() {
            "Version portable : fermez l’application et remplacez les exécutables avec une livraison vérifiée, en conservant Donnees et portable.json.".into()
        } else if !configured() {
            "Les mises à jour seront activées après configuration des clés et du serveur de publication.".into()
        } else {
            error.unwrap_or_else(|| "Prêt. Les données métier restent hors ligne.".into())
        },
        manifest_verified: r.get::<Option<String>, _>(5).is_some(),
        package_verified: r.get::<Option<String>, _>(6).is_some(),
    })
}
pub async fn channel(core: &Core, token: &str, channel: &str) -> Result<()> {
    let actor = auth::require(core, token, Some("updates.manage")).await?;
    if !["stable", "beta"].contains(&channel) {
        bail!("Canal invalide.");
    }
    let mut tx = core.pool.begin().await?;
    sqlx::query("UPDATE update_state SET channel=?,phase='idle',candidate_version=NULL,artifact_verified_at=NULL,manifest_verified_at=NULL,last_error=NULL WHERE singleton=1").bind(channel).execute(&mut *tx).await?;
    audit::append(
        &mut tx,
        &core.key,
        Some(&actor),
        "updates.channel",
        serde_json::json!({"channel":channel}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await
}
fn validate_envelope(bytes: &[u8], channel: &str) -> Result<(Payload, serde_json::Value)> {
    let key = B64.decode(
        option_env!("ERP_MANIFEST_PUBLIC_KEY").context("Clé du manifeste non configurée")?,
    )?;
    let key: [u8; 32] = key
        .try_into()
        .map_err(|_| anyhow::anyhow!("Clé de manifeste invalide"))?;
    let value = authenticated_payload(bytes, &key)?;
    if value.channel != channel || value.target != "windows-x86_64" {
        bail!("Canal ou plateforme inattendu.");
    }
    if chrono::DateTime::parse_from_rfc3339(&value.expires_at)? < chrono::Utc::now() {
        bail!("Manifeste expiré. L’application reste utilisable hors ligne.");
    }
    let remote = semver::Version::parse(
        value.release["version"]
            .as_str()
            .context("Version absente")?,
    )?;
    if channel == "stable" && !remote.pre.is_empty() {
        bail!("Version bêta reçue sur le canal stable.");
    }
    crate::update_transport::configured()?.release(
        value.release["url"]
            .as_str()
            .context("URL de téléchargement absente")?,
    )?;
    crate::release_proof::expected_hash(&value)?;
    if value.release["installer"].as_str() != Some(installer_name()?) {
        bail!("Le paquet ne correspond pas au type d’installation EXE/MSI")
    }

    let release = value.release.clone();
    Ok((value, release))
}
fn highwater(root: &Path, channel: &str) -> Result<u64> {
    let p = root.join("updates").join(format!("{channel}.dpapi"));
    if !p.exists() {
        return Ok(0);
    }
    Ok(serde_json::from_slice(&crate::crypto::unprotect(
        &std::fs::read(p)?,
    )?)?)
}
fn accept_sequence(root: &Path, channel: &str, sequence: u64) -> Result<()> {
    if sequence < highwater(root, channel)? {
        bail!("Ancien manifeste rejeté (protection contre le rejeu).");
    }
    db::atomic_write(
        &root.join("updates").join(format!("{channel}.dpapi")),
        &crate::crypto::protect(&serde_json::to_vec(&sequence)?)?,
    )
}
async fn check_inner(core: &Core, app: &tauri::AppHandle) -> Result<()> {
    if !configured() {
        bail!(
            "Serveur de mise à jour non configuré. Vous pouvez utiliser l’application hors ligne."
        );
    }
    let channel: String = sqlx::query_scalar("SELECT channel FROM update_state WHERE singleton=1")
        .fetch_one(&core.pool)
        .await?;
    let base = option_env!("ERP_UPDATE_BASE_URL")
        .unwrap()
        .trim_end_matches('/');
    let client = crate::update_transport::configured()?.client(Duration::from_secs(600))?;
    let mut response = client
        .get(format!(
            "{base}/{channel}/windows-x86_64-{}.json",
            installer_name()?
        ))
        .timeout(Duration::from_secs(25))
        .send()
        .await?
        .error_for_status()?;
    let mut bytes = vec![];
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            bail!("Manifeste trop volumineux.");
        }
        bytes.extend_from_slice(&chunk);
    }
    let (payload, release) = validate_envelope(&bytes, &channel)?;
    accept_sequence(&core.root, &channel, payload.sequence)?;
    // Aucun second appel à un endpoint : l'updater consomme les métadonnées déjà authentifiées.
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(600))
        .build()?;
    let candidate = updater.restore_update(release)?;
    if let Some(update) = candidate {
        sqlx::query("UPDATE update_state SET phase='downloading',candidate_version=?,manifest_verified_at=?,artifact_verified_at=NULL,last_error=NULL WHERE singleton=1").bind(&update.version).bind(db::now()).execute(&core.pool).await?;
        // download() effectue le contrôle de signature natif Tauri.
        // Téléchargement borné, sans redirection vers un autre hôte.
        let mut package = client
            .get(update.download_url.clone())
            .send()
            .await?
            .error_for_status()?;
        const MAX_PACKAGE: usize = 1024 * 1024 * 1024;
        if package
            .content_length()
            .is_some_and(|n| n > MAX_PACKAGE as u64)
        {
            bail!("Paquet de mise à jour trop volumineux.");
        }
        let mut body = Vec::new();
        while let Some(chunk) = package.chunk().await? {
            if body.len() + chunk.len() > MAX_PACKAGE {
                bail!("Paquet de mise à jour trop volumineux.");
            }
            body.extend_from_slice(&chunk);
        }
        update.verify(&body)?;
        crate::release_proof::verify_bytes(&payload, &body)?;
        let path = core.root.join("updates/package.bin");
        db::atomic_write(&path, &body)?;
        db::atomic_write(&core.root.join("updates/envelope.json"), &bytes)?;
        sqlx::query("UPDATE update_state SET phase='ready',artifact_verified_at=?,downloaded_path=?,signed_manifest_json=? WHERE singleton=1")
            .bind(db::now()).bind(path.to_string_lossy().as_ref()).bind(String::from_utf8(bytes)?).execute(&core.pool).await?;
        let notified: Option<String> = sqlx::query_scalar("SELECT value_json FROM settings WHERE key='updates.notified'").fetch_optional(&core.pool).await?;
        let marker = serde_json::json!(format!("{channel}:{}", update.version)).to_string();
        if notified.as_deref() != Some(&marker) {
            use tauri_plugin_notification::NotificationExt;
            if app.notification().builder().title("Libre ERP · Mise à jour disponible")
                .body(format!("La version {} est prête. Ouvrez Libre ERP pour consulter les nouveautés et l’installer.", update.version)).show().is_ok() {
                sqlx::query("INSERT INTO settings(key,value_json,modified_at) VALUES('updates.notified',?,?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,modified_at=excluded.modified_at")
                    .bind(marker).bind(db::now()).execute(&core.pool).await?;
            }
        }
    } else {
        sqlx::query("UPDATE update_state SET phase='idle',candidate_version=NULL,manifest_verified_at=?,artifact_verified_at=NULL,last_error=NULL WHERE singleton=1").bind(db::now()).execute(&core.pool).await?;
    }
    Ok(())
}
pub async fn check(core: &Core, app: &tauri::AppHandle) -> Result<Status> {
    if crate::portable::active() {
        bail!("Mise à jour par remplacement des exécutables en mode portable.")
    }
    sqlx::query("UPDATE update_state SET phase='checking',last_checked_at=?,manifest_verified_at=NULL,artifact_verified_at=NULL WHERE singleton=1").bind(db::now()).execute(&core.pool).await?;
    if let Err(e) = check_inner(core, app).await {
        sqlx::query("UPDATE update_state SET phase='failed',last_error=? WHERE singleton=1")
            .bind(e.to_string())
            .execute(&core.pool)
            .await?;
    }
    status(core).await
}
pub async fn pending(core: &Core, app: &tauri::AppHandle) -> Result<Update> {
    if crate::portable::active() {
        bail!("Installeur désactivé en mode portable.")
    }
    let channel: String = sqlx::query_scalar("SELECT channel FROM update_state WHERE singleton=1")
        .fetch_one(&core.pool)
        .await?;
    let bytes = std::fs::read(core.root.join("updates/envelope.json"))?;
    let (payload, release) = validate_envelope(&bytes, &channel)?;
    if payload.sequence < highwater(&core.root, &channel)? {
        bail!("Cette version téléchargée a été remplacée.");
    }
    let update = app
        .updater()?
        .restore_update(release)?
        .context("Aucune mise à jour applicable")?;
    let package = std::fs::read(core.root.join("updates/package.bin"))?;
    update.verify(&package)?;
    crate::release_proof::verify_bytes(&payload, &package)?;
    Ok(update)
}
pub async fn install(
    core: &mut Core,
    app: &tauri::AppHandle,
    actor: &str,
    password: &str,
) -> Result<()> {
    let update = pending(core, app).await?;
    let destination = core.root.join("backups");
    let backup =
        crate::backup::create(core, Some(actor), password, &destination, "pre_update").await?;
    db::atomic_write(
        &core.root.join("recovery/update-pending.json"),
        &serde_json::to_vec(
            &serde_json::json!({"from":env!("CARGO_PKG_VERSION"),"to":update.version,"backup":backup,"at":db::now()}),
        )?,
    )?;
    let bytes = std::fs::read(core.root.join("updates/package.bin"))?;
    sqlx::query("UPDATE update_state SET phase='installing' WHERE singleton=1")
        .execute(&core.pool)
        .await?;
    core.pool.close().await;
    let result = update.install(&bytes);
    // Sur succès Windows quitte le processus ; si erreur, réouvrir pour rester utilisable.
    core.reopen().await?;
    result?;
    Ok(())
}
