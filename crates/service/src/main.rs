#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    let addr = std::env::var("CODEXMANAGER_SERVICE_ADDR")
        .unwrap_or_else(|_| codexmanager_service::DEFAULT_ADDR.to_string());
    println!("codexmanager-service listening on {addr}");
    if let Err(err) = codexmanager_service::start_server(&addr) {
        eprintln!("service stopped: {err}");
        std::process::exit(1);
    }
}
