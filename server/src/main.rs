use std::net::SocketAddr;

use device_hub_server::{
    app::{build_router_with_job_policy_and_board_profiles, load_default_board_profiles},
    config::AppConfig,
    services::job_manager::JobPolicyProvider,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = AppConfig::load("devices.yaml").unwrap_or_default();
    if config.device_registry_path.is_none() {
        config.device_registry_path = Some(".device-registry.yaml".into());
    }
    let board_profiles = load_default_board_profiles();
    let app = build_router_with_job_policy_and_board_profiles(
        config,
        JobPolicyProvider::default(),
        board_profiles,
    )
    .await;
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    println!("device-hub server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
