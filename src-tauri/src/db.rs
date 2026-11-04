use anyhow::{bail, Context, Result};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    ConnectOptions, SqlitePool,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use zeroize::Zeroizing;

pub const SCHEMA_VERSION: i64 = 14;

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn ensure_within(path: &Path, base: &Path) -> Result<()> {
    let base = base.canonicalize()?;
    let resolved = if path.exists() {
        path.canonicalize()?
    } else {
        path.parent()
            .context("Dossier parent absent")?
            .canonicalize()?
            .join(path.file_name().context("Nom absent")?)
    };
    if resolved == base || !resolved.starts_with(&base) {
        bail!("Le chemin sort du dossier de travail autorisé.");
    }
    Ok(())
}
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let temp = path.with_extension(format!("{}.tmp", id()));
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    // Renommer un fichier existant n'est pas atomique sous Windows via std::fs::rename.
    // Les journaux de reprise utilisent des fichiers à nom unique ; pour l'ancre, DPAPI
    // plus remplacement Windows write-through assure un changement atomique.
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                windows_sys::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING
                    | windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    #[cfg(not(windows))]
    std::fs::rename(temp, path)?;
    Ok(())
}
pub fn prepare(root: &Path) -> Result<Zeroizing<Vec<u8>>> {
    for dir in ["db", "files", "backups", "updates", "recovery"] {
        std::fs::create_dir_all(root.join(dir))?;
    }
    if crate::portable::applies(root) {
        return crate::portable::prepare(root);
    }
    let key_path = root.join("master.dpapi");
    if key_path.exists() {
        return crate::crypto::unprotect(&std::fs::read(key_path)?);
    }
    if root.join("db/registre.db").exists() {
        bail!("La clé Windows est absente. Utilisez une sauvegarde de récupération.");
    }
    let key = crate::crypto::random_key();
    atomic_write(&key_path, &crate::crypto::protect(&key)?)?;
    Ok(key)
}
pub async fn open(path: &Path, key: &[u8], create: bool) -> Result<SqlitePool> {
    if key.len() != 32 {
        bail!("Clé SQLCipher invalide.");
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(create)
        .pragma("key", format!("\"x'{}'\"", hex::encode(key)))
        .journal_mode(SqliteJournalMode::Delete)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(10))
        .disable_statement_logging();
    // SQLx positionne le PRAGMA key en tête de sa liste de PRAGMA initiaux.
    // Pas de preload du plugin officiel : son pool standard ne connaît pas la clé.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .context("Ouverture de la base chiffrée impossible")?;
    let cipher: Option<String> = sqlx::query_scalar("PRAGMA cipher_version")
        .fetch_optional(&pool)
        .await?;
    if cipher.as_deref().unwrap_or("").is_empty() {
        pool.close().await;
        bail!("SQLCipher absent : refus d’utiliser une base non chiffrée.");
    }
    sqlx::query("SELECT count(*) FROM sqlite_master")
        .fetch_one(&pool)
        .await?;
    Ok(pool)
}
pub async fn migrate(pool: &SqlitePool) -> Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    check(pool).await
}
pub async fn check(pool: &SqlitePool) -> Result<()> {
    let result: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(pool)
        .await?;
    if result != "ok" {
        bail!("L’intégrité de la base est invalide.");
    }
    if !sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(pool)
        .await?
        .is_empty()
    {
        bail!("Relations de données invalides.");
    }
    // SQLCipher ne renvoie aucune ligne quand les HMAC sont valides.
    let cipher = sqlx::query("PRAGMA cipher_integrity_check")
        .fetch_all(pool)
        .await?;
    if !cipher.is_empty() {
        bail!("Contrôle cryptographique SQLCipher en échec.");
    }
    Ok(())
}
pub async fn before_migration(root: &Path, pool: &SqlitePool) -> Result<Option<PathBuf>> {
    let exists: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE name='_sqlx_migrations'")
            .fetch_one(pool)
            .await?;
    if exists == 0 {
        return Ok(None);
    }
    let latest: i64 =
        sqlx::query_scalar("SELECT coalesce(max(version),0) FROM _sqlx_migrations WHERE success=1")
            .fetch_one(pool)
            .await?;
    if latest > SCHEMA_VERSION {
        bail!("Cette base provient d’une version plus récente. Aucune modification effectuée.");
    }
    if latest == SCHEMA_VERSION {
        return Ok(None);
    }
    // journal DELETE + aucune écriture métier au démarrage ; connexion unique.
    let destination = root
        .join("recovery")
        .join(format!("pre-migration-{}.db", id()));
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(pool)
        .await?;
    std::fs::copy(root.join("db/registre.db"), &destination)?;
    Ok(Some(destination))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn vector_map_upgrade_preserves_existing_maps_and_validates_insert_update() {
        let root = std::env::temp_dir().join(format!("erp-map-upgrade-{}", id()));
        let migrations = root.join("old-migrations");
        std::fs::create_dir_all(&migrations).unwrap();
        for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations")).unwrap()
        {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".sql") && name[..4].parse::<i64>().unwrap() <= 13 {
                std::fs::copy(entry.path(), migrations.join(name)).unwrap();
            }
        }
        std::fs::create_dir_all(root.join("db")).unwrap();
        std::fs::create_dir_all(root.join("recovery")).unwrap();
        let path = root.join("db/registre.db");
        let key = crate::crypto::random_key();
        let pool = open(&path, &key, true).await.unwrap();
        sqlx::migrate::Migrator::new(migrations.as_path())
            .await
            .unwrap()
            .run(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO organisations(id,name,created_at) VALUES('org','Test','2026-11-04')",
        )
        .execute(&pool)
        .await
        .unwrap();
        for (id, source) in [("old-osm", "osm"), ("old-ign", "ign")] {
            sqlx::query("INSERT INTO entities(id,organisation_id,kind,created_at) VALUES(?,'org','building','2026-11-04')").bind(id).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO buildings(id,organisation_id,name,address,erp_category,capacity,location_json) VALUES(?,'org',?,'Test',5,10,?)")
                .bind(id).bind(id).bind(serde_json::json!({"plan":{"source":source,"features":[]}}).to_string()).execute(&pool).await.unwrap();
        }
        let vector = r#"{"plan":{"source":"osm-vector","features":[]}}"#;
        assert!(
            sqlx::query("UPDATE buildings SET location_json=? WHERE id='old-osm'")
                .bind(vector)
                .execute(&pool)
                .await
                .unwrap_err()
                .to_string()
                .contains("Unknown map provenance")
        );
        let before: Vec<(String, String)> =
            sqlx::query_as("SELECT id,location_json FROM buildings ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        let snapshot = before_migration(&root, &pool).await.unwrap().unwrap();
        assert!(snapshot.exists());
        migrate(&pool).await.unwrap();
        migrate(&pool).await.unwrap();
        let after: Vec<(String, String)> =
            sqlx::query_as("SELECT id,location_json FROM buildings ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(before, after);
        sqlx::query("UPDATE buildings SET location_json=? WHERE id='old-osm'")
            .bind(vector)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO entities(id,organisation_id,kind,created_at) VALUES('new','org','building','2026-11-04')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO buildings(id,organisation_id,name,address,erp_category,capacity,location_json) VALUES('new','org','New','Test',5,10,?)").bind(vector).execute(&pool).await.unwrap();
        let invalid = r#"{"plan":{"source":"unknown"}}"#;
        assert!(
            sqlx::query("UPDATE buildings SET location_json=? WHERE id='new'")
                .bind(invalid)
                .execute(&pool)
                .await
                .is_err()
        );
        sqlx::query("INSERT INTO entities(id,organisation_id,kind,created_at) VALUES('bad','org','building','2026-11-04')").execute(&pool).await.unwrap();
        assert!(sqlx::query("INSERT INTO buildings(id,organisation_id,name,address,erp_category,capacity,location_json) VALUES('bad','org','Bad','Test',5,10,?)").bind(invalid).execute(&pool).await.is_err());
        check(&pool).await.unwrap();
        pool.close().await;
        let reopened = open(&path, &key, false).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT location_json FROM buildings WHERE id='new'")
                .fetch_one(&reopened)
                .await
                .unwrap(),
            vector
        );
        reopened.close().await;
        ensure_within(&root, &std::env::temp_dir()).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn encryption_and_migrations() {
        let root = std::env::temp_dir().join(format!("erp-test-{}", id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("test.db");
        let key = crate::crypto::random_key();
        let p = open(&path, &key, true).await.unwrap();
        migrate(&p).await.unwrap();
        p.close().await;
        assert_ne!(&std::fs::read(&path).unwrap()[..16], b"SQLite format 3\0");
        assert!(open(&path, &crate::crypto::random_key(), false)
            .await
            .is_err());
        let p = open(&path, &key, false).await.unwrap();
        check(&p).await.unwrap();
        p.close().await;
        ensure_within(&root, &std::env::temp_dir()).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
