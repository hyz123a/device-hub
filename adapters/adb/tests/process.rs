use device_hub_adapter_adb::AdbAdapter;
use device_hub_shared::AdbConfig;

#[tokio::test]
async fn logcat_decodes_invalid_utf8_lossy_instead_of_failing() {
    let adb = env!("CARGO_BIN_EXE_adb-process-fixture");
    let adapter = AdbAdapter::with_program(adb);
    let config = AdbConfig {
        serial: "device-1".into(),
        transport: None,
        default_logcat_args: vec!["-v".into(), "time".into()],
    };
    let mut session = adapter
        .logcat(&config, &config.default_logcat_args)
        .await
        .expect("logcat session");

    assert_eq!(
        session.next_line().await.expect("first line").as_deref(),
        Some("valid")
    );
    assert_eq!(
        session.next_line().await.expect("lossy line").as_deref(),
        Some("bad �")
    );
    assert_eq!(session.next_line().await.expect("eof"), None);
}
