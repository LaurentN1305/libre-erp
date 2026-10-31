mod reports;
mod csv_import;
mod alerts;
mod audit;
mod branding;
mod contracts;
mod documents;
mod exchange;
mod inventory;
mod maps;
mod maps_ign;
mod portable;
mod preferences;
pub fn portable_start() -> anyhow::Result<()> {
    portable::unlock_console()?;
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Console::FreeConsole();
    }
    run();
    Ok(())
}
mod auth;
mod backup;
mod crypto;
mod dashboard;
mod db;
#[cfg(all(test, windows))]
mod integration_tests;
mod recovery;
mod register;
mod scheduler;
mod release_proof;
mod update_transport;
mod updates;
mod vault;
mod works;
pub fn recover_console() -> anyhow::Result<()> {
    recovery::run()
}

use anyhow::{bail, Result};
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::Mutex;
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Core {
    root: PathBuf,
    pool: SqlitePool,
    key: Zeroizing<Vec<u8>>,
    dummy_hash: String,
    recovery: Option<String>,
    pending_mfa: HashMap<String, (Zeroizing<Vec<u8>>, i64)>,
    update_busy: Arc<AtomicBool>,
    _process_lock: Arc<std::fs::File>,
    app: Option<tauri::AppHandle>,
}
impl Core {
    async fn register_sql_pool(&self) {
        if let Some(app) = &self.app {
            let instances = app.state::<tauri_plugin_sql::DbInstances>();
            instances.0.write().await.insert(
                "sqlcipher:private".into(),
                tauri_plugin_sql::DbPool::Sqlite(self.pool.clone()),
            );
        }
    }
    async fn reopen(&mut self) -> Result<()> {
        self.pool = db::open(&self.root.join("db/registre.db"), &self.key, false).await?;
        self.register_sql_pool().await;
        Ok(())
    }
}
type Shared = Arc<Mutex<Core>>;
type Reply<T> = std::result::Result<T, String>;
fn response<T>(result: Result<T>) -> Reply<T> {
    result.map_err(|e| e.to_string())
}
fn not_downloading(core: &Core) -> Result<()> {
    if core.update_busy.load(Ordering::SeqCst) {
        bail!("Attendez la fin du téléchargement avant cette opération.");
    }
    Ok(())
}
#[derive(Serialize)]
struct Status {
    portable: bool,
    first_run: bool,
    version: String,
    database: String,
    storage: String,
    cipher: String,
    recovery: Option<String>,
}
#[tauri::command]
async fn app_status(state: State<'_, Shared>) -> Reply<Status> {
    response(
        async {
            let c = state.lock().await;
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
                .fetch_one(&c.pool)
                .await?;
            Ok(Status {
                portable: portable::active(),
                first_run: count == 0,
                version: env!("CARGO_PKG_VERSION").into(),
                database: "SQLCipher · stockage local chiffré".into(),
                storage: c.root.to_string_lossy().into_owned(),
                cipher: sqlx::query_scalar("PRAGMA cipher_version")
                    .fetch_one(&c.pool)
                    .await?,
                recovery: c.recovery.clone(),
            })
        }
        .await,
    )
}
#[tauri::command]
async fn bootstrap(
    state: State<'_, Shared>,
    organisation: String,
    name: String,
    login: String,
    password: String,
) -> Reply<auth::Session> {
    let p = Zeroizing::new(password);
    response(auth::bootstrap(&*state.lock().await, &organisation, &name, &login, &p).await)
}
#[tauri::command]
async fn login(
    state: State<'_, Shared>,
    login: String,
    password: String,
    otp: Option<String>,
) -> Reply<auth::Session> {
    let p = Zeroizing::new(password);
    response(auth::login(&*state.lock().await, &login, &p, otp.as_deref()).await)
}
#[tauri::command]
async fn logout(state: State<'_, Shared>, token: String) -> Reply<()> {
    response(auth::logout(&*state.lock().await, &token).await)
}
#[tauri::command]
async fn users(state: State<'_, Shared>, token: String) -> Reply<Vec<auth::User>> {
    response(auth::list_users(&*state.lock().await, &token).await)
}
#[tauri::command]
async fn user_create(
    state: State<'_, Shared>,
    token: String,
    name: String,
    login: String,
    password: String,
    role: String,
) -> Reply<()> {
    let p = Zeroizing::new(password);
    response(auth::create_user(&*state.lock().await, &token, &name, &login, &p, &role).await)
}
#[tauri::command]
async fn user_active(
    state: State<'_, Shared>,
    token: String,
    user_id: String,
    active: bool,
) -> Reply<()> {
    response(auth::set_active(&*state.lock().await, &token, &user_id, active).await)
}
#[tauri::command]
async fn password_change(
    state: State<'_, Shared>,
    token: String,
    current: String,
    new: String,
) -> Reply<()> {
    let current = Zeroizing::new(current);
    let new = Zeroizing::new(new);
    response(auth::password_change(&*state.lock().await, &token, &current, &new).await)
}
#[tauri::command]
async fn mfa_begin(
    state: State<'_, Shared>,
    token: String,
    password: String,
) -> Reply<serde_json::Value> {
    let p = Zeroizing::new(password);
    response(auth::begin_mfa(&mut *state.lock().await, &token, &p).await)
}
#[tauri::command]
async fn mfa_confirm(state: State<'_, Shared>, token: String, code: String) -> Reply<Vec<String>> {
    response(auth::confirm_mfa(&mut *state.lock().await, &token, &code).await)
}
#[tauri::command]
async fn audit_list(state: State<'_, Shared>, token: String) -> Reply<Vec<serde_json::Value>> {
    response(async{let c=state.lock().await;auth::require(&c,&token,Some("audit.read")).await?;
        let rows=sqlx::query("SELECT a.sequence,a.occurred_at,a.action,u.display_name FROM audit_events a LEFT JOIN users u ON u.id=a.actor_id ORDER BY a.sequence DESC LIMIT 200").fetch_all(&c.pool).await?;
        Ok(rows.into_iter().map(|r|serde_json::json!({"sequence":r.get::<i64,_>(0),"occurred_at":r.get::<String,_>(1),"action":r.get::<String,_>(2),"actor":r.get::<Option<String>,_>(3)})).collect())}.await)
}
#[tauri::command]
async fn audit_verify(state: State<'_, Shared>, token: String) -> Reply<()> {
    response(
        async {
            let c = state.lock().await;
            auth::require(&c, &token, Some("audit.read")).await?;
            audit::verify(&c.pool, &c.key, &c.root).await
        }
        .await,
    )
}
#[tauri::command]
async fn backup_settings(state: State<'_, Shared>, token: String) -> Reply<backup::Schedule> {
    response(
        async {
            let c = state.lock().await;
            auth::require(&c, &token, Some("backup.manage")).await?;
            backup::schedule(&c).await
        }
        .await,
    )
}
#[tauri::command]
async fn backup_schedule(
    state: State<'_, Shared>,
    token: String,
    enabled: bool,
    time: String,
    password: String,
) -> Reply<()> {
    let p = Zeroizing::new(password);
    response(
        async {
            let c = state.lock().await;
            let actor = auth::require(&c, &token, Some("backup.manage")).await?;
            backup::save_schedule(&c, &actor, enabled, &time, &p).await
        }
        .await,
    )
}
#[tauri::command]
async fn backup_folder(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
) -> Reply<Option<String>> {
    response(
        async {
            let c = state.lock().await;
            let actor = auth::require(&c, &token, Some("backup.manage")).await?;
            let folder = app
                .dialog()
                .file()
                .set_title("Choisir le dossier des sauvegardes")
                .blocking_pick_folder();
            if let Some(folder) = folder {
                let path = folder.into_path()?;
                backup::set_destination(&c, &actor, &path).await?;
                Ok(Some(path.to_string_lossy().into_owned()))
            } else {
                Ok(None)
            }
        }
        .await,
    )
}
#[tauri::command]
async fn backup_now(state: State<'_, Shared>, token: String, password: String) -> Reply<String> {
    let p = Zeroizing::new(password);
    response(
        async {
            let mut c = state.lock().await;
            not_downloading(&c)?;
            let actor = auth::require(&c, &token, Some("backup.manage")).await?;
            let settings = backup::schedule(&c).await?;
            let dest = settings
                .destination
                .map(PathBuf::from)
                .unwrap_or_else(|| c.root.join("backups"));
            backup::create(&mut c, Some(&actor), &p, &dest, "manual").await
        }
        .await,
    )
}
#[tauri::command]
async fn restore_prepare(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    password: String,
) -> Reply<Option<String>> {
    let p = Zeroizing::new(password);
    response(
        async {
            let c = state.lock().await;
            not_downloading(&c)?;
            let actor = auth::require(&c, &token, Some("backup.manage")).await?;
            let file = app
                .dialog()
                .file()
                .set_title("Choisir une sauvegarde Libre ERP")
                .add_filter("Sauvegarde ZIP", &["zip"])
                .blocking_pick_file();
            if let Some(file) = file {
                Ok(Some(
                    backup::stage_restore(&c, &actor, &file.into_path()?, &p).await?,
                ))
            } else {
                Ok(None)
            }
        }
        .await,
    )
}
#[tauri::command]
async fn restore_apply(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    restore_id: String,
    password: String,
) -> Reply<()> {
    let p = Zeroizing::new(password);
    response(
        async {
            let mut c = state.lock().await;
            not_downloading(&c)?;
            let actor = auth::require(&c, &token, Some("backup.manage")).await?;
            // Sauvegarde chiffrée obligatoire de l'état actuel avant tout remplacement.
            let dest = c.root.join("backups");
            backup::create(&mut c, Some(&actor), &p, &dest, "pre_restore").await?;
            backup::request_restore(&c.root, &restore_id)?;
            c.pool.close().await;
            app.restart();
        }
        .await,
    )
}
#[tauri::command]
async fn update_status(state: State<'_, Shared>, token: String) -> Reply<updates::Status> {
    response(
        async {
            let c = state.lock().await;
            auth::require(&c, &token, Some("settings.read")).await?;
            updates::status(&c).await
        }
        .await,
    )
}
#[tauri::command]
async fn update_channel(state: State<'_, Shared>, token: String, channel: String) -> Reply<()> {
    response(
        async {
            let c = state.lock().await;
            not_downloading(&c)?;
            updates::channel(&c, &token, &channel).await
        }
        .await,
    )
}
fn dispatch_check(core: Core, app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let _ = updates::check(&core, &app).await;
        core.update_busy.store(false, Ordering::SeqCst);
    });
}
#[tauri::command]
async fn update_check(app: tauri::AppHandle, state: State<'_, Shared>, token: String) -> Reply<()> {
    response(
        async {
            let c = state.lock().await;
            auth::require(&c, &token, Some("updates.manage")).await?;
            if c.update_busy.swap(true, Ordering::SeqCst) {
                bail!("Une vérification est déjà en cours.");
            }
            dispatch_check(c.clone(), app);
            Ok(())
        }
        .await,
    )
}
#[tauri::command]
async fn update_install(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    password: String,
) -> Reply<()> {
    let p = Zeroizing::new(password);
    response(
        async {
            let mut c = state.lock().await;
            not_downloading(&c)?;
            let actor = auth::require(&c, &token, Some("updates.manage")).await?;
            updates::install(&mut c, &app, &actor, &p).await
        }
        .await,
    )
}

