fn main() {
    if let Err(e) = securite_erp_lib::portable_start() {
        eprintln!("Ouverture impossible : {e}");
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        std::process::exit(1)
    }
}
