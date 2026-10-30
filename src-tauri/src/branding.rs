//! Ne modifie que l’icône des raccourcis pointant vers cet exécutable.
use anyhow::{Context, Result};
use tauri::Manager;
pub fn refresh(app: &tauri::AppHandle, root: &std::path::Path) -> Result<String> {
    if let Some(window) = app.get_webview_window("main") {
        if let Some(icon) = app.default_window_icon() {
            window.set_icon(icon.clone())?;
        }
    }
    let dir = root.join("branding");
    std::fs::create_dir_all(&dir)?;
    let icon = dir.join(format!("erp-{}.ico", env!("CARGO_PKG_VERSION")));
    crate::db::atomic_write(&icon, include_bytes!("../icons/icon.ico"))?;
    let script = dir.join("Actualiser-Icones.ps1");
    crate::db::atomic_write(
        &script,
        include_bytes!("../../scripts/Actualiser-Icones.ps1"),
    )?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let shell = std::path::PathBuf::from(
            std::env::var_os("SystemRoot").context("Windows introuvable")?,
        )
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let output = std::process::Command::new(shell)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(script)
            .arg("-Application")
            .arg(std::env::current_exe()?)
            .arg("-Icon")
            .arg(icon)
            .creation_flags(0x08000000)
            .output()?;
        if !output.status.success() {
            anyhow::bail!("Windows n’a pas pu actualiser les raccourcis. L’application et les données restent disponibles.")
        }
        return Ok("Icône de la fenêtre et raccourcis de cette application actualisés. Si une ancienne épingle persiste, détachez-la puis épinglez la fenêtre actuelle.".into());
    }
    #[cfg(not(windows))]
    Ok("Fonction réservée à Windows".into())
}
