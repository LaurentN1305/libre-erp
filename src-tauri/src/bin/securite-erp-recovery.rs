fn main() {
    if let Err(error) = securite_erp_lib::recover_console() {
        eprintln!("\nRécupération interrompue : {error}\nLes données existantes sont conservées.");
        if !std::env::args().any(|a| a == "--verifier-installateur") {
            let mut wait = String::new();
            let _ = std::io::stdin().read_line(&mut wait);
        }
        std::process::exit(1);
    }
}