#[tauri::command]
async fn dashboard_snapshot(state: State<'_, Shared>, token: String) -> Reply<serde_json::Value> {
    response(dashboard::snapshot(&*state.lock().await, &token).await)
}
#[tauri::command]
async fn building_save(
    state: State<'_, Shared>,
    token: String,
    input: dashboard::BuildingInput,
) -> Reply<String> {
    response(dashboard::save(&*state.lock().await, &token, input).await)
}
#[tauri::command]
async fn building_archive(
    state: State<'_, Shared>,
    token: String,
    id: String,
    archived: bool,
) -> Reply<()> {
    response(dashboard::archive(&*state.lock().await, &token, &id, archived).await)
}
#[tauri::command]
async fn building_photo(
    state: State<'_, Shared>,
    token: String,
    id: String,
) -> Reply<Option<String>> {
    response(dashboard::photo(&*state.lock().await, &token, &id).await)
}
#[tauri::command]
async fn building_photo_set(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    id: String,
    remove: bool,
) -> Reply<()> {
    response(
        async {
            let c = state.lock().await;
            let actor = auth::require(&c, &token, None).await?;
            if !dashboard::allowed(&c, &actor, &id, "buildings.write").await? {
                bail!("Accès refusé.");
            }
            let path = if remove {
                None
            } else {
                let selected = app
                    .dialog()
                    .file()
                    .set_title("Photo du bâtiment · PNG ou JPEG, 8 Mio maximum")
                    .add_filter("Photo", &["png", "jpg", "jpeg"])
                    .blocking_pick_file();
                match selected {
                    Some(f) => Some(f.into_path()?),
                    None => return Ok(()),
                }
            };
            dashboard::set_photo(&c, &token, &id, path).await
        }
        .await,
    )
}

#[tauri::command]
async fn user_scope(
    state: State<'_, Shared>,
    token: String,
    user_id: String,
    buildings: Vec<String>,
) -> Reply<()> {
    response(dashboard::set_scope(&*state.lock().await, &token, &user_id, buildings).await)
}

