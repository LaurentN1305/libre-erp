//! Tâche Windows limitée au compte courant ; aucun secret dans les arguments.
use anyhow::{bail, Result};
use std::path::Path;

#[cfg(windows)]
pub fn configure(root: &Path, enabled: bool, time: &str) -> Result<()> {
    if crate::portable::applies(root) {
        return Ok(());
    }
    use std::os::windows::process::CommandExt;
    let windows = std::env::var_os("SystemRoot")
        .ok_or_else(|| anyhow::anyhow!("Dossier Windows introuvable"))?;
    let mut command = std::process::Command::new(
        std::path::PathBuf::from(windows).join("System32/WindowsPowerShell/v1.0/powershell.exe"),
    );
    command.creation_flags(0x08000000).args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
    ]);
    if enabled {
        let script = root.join("backup-task.ps1");
        crate::db::atomic_write(
            &script,
            include_bytes!("../../scripts/Planifier-Sauvegarde.ps1"),
        )?;
        command
            .arg("-File")
            .arg(script)
            .arg("-Application")
            .arg(std::env::current_exe()?)
            .arg("-Heure")
            .arg(time);
    } else {
        command.args(["-Command", "$taskName = 'SecuriteERP-Sauvegarde-' + [Security.Principal.WindowsIdentity]::GetCurrent().User.Value; Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue | Unregister-ScheduledTask -Confirm:$false"]);
    }
    let output = command.output()?;
    if !output.status.success() {
        bail!("La tâche Windows n’a pas pu être configurée. La sauvegarde reste disponible dans l’application.");
    }
    Ok(())
}
#[cfg(not(windows))]
pub fn configure(_root: &Path, _enabled: bool, _time: &str) -> Result<()> {
    bail!("La planification système nécessite Windows.")
}
#[cfg(windows)]
pub fn configure_alerts(root: &Path, enabled: bool, time: &str) -> Result<()> {
    if crate::portable::applies(root) {
        if enabled {
            bail!("La tâche d’alertes nécessite la version installée.")
        }
        return Ok(());
    }
    use std::os::windows::process::CommandExt;
    let script = root.join("alerts-task.ps1");
    crate::db::atomic_write(
        &script,
        include_bytes!("../../scripts/Planifier-Alertes.ps1"),
    )?;
    let shell = std::path::PathBuf::from(
        std::env::var_os("SystemRoot").ok_or_else(|| anyhow::anyhow!("Windows introuvable"))?,
    )
    .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let output = std::process::Command::new(shell)
        .creation_flags(0x08000000)
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
        .arg("-Heure")
        .arg(time)
        .arg("-Activer")
        .arg(if enabled { "1" } else { "0" })
        .output()?;
    if !output.status.success() {
        bail!("La tâche Windows d’alertes n’a pas pu être enregistrée. Les alertes restent disponibles dans l’application.")
    }
    Ok(())
}
#[cfg(not(windows))]
pub fn configure_alerts(_root: &Path, _enabled: bool, _time: &str) -> Result<()> {
    bail!("Planification Windows uniquement")
}
