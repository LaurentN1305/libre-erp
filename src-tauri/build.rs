fn main() {
    // Le cache OpenSSL statique ne distribue pas ses PDB : cela ne concerne que
    // le débogage des sources C externes, pas les symboles ni le chiffrement.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg=/IGNORE:4099");
        // Les tests unitaires Rust sont des exécutables distincts. Sans ce
        // manifeste, TaskDialogIndirect est résolu dans Common Controls v5.
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
    }
    println!("cargo:rerun-if-env-changed=ERP_MANIFEST_PUBLIC_KEY");
    println!("cargo:rerun-if-env-changed=ERP_UPDATE_BASE_URL");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let attributes = tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        tauri_build::try_build(attributes).expect("Configuration Windows invalide");
    } else {
        tauri_build::build()
    }
}
