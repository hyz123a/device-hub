use device_hub_server::config::AppConfig;
use device_hub_shared::{BoardProfile, JlinkBoardProfile};

use crate::helpers::spawn_app_with_config_and_board_profiles;

#[tokio::test]
async fn list_board_profiles_returns_yaml_backed_profiles() {
    let app = spawn_app_with_config_and_board_profiles(
        AppConfig::default(),
        vec![BoardProfile {
            id: "brd4186c".into(),
            label: "BRD4186C".into(),
            jlink: JlinkBoardProfile {
                device: "EFR32MG24BXXXF1536".into(),
                interface: "SWD".into(),
                speed_khz: 2700,
            },
        }],
    )
    .await;

    let response = app.get_board_profiles().await;

    assert!(response.status().is_success());
    let profiles: Vec<BoardProfile> = response.json().await.expect("invalid board profiles json");
    assert_eq!(
        profiles,
        vec![BoardProfile {
            id: "brd4186c".into(),
            label: "BRD4186C".into(),
            jlink: JlinkBoardProfile {
                device: "EFR32MG24BXXXF1536".into(),
                interface: "SWD".into(),
                speed_khz: 2700,
            },
        }]
    );
}