#[tauri::command]
async fn register_snapshot(
    state: State<'_, Shared>,
    token: String,
    building_id: String,
) -> Reply<serde_json::Value> {
    response(register::snapshot(&*state.lock().await, &token, &building_id).await)
}
#[tauri::command]
async fn register_save(
    state: State<'_, Shared>,
    token: String,
    input: register::Input,
) -> Reply<String> {
    response(register::save(&*state.lock().await, &token, input).await)
}
#[tauri::command]
async fn register_history(
    state: State<'_, Shared>,
    token: String,
    id: String,
) -> Reply<serde_json::Value> {
    response(register::history(&*state.lock().await, &token, &id).await)
}
#[tauri::command]
async fn register_archive(
    state: State<'_, Shared>,
    token: String,
    id: String,
    archived: bool,
) -> Reply<()> {
    response(register::archive(&*state.lock().await, &token, &id, archived).await)
}
#[tauri::command]
async fn register_attachment(
    state: State<'_, Shared>,
    token: String,
    building_id: String,
    file_id: String,
) -> Reply<serde_json::Value> {
    response(register::attachment(&*state.lock().await, &token, &building_id, &file_id).await)
}
#[tauri::command]
async fn register_attach(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    id: String,
) -> Reply<()> {
    response(
        async {
            let c = state.lock().await;
            let building: String =
                sqlx::query_scalar("SELECT building_id FROM register_entries WHERE id=?")
                    .bind(&id)
                    .fetch_one(&c.pool)
                    .await?;
            register::access(&c, &token, &building, true).await?;
            let selected = app
                .dialog()
                .file()
                .set_title("Joindre un document au registre · 20 Mio maximum")
                .add_filter("PDF et images", &["pdf", "png", "jpg", "jpeg"])
                .blocking_pick_file();
            if let Some(file) = selected {
                register::attach(&c, &token, &id, &file.into_path()?).await?;
            }
            Ok(())
        }
        .await,
    )
}
#[tauri::command]
async fn register_export_pdf(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    building_id: String,
    bytes: String,
) -> Reply<Option<String>> {
    response(
        async {
            use base64::Engine;
            let c = state.lock().await;
            let actor = register::access(&c, &token, &building_id, false).await?;
            if bytes.len() > 280 * 1024 * 1024 {
                bail!("Export supérieur à 200 Mio.");
            }
            let data = base64::engine::general_purpose::STANDARD.decode(bytes)?;
            if !data.starts_with(b"%PDF-") {
                bail!("PDF invalide.");
            }
            let selected = app
                .dialog()
                .file()
                .set_title("Enregistrer le registre imprimable")
                .set_file_name("Registre-securite.pdf")
                .add_filter("PDF", &["pdf"])
                .blocking_save_file();
            if let Some(file) = selected {
                let path = file.into_path()?;
                std::fs::write(&path, &data)?;
                let hash = {
                    use sha2::Digest;
                    hex::encode(sha2::Sha256::digest(&data))
                };
                let mut tx = c.pool.begin().await?;
                audit::append(
                    &mut tx,
                    &c.key,
                    Some(&actor),
                    "register.export",
                    serde_json::json!({"building_id":building_id,"sha256":hash}),
                )
                .await?;
                tx.commit().await?;
                audit::checkpoint(&c.pool, &c.root).await?;
                Ok(Some(path.to_string_lossy().into_owned()))
            } else {
                Ok(None)
            }
        }
        .await,
    )
}

#[tauri::command]
async fn building_exchange_export(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    building_id: String,
    password: String,
) -> Reply<Option<String>> {
    let password = Zeroizing::new(password);
    response(
        async {
            let c = state.lock().await;
            not_downloading(&c)?;
            exchange::access(&c, &token).await?;
            let selected = app
                .dialog()
                .file()
                .set_title("Exporter un bâtiment")
                .set_file_name("Batiment.erp-partage")
                .add_filter("Partage de bâtiment", &["erp-partage"])
                .blocking_save_file();
            if let Some(f) = selected {
                let p = f.into_path()?;
                exchange::export(&c, &token, &building_id, &password, &p).await?;
                Ok(Some(p.to_string_lossy().into_owned()))
            } else {
                Ok(None)
            }
        }
        .await,
    )
}
#[tauri::command]
async fn building_exchange_preview(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    password: String,
) -> Reply<Option<exchange::Preview>> {
    let password = Zeroizing::new(password);
    response(
        async {
            let c = state.lock().await;
            exchange::access(&c, &token).await?;
            let selected = app
                .dialog()
                .file()
                .set_title("Choisir un bâtiment reçu")
                .add_filter("Partage de bâtiment", &["erp-partage"])
                .blocking_pick_file();
            if let Some(f) = selected {
                Ok(Some(
                    exchange::preview(&c, &token, &f.into_path()?, &password).await?,
                ))
            } else {
                Ok(None)
            }
        }
        .await,
    )
}
#[tauri::command]
async fn building_exchange_import(
    state: State<'_, Shared>,
    token: String,
    path: String,
    password: String,
    digest: String,
    local_fingerprint: Option<String>,
    replace: bool,
) -> Reply<String> {
    let password = Zeroizing::new(password);
    response(
        async {
            let c = state.lock().await;
            not_downloading(&c)?;
            exchange::import(
                &c,
                &token,
                std::path::Path::new(&path),
                &password,
                &digest,
                local_fingerprint.as_deref(),
                replace,
            )
            .await
        }
        .await,
    )
}

#[tauri::command]
async fn work_simple_transition(
    state: State<'_, Shared>,
    token: String,
    id: String,
    version: i64,
    done: bool,
    reason: String,
) -> Reply<()> {
    response(
        works::simple_transition(&*state.lock().await, &token, &id, version, done, &reason).await,
    )
}

#[tauri::command]
async fn map_geocode(
    state: State<'_, Shared>,
    token: String,
    address: String,
) -> Reply<Vec<maps::Candidate>> {
    response(maps::geocode(&*state.lock().await, &token, &address).await)
}
#[tauri::command]
async fn map_context(
    state: State<'_, Shared>,
    token: String,
    lat: f64,
    lon: f64,
) -> Reply<maps::Plan> {
    response(maps::context(&*state.lock().await, &token, lat, lon).await)
}
#[tauri::command]
async fn map_diagnostics(state: State<'_, Shared>, token: String) -> Reply<Vec<maps::Diagnostic>> {
    response(maps::diagnostics(&*state.lock().await, &token).await)
}
#[tauri::command]
async fn map_settings(state: State<'_, Shared>, token: String) -> Reply<maps::Services> {
    response(
        async {
            let c = state.lock().await;
            auth::require(&c, &token, Some("users.manage")).await?;
            maps::settings(&c).await
        }
        .await,
    )
}
#[tauri::command]
async fn map_settings_save(
    state: State<'_, Shared>,
    token: String,
    input: maps::Services,
) -> Reply<()> {
    response(maps::save_settings(&*state.lock().await, &token, input).await)
}

#[tauri::command]
fn map_open_licence(source: Option<String>) -> Reply<()> {
    response((|| -> Result<()> {
        let url = match source.as_deref().unwrap_or("osm") {
            "osm" => "https://www.openstreetmap.org/copyright",
            "ign" => "https://geoservices.ign.fr/bdtopo",
            _ => bail!("Provenance inconnue"),
        };
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let path = std::path::PathBuf::from(
                std::env::var_os("SystemRoot")
                    .ok_or_else(|| anyhow::anyhow!("Dossier Windows absent"))?,
            )
            .join("explorer.exe");
            std::process::Command::new(path)
                .arg(url)
                .creation_flags(0x08000000)
                .spawn()?;
        }
        Ok(())
    })())
}

