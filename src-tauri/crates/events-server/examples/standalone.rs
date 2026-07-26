// Standalone runner — useful for smoke tests outside the Tauri app.
// `cargo run --example standalone -p xangi-events-server`
// Honours XANGI_PET_PORT / XANGI_PET_BIND env vars (same as the embedded server).

use std::net::SocketAddr;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let port: u16 = std::env::var("XANGI_PET_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7895);
    let bind: std::net::IpAddr = std::env::var("XANGI_PET_BIND")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| "127.0.0.1".parse().unwrap());
    let addr = SocketAddr::new(bind, port);

    let state = xangi_events_server::make_state();
    let listener = xangi_events_server::bind_with_autoshift(addr, 10).await?;
    let bound = listener.local_addr().unwrap_or(addr);
    println!("xangi-events listening on http://{bound}");
    if bound.port() != addr.port() {
        println!(
            "  (requested :{} was in use, auto-shifted to :{})",
            addr.port(),
            bound.port()
        );
    }
    println!("  pet dirs (search order):");
    for d in state.pet_dirs.iter() {
        println!("    - {}", d.display());
    }
    xangi_events_server::serve_listener(listener, state).await
}
