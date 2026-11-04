use crate::{audit, crypto, db, Core};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

const MAX_ENTRIES: usize = 1_000_000;
const MAX_TOTAL: u64 = 100 * 1024 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
struct Entry {
    name: String,
    size: u64,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
struct Manifest {
    format: u32,
    app_version: String,
    schema: i64,
    created_at: String,
    entries: Vec<Entry>,
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    format: u32,
    salt: String,
    key: String,
    manifest: String,
}
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Schedule {
    pub enabled: bool,
    pub local_time: String,
    pub destination: Option<String>,
    pub last_success: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct StoredSchedule {
    enabled: bool,
    local_time: String,
    destination: Option<String>,
    password_sealed: String,
    last_success: Option<String>,
}
fn digest_file(path: &Path) -> Result<(u64, String)> {
    let mut f = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    let mut size = 0;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hash.update(&buf[..n]);
    }
    Ok((size, hex::encode(hash.finalize())))
}
pub async fn schedule(core: &Core) -> Result<Schedule> {
    let saved: Option<String> =
        sqlx::query_scalar("SELECT value_json FROM settings WHERE key='backup.schedule'")
            .fetch_optional(&core.pool)
            .await?;
    if let Some(s) = saved {
        let s: StoredSchedule = serde_json::from_str(&s)?;
        Ok(Schedule {
            enabled: s.enabled,
            local_time: s.local_time,
            destination: s.destination,
            last_success: s.last_success,
        })
    } else {
        Ok(Schedule {
            local_time: "18:00".into(),
            ..Default::default()
        })
    }
}
pub async fn save_schedule(
    core: &Core,
    actor: &str,
    enabled: bool,
    time: &str,
    password: &str,
) -> Result<()> {
    crate::shared_workspace::require_local(core)?;
    chrono::NaiveTime::parse_from_str(time, "%H:%M").context("Heure invalide (HH:MM)")?;
    let existing: Option<String> =
        sqlx::query_scalar("SELECT value_json FROM settings WHERE key='backup.schedule'")
            .fetch_optional(&core.pool)
            .await?;
    let old = existing
        .map(|s| serde_json::from_str::<StoredSchedule>(&s))
        .transpose()?;
    let sealed = if !password.is_empty() {
        crypto::validate_password(password)?;
        B64.encode(crypto::seal(
            &core.key,
            password.as_bytes(),
            b"backup.schedule",
        )?)
    } else {
        old.as_ref()
            .map(|s| s.password_sealed.clone())
            .unwrap_or_default()
    };
    if enabled && sealed.is_empty() {
        bail!("Définissez une phrase de sauvegarde d’au moins 12 caractères.");
    }
    crate::scheduler::configure(&core.root, enabled, time)?;
    let saved = StoredSchedule {
        enabled,
        local_time: time.into(),
        destination: old.as_ref().and_then(|s| s.destination.clone()),
        password_sealed: sealed,
        last_success: old.and_then(|s| s.last_success),
    };
    let mut tx = core.pool.begin().await?;
    sqlx::query("INSERT INTO settings(key,value_json,modified_at,modified_by) VALUES('backup.schedule',?,?,?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,modified_at=excluded.modified_at,modified_by=excluded.modified_by")
        .bind(serde_json::to_string(&saved)?).bind(db::now()).bind(actor).execute(&mut *tx).await?;
    audit::append(
        &mut tx,
        &core.key,
        Some(actor),
        "backup.schedule",
        serde_json::json!({"enabled":enabled,"time":time}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await
}
pub async fn set_destination(core: &Core, actor: &str, path: &Path) -> Result<()> {
    crate::shared_workspace::require_local(core)?;
    let path = path.canonicalize()?;
    if path.starts_with(core.root.join("db")) || path.starts_with(core.root.join("files")) {
        bail!("Choisissez un dossier distinct des données actives.");
    }
    let saved: Option<String> =
        sqlx::query_scalar("SELECT value_json FROM settings WHERE key='backup.schedule'")
            .fetch_optional(&core.pool)
            .await?;
    let mut saved = if let Some(s) = saved {
        serde_json::from_str::<StoredSchedule>(&s)?
    } else {
        StoredSchedule {
            enabled: false,
            local_time: "18:00".into(),
            destination: None,
            password_sealed: String::new(),
            last_success: None,
        }
    };
    saved.destination = Some(path.to_string_lossy().into_owned());
    sqlx::query("INSERT INTO settings(key,value_json,modified_at,modified_by) VALUES('backup.schedule',?,?,?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,modified_at=excluded.modified_at,modified_by=excluded.modified_by")
        .bind(serde_json::to_string(&saved)?).bind(db::now()).bind(actor).execute(&core.pool).await?;
    Ok(())
}
pub async fn scheduled(core: &mut Core) -> Result<Option<String>> {
    if core.sharing.enabled.load(std::sync::atomic::Ordering::SeqCst) { return Ok(None); }
    let saved: Option<String> =
        sqlx::query_scalar("SELECT value_json FROM settings WHERE key='backup.schedule'")
            .fetch_optional(&core.pool)
            .await?;
    let Some(saved) = saved else { return Ok(None) };
    let mut settings: StoredSchedule = serde_json::from_str(&saved)?;
    if !settings.enabled {
        return Ok(None);
    }
    let now = chrono::Utc::now().with_timezone(&chrono_tz::Europe::Paris);
    let time = chrono::NaiveTime::parse_from_str(&settings.local_time, "%H:%M")?;
    let today = now.date_naive();
    let last = settings
        .last_success
        .as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono_tz::Europe::Paris).date_naive());
    if now.time() < time || last == Some(today) {
        return Ok(None);
    }
    let secret = crypto::unseal(
        &core.key,
        &B64.decode(&settings.password_sealed)?,
        b"backup.schedule",
    )?;
    let pass = std::str::from_utf8(&secret)?;
    let dest = settings
        .destination
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| core.root.join("backups"));
    let result = create(core, None, pass, &dest, "scheduled").await?;
    settings.last_success = Some(db::now());
    sqlx::query("UPDATE settings SET value_json=?,modified_at=? WHERE key='backup.schedule'")
        .bind(serde_json::to_string(&settings)?)
        .bind(db::now())
        .execute(&core.pool)
        .await?;
    Ok(Some(result))
}
pub async fn create(
    core: &mut Core,
    actor: Option<&str>,
    password: &str,
    destination: &Path,
    trigger: &str,
) -> Result<String> {
    crypto::validate_password(password)?;
    std::fs::create_dir_all(destination)?;
    let run = db::id();
    let schema: i64 =
        sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations WHERE success=1")
            .fetch_one(&core.pool)
            .await?;
    let staging = core.root.join("recovery").join(format!("backup-{run}"));
    std::fs::create_dir(&staging)?;
    sqlx::query("INSERT INTO backup_runs(id,trigger,started_at,state) VALUES(?,?,?,'running')")
        .bind(&run)
        .bind(trigger)
        .bind(db::now())
        .execute(&core.pool)
        .await?;
    let result = create_inner(core, actor, password, destination, &staging, &run).await;
    // Réouverture même en cas d'échec de copie.
    if core.pool.is_closed() {
        core.reopen().await?;
    }
    let (state, path, error) = match &result {
        Ok(p) => ("succeeded", Some(p.as_str()), None),
        Err(e) => ("failed", None, Some(e.to_string())),
    };
    sqlx::query("UPDATE backup_runs SET state=?,archive_path=?,error=?,completed_at=?,schema_version=? WHERE id=?")
        .bind(state).bind(path).bind(error).bind(db::now()).bind(schema).bind(&run).execute(&core.pool).await?;
    let mut tx = core.pool.begin().await?;
    audit::append(
        &mut tx,
        &core.key,
        actor,
        "backup.complete",
        serde_json::json!({"run":run,"state":state}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await?;
    // staging est toujours un enfant connu et généré de recovery.
    if staging.exists() {
        db::ensure_within(&staging, &core.root.join("recovery"))?;
        std::fs::remove_dir_all(&staging)?;
    }
    result
}
async fn create_inner(
    core: &mut Core,
    _actor: Option<&str>,
    password: &str,
    dest: &Path,
    stage: &Path,
    run: &str,
) -> Result<String> {
    db::check(&core.pool).await?;
    audit::verify(&core.pool, &core.key, &core.root).await?;
    let schema: i64 =
        sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations WHERE success=1")
            .fetch_one(&core.pool)
            .await?;
    core.pool.close().await;
    std::fs::copy(core.root.join("db/registre.db"), stage.join("database.bin"))?;
    let check = db::open(&stage.join("database.bin"), &core.key, false).await?;
    db::check(&check).await?;
    check.close().await;
    // Le verrou Core est détenu : aucun écrivain DB/fichier ne peut intervenir.
    let mut paths = vec![("database.bin".to_string(), stage.join("database.bin"))];
    for entry in walkdir::WalkDir::new(core.root.join("files")).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_symlink() {
            bail!("Lien symbolique interdit dans les pièces jointes.");
        }
        if entry.file_type().is_file() {
            let relative = entry
                .path()
                .strip_prefix(core.root.join("files"))?
                .to_string_lossy()
                .replace('\\', "/");
            paths.push((format!("files/{relative}"), entry.path().to_path_buf()));
        }
    }
    if paths.len() > MAX_ENTRIES {
        bail!("Trop de fichiers dans cette sauvegarde.");
    }
    let mut entries = vec![];
    let mut total = 0;
    for (name, path) in &paths {
        safe_name(name)?;
        let (size, sha256) = digest_file(path)?;
        total += size;
        if total > MAX_TOTAL {
            bail!("Sauvegarde supérieure à 100 Gio.");
        }
        entries.push(Entry {
            name: name.clone(),
            size,
            sha256,
        });
    }
    let manifest = Manifest {
        format: 1,
        app_version: env!("CARGO_PKG_VERSION").into(),
        schema,
        created_at: db::now(),
        entries,
    };
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let wrapping = crypto::derive_backup_key(password, &salt)?;
    let envelope = Envelope {
        format: 1,
        salt: B64.encode(salt),
        key: B64.encode(crypto::seal(&wrapping, &core.key, b"backup.key.v1")?),
        manifest: B64.encode(crypto::seal(
            &core.key,
            &serde_json::to_vec(&manifest)?,
            b"backup.manifest.v1",
        )?),
    };
    let name = format!(
        "SecuriteERP-{}-{run}.zip",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );
    let final_path = dest.join(name);
    let temp = final_path.with_extension("partial");
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(true);
    zip.start_file("envelope.json", options)?;
    zip.write_all(&serde_json::to_vec(&envelope)?)?;
    for (name, path) in paths {
        zip.start_file(name, options)?;
        std::io::copy(&mut File::open(path)?, &mut zip)?;
    }
    zip.finish()?.sync_all()?;
    // Vérifie l'archive écrite avant de la publier.
    let verify = stage.join("verify");
    std::fs::create_dir(&verify)?;
    let _ = extract(&temp, password, &verify)?;
    std::fs::rename(&temp, &final_path)?;
    Ok(final_path.to_string_lossy().into_owned())
}
fn safe_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 1024
        || name.contains('\\')
        || name.contains(':')
        || name.starts_with('/')
        || name
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == ".." || s.ends_with('.') || s.ends_with(' '))
    {
        bail!("Chemin interdit dans l’archive.");
    }
    Ok(())
}
fn extract(archive: &Path, password: &str, dest: &Path) -> Result<zeroize::Zeroizing<Vec<u8>>> {
    let file = File::open(archive)?;
    if file.metadata()?.len() > MAX_TOTAL + 100 * 1024 * 1024 {
        bail!("Archive trop volumineuse.");
    }
    let mut zip = ZipArchive::new(file)?;
    if zip.len() > MAX_ENTRIES + 1 {
        bail!("Archive trop volumineuse.");
    }
    let mut envelope = String::new();
    zip.by_name("envelope.json")?
        .take(32 * 1024 * 1024)
        .read_to_string(&mut envelope)?;
    let env: Envelope = serde_json::from_str(&envelope)?;
    if env.format != 1 {
        bail!("Format de sauvegarde inconnu.");
    }
    let salt = B64.decode(env.salt)?;
    if salt.len() != 16 {
        bail!("Enveloppe invalide.");
    }
    let wrapping = crypto::derive_backup_key(password, &salt)?;
    let key = crypto::unseal(&wrapping, &B64.decode(env.key)?, b"backup.key.v1")?;
    let plain = crypto::unseal(&key, &B64.decode(env.manifest)?, b"backup.manifest.v1")?;
    let manifest: Manifest = serde_json::from_slice(&plain)?;
    if manifest.format != 1 || manifest.schema > db::SCHEMA_VERSION || manifest.schema < 1 {
        bail!("Version de sauvegarde incompatible.");
    }
    if zip.len() != manifest.entries.len() + 1 {
        bail!("Contenu de sauvegarde inattendu.");
    }
    let mut names = HashSet::new();
    names.insert("envelope.json".to_string());
    let mut total = 0u64;
    for e in manifest.entries {
        safe_name(&e.name)?;
        if e.name != "database.bin" && !e.name.starts_with("files/") {
            bail!("Entrée inattendue.");
        }
        if !names.insert(e.name.to_ascii_lowercase()) {
            bail!("Entrée dupliquée.");
        }
        total = total
            .checked_add(e.size)
            .ok_or_else(|| anyhow::anyhow!("Taille invalide"))?;
        if total > MAX_TOTAL {
            bail!("Archive décompressée trop grande.");
        }
        let mut entry = zip.by_name(&e.name)?;
        if entry.size() != e.size || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            bail!("Entrée invalide.");
        }
        let target = dest.join(&e.name);
        if let Some(p) = target.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        let copied = std::io::copy(&mut (&mut entry).take(e.size + 1), &mut out)?;
        out.sync_all()?;
        let (size, hash) = digest_file(&target)?;
        if copied != e.size || size != e.size || hash != e.sha256 {
            bail!("Empreinte de sauvegarde invalide.");
        }
    }
    if !dest.join("database.bin").is_file() {
        bail!("La base est absente.");
    }
    // Vérifier qu'aucune entrée du ZIP ne possède un nom répété masqué par by_name.
    let mut actual = HashSet::new();
    for i in 0..zip.len() {
        let f = zip.by_index(i)?;
        if !actual.insert(f.name().to_ascii_lowercase()) {
            bail!("Entrée ZIP dupliquée.");
        }
    }
    Ok(key)
}
pub async fn stage_restore(
    core: &Core,
    actor: &str,
    archive: &Path,
    password: &str,
) -> Result<String> {
    let rid = validate_restore(&core.root, archive, password).await?;
    let mut tx = core.pool.begin().await?;
    audit::append(
        &mut tx,
        &core.key,
        Some(actor),
        "restore.prepare",
        serde_json::json!({"restore_id":rid}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&core.pool, &core.root).await?;
    Ok(rid)
}
pub async fn validate_restore(root: &Path, archive: &Path, password: &str) -> Result<String> {
    let rid = db::id();
    let stage = root.join("recovery").join(format!("restore-{rid}"));
    std::fs::create_dir_all(&stage)?;
    let key = extract(archive, password, &stage)?;
    std::fs::create_dir(stage.join("db"))?;
    std::fs::rename(stage.join("database.bin"), stage.join("db/registre.db"))?;
    std::fs::create_dir_all(stage.join("files"))?;
    let restored = db::open(&stage.join("db/registre.db"), &key, false).await?;
    db::check(&restored).await?;
    audit::verify(&restored, &key, &stage).await?;
    // Le retour à un ancien snapshot ne doit pas ressusciter des sessions.
    sqlx::query("UPDATE sessions SET revoked_at=?")
        .bind(db::now())
        .execute(&restored)
        .await?;
    restored.close().await;
    if crate::portable::applies(root) {
        crate::portable::stage_key(&stage, &key)?
    } else {
        db::atomic_write(&stage.join("master.dpapi"), &crypto::protect(&key)?)?;
    }
    db::atomic_write(&stage.join("validated"), b"v1")?;
    Ok(rid)
}
pub fn request_restore(root: &Path, rid: &str) -> Result<()> {
    uuid::Uuid::parse_str(rid)?;
    db::atomic_write(
        &root.join("recovery/pending-restore.json"),
        &serde_json::to_vec(&serde_json::json!({"id":rid}))?,
    )
}
pub fn apply_pending(root: &Path) -> Result<Option<String>> {
    let marker = root.join("recovery/pending-restore.json");
    if !marker.exists() {
        return Ok(None);
    }
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&marker)?)?;
    let rid = value["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Marqueur de restauration invalide"))?;
    uuid::Uuid::parse_str(rid)?;
    let stage = root.join("recovery").join(format!("restore-{rid}"));
    if !stage.join("validated").is_file() {
        bail!("Restauration non validée. Données originales conservées.");
    }
    let old = root.join("recovery").join(format!("before-restore-{rid}"));
    std::fs::create_dir_all(&old)?;
    // Reprise idempotente : un composant déjà transféré ne sera pas échangé deux fois.
    let portable = root.join("master.portable").exists();
    for component in [
        "db",
        "files",
        if portable {
            "master.portable"
        } else {
            "master.dpapi"
        },
        "audit.dpapi",
    ] {
        let source = stage.join(component);
        let target = root.join(component);
        let original = old.join(component);
        if source.exists() {
            db::ensure_within(&source, root)?;
            db::ensure_within(&target, root)?;
            db::ensure_within(&original, root)?;
            if target.exists() && !original.exists() {
                std::fs::rename(&target, &original)?;
            }
            if target.exists() {
                bail!(
                    "Restauration interrompue : composant ambigu, récupération manuelle requise."
                );
            }
            std::fs::rename(source, target)?;
        } else if !target.exists() {
            bail!("Restauration incomplète : composant absent.");
        }
    }
    std::fs::remove_file(marker)?;
    Ok(Some(
        "Sauvegarde restaurée. Les données précédentes restent dans le dossier de récupération."
            .into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn traversal_rejected() {
        for name in [
            "../x",
            "/x",
            "files/../../x",
            "C:/x",
            "files\\x",
            "files/x.",
        ] {
            assert!(safe_name(name).is_err(), "{name}");
        }
        assert!(safe_name("files/123.blob").is_ok());
    }
}
