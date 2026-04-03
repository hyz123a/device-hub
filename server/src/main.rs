use std::net::SocketAddr;

use device_hub_server::{app::build_router, config::AppConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = AppConfig::load("devices.yaml").unwrap_or_default();
    if config.device_registry_path.is_none() {
        config.device_registry_path = Some(".device-registry.yaml".into());
    }
    let app = build_router(config).await;
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    println!("device-hub server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
