use argus_server::server_core::{AdminSettings, ServerCore};

#[tokio::test]
async fn server_core_persists_admin_settings_across_restart() {
    let database_path = std::env::temp_dir().join(format!(
        "argus-server-settings-{}-{}.sqlite",
        std::process::id(),
        chrono_like_timestamp()
    ));
    let database_path = database_path
        .to_str()
        .expect("temp database path should be utf-8")
        .to_string();

    let first = ServerCore::init(Some(&database_path))
        .await
        .expect("first server core should initialize");
    first
        .update_admin_settings(AdminSettings {
            instance_name: "Persistent Admin".to_string(),
        })
        .await
        .expect("settings should persist");

    drop(first);

    let second = ServerCore::init(Some(&database_path))
        .await
        .expect("second server core should initialize");
    assert_eq!(
        second.admin_settings().await.instance_name,
        "Persistent Admin"
    );

    let _ = std::fs::remove_file(database_path);
}

fn chrono_like_timestamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_millis()
}