#[tauri::command]
async fn app_icon_refresh(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
) -> Reply<String> {
    response(
        async {
            let root = {
                let c = state.lock().await;
                auth::require(&c, &token, None).await?;
                c.root.clone()
            };
            tauri::async_runtime::spawn_blocking(move || branding::refresh(&app, &root)).await?
        }
        .await,
    )
}

#[tauri::command]
async fn inventory_snapshot(state: State<'_, Shared>, token: String) -> Reply<serde_json::Value> {
    response(inventory::snapshot(&*state.lock().await, &token).await)
}
#[tauri::command]
async fn inventory_detail(
    state: State<'_, Shared>,
    token: String,
    id: String,
) -> Reply<serde_json::Value> {
    response(inventory::detail(&*state.lock().await, &token, &id).await)
}
#[tauri::command]
async fn inventory_save(
    state: State<'_, Shared>,
    token: String,
    input: inventory::Input,
) -> Reply<String> {
    response(inventory::save(&*state.lock().await, &token, input).await)
}
#[tauri::command]
async fn inventory_archive(
    state: State<'_, Shared>,
    token: String,
    id: String,
    version: i64,
    archived: bool,
    reason: String,
) -> Reply<()> {
    response(
        inventory::archive(
            &*state.lock().await,
            &token,
            &id,
            version,
            archived,
            &reason,
        )
        .await,
    )
}
#[tauri::command]
async fn inventory_location_add(
    state: State<'_, Shared>,
    token: String,
    building: String,
    parent: Option<String>,
    kind: String,
    name: String,
) -> Reply<String> {
    response(
        inventory::location_add(
            &*state.lock().await,
            &token,
            &building,
            parent,
            &kind,
            &name,
        )
        .await,
    )
}
#[tauri::command]
async fn inventory_category_add(
    state: State<'_, Shared>,
    token: String,
    parent: Option<String>,
    name: String,
) -> Reply<String> {
    response(inventory::category_add(&*state.lock().await, &token, parent, &name).await)
}
#[tauri::command]
async fn inventory_maintenance(
    state: State<'_, Shared>,
    token: String,
    id: String,
    version: i64,
    day: String,
    next: Option<String>,
    work: Option<String>,
    reason: String,
) -> Reply<()> {
    response(
        inventory::maintenance(
            &*state.lock().await,
            &token,
            &id,
            version,
            day,
            next,
            work,
            &reason,
        )
        .await,
    )
}
#[tauri::command]
async fn inventory_reading(
    state: State<'_, Shared>,
    token: String,
    input: inventory::ReadingInput,
) -> Reply<()> {
    response(inventory::reading(&*state.lock().await, &token, input).await)
}
#[tauri::command]
async fn inventory_reading_cancel(
    state: State<'_, Shared>,
    token: String,
    id: String,
    version: i64,
    reading: String,
    reason: String,
) -> Reply<()> {
    response(
        inventory::reading_cancel(
            &*state.lock().await,
            &token,
            &id,
            version,
            &reading,
            &reason,
        )
        .await,
    )
}
#[tauri::command]
async fn inventory_history(
    state: State<'_, Shared>,
    token: String,
    id: String,
    offset: i64,
) -> Reply<serde_json::Value> {
    response(inventory::history(&*state.lock().await, &token, &id, offset).await)
}
#[tauri::command]
async fn inventory_search(
    state: State<'_, Shared>,
    token: String,
    query: String,
) -> Reply<serde_json::Value> {
    response(inventory::search(&*state.lock().await, &token, &query).await)
}
#[tauri::command]
async fn inventory_photo(
    state: State<'_, Shared>,
    token: String,
    id: String,
    file: String,
) -> Reply<serde_json::Value> {
    response(inventory::photo(&*state.lock().await, &token, &id, &file).await)
}
#[tauri::command]
async fn inventory_photo_attach(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    id: String,
    version: i64,
) -> Reply<()> {
    response(
        async {
            let c = state.lock().await;
            inventory::access(&c, &token, &id, true).await?;
            if let Some(f) = app
                .dialog()
                .file()
                .add_filter("Photos", &["png", "jpg", "jpeg"])
                .blocking_pick_file()
            {
                inventory::photo_attach(&c, &token, &id, version, &f.into_path()?).await?;
            }
            Ok(())
        }
        .await,
    )
}
#[tauri::command]
async fn inventory_photo_remove(
    state: State<'_, Shared>,
    token: String,
    id: String,
    version: i64,
    file: String,
    reason: String,
) -> Reply<()> {
    response(
        inventory::photo_remove(&*state.lock().await, &token, &id, version, &file, &reason).await,
    )
}
#[tauri::command]
async fn inventory_labels_save(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    ids: Vec<String>,
    bytes: String,
) -> Reply<Option<String>> {
    response(
        async {
            use base64::Engine;
            let c = state.lock().await;
            if ids.is_empty() || ids.len() > 500 || bytes.len() > 30 * 1024 * 1024 {
                bail!("Choisissez de 1 à 500 équipements.")
            }
            for id in &ids {
                inventory::access(&c, &token, id, false).await?;
            }
            let data = base64::engine::general_purpose::STANDARD.decode(bytes)?;
            if !data.starts_with(b"%PDF-") {
                bail!("PDF invalide")
            };
            let file = app
                .dialog()
                .file()
                .set_title("Enregistrer les étiquettes QR")
                .set_file_name("Etiquettes-inventaire.pdf")
                .add_filter("PDF", &["pdf"])
                .blocking_save_file();
            if let Some(f) = file {
                let path = f.into_path()?;
                std::fs::write(&path, &data)?;
                let u = auth::require(&c, &token, None).await?;
                let mut tx = c.pool.begin().await?;
                audit::append(
                    &mut tx,
                    &c.key,
                    Some(&u),
                    "inventory.labels",
                    serde_json::json!({"equipment_ids":ids}),
                )
                .await?;
                tx.commit().await?;
                audit::checkpoint(&c.pool, &c.root).await?;
                Ok(Some(path.to_string_lossy().into_owned()))
            } else {
                Ok(None)
            }
        }
        .await,
    )
}

async fn initialise(root: PathBuf) -> Result<Core> {
    std::fs::create_dir_all(&root)?;
    let process_lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("process.lock"))?;
    fs2::FileExt::try_lock_exclusive(&process_lock).map_err(|_| {
        anyhow::anyhow!("Une autre instance ou une récupération utilise les données.")
    })?;
    let recovery = backup::apply_pending(&root)?;
    let key = db::prepare(&root)?;
    let pool = db::open(&root.join("db/registre.db"), &key, true).await?;
    db::before_migration(&root, &pool).await?;
    db::migrate(&pool).await?;
    audit::verify(&pool, &key, &root).await?;
    works::sync_legacy(&pool, &key).await?;
    sqlx::query("UPDATE sessions SET revoked_at=? WHERE revoked_at IS NULL")
        .bind(db::now())
        .execute(&pool)
        .await?;
    sqlx::query("UPDATE update_state SET installed_version=?,phase=CASE WHEN phase IN('installing','checking','downloading') THEN 'idle' ELSE phase END WHERE singleton=1").bind(env!("CARGO_PKG_VERSION")).execute(&pool).await?;
    let mut tx = pool.begin().await?;
    audit::append(
        &mut tx,
        &key,
        None,
        if recovery.is_some() {
            "system.restore.complete"
        } else {
            "system.start"
        },
        serde_json::json!({"version":env!("CARGO_PKG_VERSION")}),
    )
    .await?;
    tx.commit().await?;
    audit::checkpoint(&pool, &root).await?;
    let pending = root.join("recovery/update-pending.json");
    if pending.exists() && !std::env::args().any(|a| a == "--alerts-only" || a == "--backup-only") {
        let healthy = root
            .join("recovery")
            .join(format!("update-healthy-{}.json", db::id()));
        std::fs::rename(pending, healthy)?;
    }
    Ok(Core {
        root,
        pool,
        key,
        dummy_hash: crypto::password_hash("Remplissage interne non utilisable")?,
        recovery,
        pending_mfa: HashMap::new(),
        update_busy: Arc::new(AtomicBool::new(false)),
        _process_lock: Arc::new(process_lock),
        app: None,
    })
}
pub fn run() {
    let builder=tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app,args,_|{
            if args.iter().any(|a|a=="--alerts-only"){if let Some(shared)=app.try_state::<Shared>(){let shared=shared.inner().clone();tauri::async_runtime::spawn(async move{let c=shared.lock().await;let _=alerts::scheduled(&c).await;});}}else if !args.iter().any(|a|a=="--backup-only"){if let Some(w)=app.get_webview_window("main"){let _=w.show();let _=w.set_focus();}}
        }))
        // Le plugin SQL est conservé, mais aucune ouverture/premission SQL frontend.
        // L'adaptateur db.rs gère le pool chiffré SQLx et ses migrations côté Rust.
        .plugin(tauri_plugin_sql::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app|{
            let root=portable::root().unwrap_or(app.path().app_local_data_dir()?);
            let mut core=match tauri::async_runtime::block_on(initialise(root.clone())){Ok(c)=>c,Err(e)=>{
                app.dialog().message(format!("Le démarrage sécurisé a été interrompu.\n\n{e}\n\nVos données sont conservées dans :\n{}",root.display())).title("Libre ERP — récupération").blocking_show();return Err(e.into());
            }};
            core.app=Some(app.handle().clone());
            tauri::async_runtime::block_on(core.register_sql_pool());
            let backup_only=std::env::args().any(|a|a=="--backup-only");let alerts_only=std::env::args().any(|a|a=="--alerts-only");
            let shared:Shared=Arc::new(Mutex::new(core));app.manage(shared.clone());
            let handle=app.handle().clone();
            if alerts_only{
                tauri::async_runtime::spawn(async move{let c=shared.lock().await;let result=alerts::scheduled(&c).await;handle.exit(if result.is_ok(){0}else{1});});
            }else if backup_only{
                tauri::async_runtime::spawn(async move{let mut c=shared.lock().await;let result=backup::scheduled(&mut c).await;handle.exit(if result.is_ok(){0}else{1});});
            }else{
                if let Some(window)=app.get_webview_window("main"){if let Some(icon)=app.default_window_icon(){window.set_icon(icon.clone())?;}window.show()?;}
                let icon_app=app.handle().clone();let icon_root=root.clone();tauri::async_runtime::spawn_blocking(move||{let _=branding::refresh(&icon_app,&icon_root);});
                tauri::async_runtime::spawn(async move{
                    let mut tick=0u64;
                    loop{
                        {let mut c=shared.lock().await;
                            if !c.update_busy.load(Ordering::SeqCst){
                                if let Err(error)=alerts::evaluate(&c).await{let _=sqlx::query("INSERT INTO settings(key,value_json,modified_at) VALUES('alerts.last_error',?,?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,modified_at=excluded.modified_at").bind(serde_json::json!(error.to_string()).to_string()).bind(db::now()).execute(&c.pool).await;}
                                match backup::scheduled(&mut c).await{
                                    Ok(Some(_))=>{use tauri_plugin_notification::NotificationExt;let _=handle.notification().builder().title("Libre ERP").body("La sauvegarde planifiée est terminée.").show();},
                                    Err(e)=>{let _=sqlx::query("INSERT INTO settings(key,value_json,modified_at) VALUES('backup.last_error',?,?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,modified_at=excluded.modified_at").bind(serde_json::json!(e.to_string()).to_string()).bind(db::now()).execute(&c.pool).await;},_=>{}
                                }
                            }
                            if !portable::active() && tick%360==0 && option_env!("ERP_UPDATE_BASE_URL").is_some() && !c.update_busy.swap(true,Ordering::SeqCst){dispatch_check(c.clone(),handle.clone());}
                        }
                        tick+=1;tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![report_table,report_save,csv_import_options,csv_import_preview,csv_import_apply,csv_import_template,alerts_snapshot,alerts_poll,alert_acknowledge,alert_history,alert_rule_save,alert_manual_save,alert_manual_complete,alert_settings_save,alert_notification_test,alert_notification_retry,documents_snapshot,document_save,document_archive,document_history,document_file_save,documents_search,preferences_get,preferences_save,organisation_logo,contracts_snapshot,contract_save,contract_action,contract_history,contract_attach,contract_file_save,inventory_snapshot,inventory_detail,inventory_save,inventory_archive,inventory_location_add,inventory_category_add,inventory_maintenance,inventory_reading,inventory_reading_cancel,inventory_history,inventory_search,inventory_photo,inventory_photo_attach,inventory_photo_remove,inventory_labels_save,app_icon_refresh,map_open_licence,map_geocode,map_context,map_diagnostics,map_settings,map_settings_save,work_simple_transition,building_exchange_export,building_exchange_preview,building_exchange_import,works_snapshot,work_save,work_transition,work_history,works_search,work_cost,work_cost_reverse,work_file_active,work_attach,work_file_save,register_search,register_attachment_save,register_font,register_snapshot,register_save,register_history,register_archive,register_attachment,register_attach,register_export_pdf,user_scope,dashboard_snapshot,building_save,building_archive,building_photo,building_photo_set,app_status,bootstrap,login,logout,users,user_create,user_active,password_change,mfa_begin,mfa_confirm,audit_list,audit_verify,backup_settings,backup_schedule,backup_folder,backup_now,restore_prepare,restore_apply,update_status,update_channel,update_check,update_install]);
    if let Err(e) = builder.run(tauri::generate_context!()) {
        eprintln!("Démarrage interrompu : {e}");
    }
}

#[cfg(all(test, windows))]
mod register_tests;
#[tauri::command]
async fn register_font(state: State<'_, Shared>, token: String) -> Reply<String> {
    response(
        async {
            use base64::Engine;
            let c = state.lock().await;
            auth::require(&c, &token, None).await?;
            let font = std::path::PathBuf::from(
                std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()),
            )
            .join("Fonts/arial.ttf");
            let data = std::fs::read(font)?;
            Ok(base64::engine::general_purpose::STANDARD.encode(data))
        }
        .await,
    )
}
#[tauri::command]
async fn register_attachment_save(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    building_id: String,
    file_id: String,
) -> Reply<Option<String>> {
    response(
        async {
            use base64::Engine;
            let c = state.lock().await;
            let file = register::attachment(&c, &token, &building_id, &file_id).await?;
            let selected = app
                .dialog()
                .file()
                .set_title("Enregistrer une copie de la pièce jointe")
                .set_file_name(file["name"].as_str().unwrap_or("document.pdf"))
                .blocking_save_file();
            if let Some(path) = selected {
                let path = path.into_path()?;
                std::fs::write(
                    &path,
                    base64::engine::general_purpose::STANDARD
                        .decode(file["bytes"].as_str().unwrap_or(""))?,
                )?;
                Ok(Some(path.to_string_lossy().into_owned()))
            } else {
                Ok(None)
            }
        }
        .await,
    )
}
#[tauri::command]
async fn register_search(
    state: State<'_, Shared>,
    token: String,
    query: String,
) -> Reply<serde_json::Value> {
    response(register::search(&*state.lock().await, &token, &query).await)
}
#[tauri::command]
async fn works_snapshot(state: State<'_, Shared>, token: String) -> Reply<serde_json::Value> {
    response(works::snapshot(&*state.lock().await, &token).await)
}
#[tauri::command]
async fn work_save(state: State<'_, Shared>, token: String, input: works::Input) -> Reply<String> {
    response(works::save(&*state.lock().await, &token, input).await)
}
#[tauri::command]
async fn work_transition(
    state: State<'_, Shared>,
    token: String,
    id: String,
    version: i64,
    status: String,
    reason: String,
) -> Reply<()> {
    if status == "abandon" || status == "restore_abandoned" {
        return response(
            works::abandon(
                &*state.lock().await,
                &token,
                &id,
                version,
                &reason,
                status == "restore_abandoned",
            )
            .await,
        );
    }
    response(works::transition(&*state.lock().await, &token, &id, version, &status, &reason).await)
}
#[tauri::command]
async fn work_history(
    state: State<'_, Shared>,
    token: String,
    id: String,
) -> Reply<serde_json::Value> {
    response(works::history(&*state.lock().await, &token, &id).await)
}
#[tauri::command]
async fn works_search(
    state: State<'_, Shared>,
    token: String,
    query: String,
) -> Reply<serde_json::Value> {
    response(works::search(&*state.lock().await, &token, &query).await)
}
#[tauri::command]
async fn work_cost(
    state: State<'_, Shared>,
    token: String,
    id: String,
    version: i64,
    description: String,
    amount_cents: i64,
    incurred_on: String,
    file_id: Option<String>,
    reason: String,
) -> Reply<()> {
    response(
        works::cost(
            &*state.lock().await,
            &token,
            &id,
            version,
            &description,
            amount_cents,
            &incurred_on,
            file_id,
            &reason,
        )
        .await,
    )
}
#[tauri::command]
async fn work_cost_reverse(
    state: State<'_, Shared>,
    token: String,
    id: String,
    version: i64,
    cost_id: String,
    reason: String,
) -> Reply<()> {
    response(
        works::reverse_cost(
            &*state.lock().await,
            &token,
            &id,
            version,
            &cost_id,
            &reason,
        )
        .await,
    )
}
#[tauri::command]
async fn work_file_active(
    state: State<'_, Shared>,
    token: String,
    id: String,
    version: i64,
    file_id: String,
    active: bool,
    reason: String,
) -> Reply<()> {
    response(
        works::file_active(
            &*state.lock().await,
            &token,
            &id,
            version,
            &file_id,
            active,
            &reason,
        )
        .await,
    )
}
#[tauri::command]
async fn work_attach(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    id: String,
    version: i64,
    purpose: String,
) -> Reply<()> {
    response(
        async {
            let c = state.lock().await;
            works::access(&c, &token, &id, true).await?;
            works::writable(&c, &id, false).await?;
            works::cas(&c, &id, version).await?;
            let selected = app
                .dialog()
                .file()
                .set_title("Joindre un document au travail · 20 Mio maximum")
                .add_filter("PDF et photos", &["pdf", "png", "jpg", "jpeg"])
                .blocking_pick_file();
            if let Some(file) = selected {
                works::attach(&c, &token, &id, version, &purpose, &file.into_path()?).await?;
            }
            Ok(())
        }
        .await,
    )
}
#[tauri::command]
async fn work_file_save(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    id: String,
    file_id: String,
) -> Reply<Option<String>> {
    response(
        async {
            use base64::Engine;
            let c = state.lock().await;
            let file = works::file(&c, &token, &id, &file_id).await?;
            let selected = app
                .dialog()
                .file()
                .set_title("Enregistrer une copie du document")
                .set_file_name(file["name"].as_str().unwrap_or("document.pdf"))
                .blocking_save_file();
            if let Some(file_path) = selected {
                let path = file_path.into_path()?;
                std::fs::write(
                    &path,
                    base64::engine::general_purpose::STANDARD
                        .decode(file["bytes"].as_str().unwrap_or(""))?,
                )?;
                let (actor, _) = works::access(&c, &token, &id, false).await?;
                let mut tx = c.pool.begin().await?;
                audit::append(
                    &mut tx,
                    &c.key,
                    Some(&actor),
                    "work.file.export",
                    serde_json::json!({"work_id":id,"file_id":file_id}),
                )
                .await?;
                tx.commit().await?;
                audit::checkpoint(&c.pool, &c.root).await?;
                Ok(Some(path.to_string_lossy().into_owned()))
            } else {
                Ok(None)
            }
        }
        .await,
    )
}
#[cfg(all(test, windows))]
mod works_tests;

#[cfg(all(test, windows))]
mod inventory_tests;

#[tauri::command]
async fn preferences_get(state: State<'_, Shared>, token: String) -> Reply<serde_json::Value> {
    response(preferences::get(&*state.lock().await, &token).await)
}
#[tauri::command]
async fn preferences_save(
    state: State<'_, Shared>,
    token: String,
    features: preferences::Features,
) -> Reply<()> {
    response(preferences::save(&*state.lock().await, &token, features).await)
}
#[tauri::command]
async fn organisation_logo(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    remove: bool,
) -> Reply<()> {
    response(
        async {
            let c = state.lock().await;
            let u = auth::require(&c, &token, None).await?;
            if !preferences::global(&c, &u, "users.manage").await? {
                bail!("Gestion du logo non autorisée")
            };
            if remove {
                return preferences::logo(&c, &token, None).await;
            };
            if let Some(f) = app
                .dialog()
                .file()
                .set_title("Logo de la commune")
                .add_filter("PNG ou JPEG", &["png", "jpg", "jpeg"])
                .blocking_pick_file()
            {
                preferences::logo(&c, &token, Some(&f.into_path()?)).await?;
            }
            Ok(())
        }
        .await,
    )
}
#[tauri::command]
async fn contracts_snapshot(state: State<'_, Shared>, token: String) -> Reply<serde_json::Value> {
    response(contracts::snapshot(&*state.lock().await, &token).await)
}
#[tauri::command]
async fn contract_save(
    state: State<'_, Shared>,
    token: String,
    provider: bool,
    input: serde_json::Value,
) -> Reply<String> {
    response(contracts::save(&*state.lock().await, &token, provider, input).await)
}
#[tauri::command]
async fn contract_action(
    state: State<'_, Shared>,
    token: String,
    id: String,
    provider: bool,
    version: i64,
    op: String,
    input: serde_json::Value,
) -> Reply<()> {
    response(
        contracts::action(
            &*state.lock().await,
            &token,
            &id,
            provider,
            version,
            &op,
            input,
        )
        .await,
    )
}
#[tauri::command]
async fn contract_history(
    state: State<'_, Shared>,
    token: String,
    id: String,
    provider: bool,
) -> Reply<serde_json::Value> {
    response(contracts::history(&*state.lock().await, &token, &id, provider).await)
}
#[tauri::command]
async fn contract_attach(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    id: String,
    provider: bool,
    version: i64,
    target: Option<String>,
) -> Reply<()> {
    response(
        async {
            let c = state.lock().await;
            let u = auth::require(&c, &token, None).await?;
            if provider {
                if !contracts::provider_access(&c, &u, &id, true).await? {
                    bail!("Prestataire non modifiable")
                }
            } else {
                contracts::access(&c, &token, &id, true).await?;
            };
            if let Some(f) = app
                .dialog()
                .file()
                .add_filter("Documents", &["pdf", "png", "jpg", "jpeg"])
                .blocking_pick_file()
            {
                contracts::attach(
                    &c,
                    &token,
                    &id,
                    provider,
                    version,
                    target.as_deref(),
                    &f.into_path()?,
                )
                .await?;
            }
            Ok(())
        }
        .await,
    )
}
#[tauri::command]
async fn contract_file_save(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    id: String,
    provider: bool,
    file: String,
) -> Reply<Option<String>> {
    response(
        async {
            let c = state.lock().await;
            let (name, bytes) = contracts::file(&c, &token, &id, provider, &file).await?;
            if let Some(f) = app
                .dialog()
                .file()
                .set_file_name(&name)
                .blocking_save_file()
            {
                let p = f.into_path()?;
                if p.starts_with(&c.root) {
                    bail!("Choisissez un dossier extérieur aux données internes")
                };
                std::fs::write(&p, bytes)?;
                return Ok(Some(p.to_string_lossy().to_string()));
            };
            Ok(None)
        }
        .await,
    )
}
#[cfg(all(test, windows))]
mod contracts_tests;

#[tauri::command]
async fn documents_snapshot(state: State<'_, Shared>, token: String) -> Reply<serde_json::Value> {
    response(documents::snapshot(&*state.lock().await, &token).await)
}
#[tauri::command]
async fn document_save(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    input: serde_json::Value,
    replace_file: bool,
) -> Reply<Option<String>> {
    response(
        async {
            let c = state.lock().await;
            let u = auth::require(&c, &token, None).await?;
            if !works::permit(
                &c,
                &u,
                input["building_id"].as_str().unwrap_or(""),
                "documents.write",
            )
            .await?
            {
                bail!("Modification non autorisée")
            };
            let path = if replace_file || input["id"].as_str().is_none() {
                let Some(f) = app
                    .dialog()
                    .file()
                    .set_title("Choisir le fichier du document")
                    .add_filter(
                        "Documents",
                        &[
                            "pdf", "png", "jpg", "jpeg", "doc", "docx", "xls", "xlsx", "odt",
                            "ods", "dwg", "dxf", "txt", "csv", "zip",
                        ],
                    )
                    .blocking_pick_file()
                else {
                    return Ok(None);
                };
                Some(f.into_path()?)
            } else {
                None
            };
            Ok(Some(
                documents::save(&c, &token, input, path.as_deref()).await?,
            ))
        }
        .await,
    )
}
#[tauri::command]
async fn document_archive(
    state: State<'_, Shared>,
    token: String,
    id: String,
    revision: i64,
    archived: bool,
    reason: String,
) -> Reply<()> {
    response(
        documents::archive(
            &*state.lock().await,
            &token,
            &id,
            revision,
            archived,
            &reason,
        )
        .await,
    )
}
#[tauri::command]
async fn document_history(
    state: State<'_, Shared>,
    token: String,
    id: String,
) -> Reply<serde_json::Value> {
    response(documents::history(&*state.lock().await, &token, &id).await)
}
#[tauri::command]
async fn document_file_save(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    token: String,
    id: String,
    file: String,
    source: Option<String>,
) -> Reply<Option<String>> {
    response(
        async {
            let c = state.lock().await;
            let (name, bytes) = documents::file(&c, &token, &id, &file, source.as_deref()).await?;
            if let Some(f) = app
                .dialog()
                .file()
                .set_file_name(&name)
                .blocking_save_file()
            {
                let p = f.into_path()?;
                if p.starts_with(&c.root) {
                    bail!("Choisissez un dossier extérieur au stockage interne")
                };
                std::fs::write(&p, bytes)?;
                let u = auth::require(&c, &token, None).await?;
                let mut tx = c.pool.begin().await?;
                audit::append(
                    &mut tx,
                    &c.key,
                    Some(&u),
                    "document.export",
                    serde_json::json!({"document":id,"file":file,"source":source}),
                )
                .await?;
                tx.commit().await?;
                audit::checkpoint(&c.pool, &c.root).await?;
                return Ok(Some(p.to_string_lossy().to_string()));
            };
            Ok(None)
        }
        .await,
    )
}
#[tauri::command]
async fn documents_search(
    state: State<'_, Shared>,
    token: String,
    query: String,
) -> Reply<serde_json::Value> {
    response(
        async {
            if query.chars().count() < 2 || query.len() > 250 {
                return Ok(serde_json::json!([]));
            };
            let d = documents::snapshot(&*state.lock().await, &token).await?;
            let q = query.to_lowercase();
            Ok(serde_json::json!(d["records"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|r| format!(
                    "{} {}",
                    r["title"].as_str().unwrap_or(""),
                    r["name"].as_str().unwrap_or("")
                )
                .to_lowercase()
                .contains(&q))
                .take(30)
                .collect::<Vec<_>>()))
        }
        .await,
    )
}
#[cfg(all(test, windows))]
mod documents_tests;

#[tauri::command]
async fn alerts_snapshot(state: State<'_, Shared>, token: String) -> Reply<serde_json::Value> {
    response(alerts::snapshot(&*state.lock().await, &token).await)
}
#[tauri::command]
async fn alerts_poll(state: State<'_, Shared>, token: String) -> Reply<serde_json::Value> {
    response(
        async {
            let c = state.lock().await;
            auth::require(&c, &token, None).await?;
            alerts::evaluate(&c).await?;
            alerts::notify(&c, &token).await?;
            alerts::summary(&c, &token).await
        }
        .await,
    )
}
#[tauri::command]
async fn alert_acknowledge(
    state: State<'_, Shared>,
    token: String,
    id: String,
    reason: String,
) -> Reply<()> {
    response(alerts::acknowledge(&*state.lock().await, &token, &id, &reason).await)
}
#[tauri::command]
async fn alert_history(
    state: State<'_, Shared>,
    token: String,
    id: String,
) -> Reply<serde_json::Value> {
    response(alerts::history(&*state.lock().await, &token, &id).await)
}
#[tauri::command]
async fn alert_rule_save(
    state: State<'_, Shared>,
    token: String,
    input: serde_json::Value,
) -> Reply<String> {
    response(alerts::rule_save(&*state.lock().await, &token, input).await)
}
#[tauri::command]
async fn alert_manual_save(
    state: State<'_, Shared>,
    token: String,
    input: serde_json::Value,
) -> Reply<String> {
    response(alerts::manual_save(&*state.lock().await, &token, input).await)
}
#[tauri::command]
async fn alert_manual_complete(
    state: State<'_, Shared>,
    token: String,
    id: String,
    reason: String,
) -> Reply<()> {
    response(alerts::manual_complete(&*state.lock().await, &token, &id, &reason).await)
}
#[tauri::command]
async fn alert_settings_save(
    state: State<'_, Shared>,
    token: String,
    input: alerts::Settings,
) -> Reply<()> {
    response(alerts::save_settings(&*state.lock().await, &token, input).await)
}
#[tauri::command]
async fn alert_notification_test(state: State<'_, Shared>, token: String) -> Reply<()> {
    response(alerts::test_notification(&*state.lock().await, &token).await)
}
#[tauri::command]
async fn alert_notification_retry(state: State<'_, Shared>, token: String) -> Reply<()> {
    response(
        async {
            let c = state.lock().await;
            alerts::retry(&c, &token).await?;
            alerts::notify(&c, &token).await?;
            Ok(())
        }
        .await,
    )
}
#[cfg(all(test, windows))]
mod alerts_tests;

#[tauri::command]
async fn report_table(state: State<'_,Shared>,token:String,kind:String,building:Option<String>,archives:bool)->Reply<reports::Table>{response(reports::table(&*state.lock().await,&token,&kind,building.as_deref(),archives).await)}
fn export_destination(p:&std::path::Path,root:&std::path::Path,ext:&str)->anyhow::Result<()>{
 let resolved=if p.exists(){p.canonicalize()?}else{p.parent().ok_or_else(||anyhow::anyhow!("Dossier absent"))?.canonicalize()?.join(p.file_name().ok_or_else(||anyhow::anyhow!("Nom absent"))?)};
 if resolved.starts_with(root.canonicalize()?) {bail!("Choisissez un dossier extérieur aux données internes")}
 if resolved.extension().and_then(|s|s.to_str()).is_none_or(|s|!s.eq_ignore_ascii_case(ext)){bail!("Extension attendue : .{ext}")}
 Ok(())
}
#[tauri::command]
async fn report_save(app:tauri::AppHandle,state:State<'_,Shared>,token:String,kind:String,building:Option<String>,archives:bool,format:String,fingerprint:String,pdf:Option<String>)->Reply<Option<String>>{
 response(async {
 let c=state.lock().await;let data=reports::table(&c,&token,&kind,building.as_deref(),archives).await?;
 if data.fingerprint!=fingerprint{bail!("Les données ont changé. Actualisez l’aperçu avant l’export.")}
 let bytes=match format.as_str(){"csv"=>reports::csv(&data),"xlsx"=>reports::xlsx(&data)?,"pdf"=>{use base64::Engine;let s=pdf.as_deref().ok_or_else(||anyhow::anyhow!("PDF absent"))?;if s.len()>48*1024*1024{bail!("PDF trop volumineux : filtrez par bâtiment")};let b=base64::engine::general_purpose::STANDARD.decode(s)?;if !b.starts_with(b"%PDF-"){bail!("PDF invalide")};b},_=>bail!("Format inconnu")};
 let Some(file)=app.dialog().file().set_title("Enregistrer l’export").set_file_name(format!("LibreERP-{kind}-{}.{}",works::today(),format)).add_filter(format.to_uppercase(),&[format.as_str()]).blocking_save_file() else{return Ok(None)};
 let p=file.into_path()?;export_destination(&p,&c.root,&format)?;db::atomic_write(&p,&bytes)?;reports::log(&c,&token,&kind,&format,data.rows.len()).await?;Ok(Some(p.to_string_lossy().to_string()))
 }.await)
}
#[tauri::command]
async fn csv_import_options(state:State<'_,Shared>,token:String)->Reply<serde_json::Value>{response(async {let c=state.lock().await;let s=inventory::snapshot(&c,&token).await?;Ok(serde_json::json!({"buildings":s["buildings"],"categories":s["categories"],"locations":s["locations"]}))}.await)}
#[tauri::command]
async fn csv_import_preview(app:tauri::AppHandle,state:State<'_,Shared>,token:String,building:String)->Reply<Option<csv_import::Preview>>{response(async {
 let c=state.lock().await;let u=auth::require(&c,&token,None).await?;if !works::permit(&c,&u,&building,"inventory.write").await?{bail!("Import non autorisé")}
 let Some(file)=app.dialog().file().set_title("Choisir le CSV d’inventaire").add_filter("CSV UTF-8",&["csv"]).blocking_pick_file() else{return Ok(None)};let p=file.into_path()?;
 use std::io::Read;let mut content=String::new();std::fs::File::open(&p)?.take(2*1024*1024+1).read_to_string(&mut content).map_err(|_|anyhow::anyhow!("Fichier illisible : enregistrez le CSV en UTF-8"))?;if content.len()>2*1024*1024{bail!("CSV limité à 2 Mio")};
 Ok(Some(csv_import::preview(&c,&token,&building,&content).await?))}.await)}
#[tauri::command]
async fn csv_import_apply(state:State<'_,Shared>,token:String,building:String,content:String,seal:String)->Reply<serde_json::Value>{response(csv_import::apply(&*state.lock().await,&token,&building,&content,&seal).await)}
#[tauri::command]
async fn csv_import_template(app:tauri::AppHandle,state:State<'_,Shared>,token:String)->Reply<Option<String>>{response(async {let c=state.lock().await;auth::require(&c,&token,None).await?;let Some(file)=app.dialog().file().set_file_name("LibreERP-modele-inventaire.csv").add_filter("CSV",&["csv"]).blocking_save_file() else{return Ok(None)};let p=file.into_path()?;export_destination(&p,&c.root,"csv")?;db::atomic_write(&p,&csv_import::template())?;Ok(Some(p.to_string_lossy().to_string()))}.await)}

#[cfg(all(test,windows))]
mod transfers_tests;
