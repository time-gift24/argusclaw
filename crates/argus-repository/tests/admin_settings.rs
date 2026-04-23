use argus_repository::traits::AdminSettingsRepository;
use argus_repository::types::AdminSettingsRecord;
use argus_repository::{ArgusSqlite, migrate};

#[tokio::test]
async fn admin_settings_default_and_update_round_trip() {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite should connect");
    migrate(&pool).await.expect("migrations should succeed");
    let sqlite = ArgusSqlite::new_with_key_material(pool, vec![7; 32]);

    let initial = AdminSettingsRepository::get_admin_settings(&sqlite)
        .await
        .expect("default settings should load");
    assert_eq!(initial.instance_name, "ArgusWing");

    let saved = AdminSettingsRepository::upsert_admin_settings(
        &sqlite,
        &AdminSettingsRecord {
            instance_name: "Workspace Admin".to_string(),
        },
    )
    .await
    .expect("settings should persist");
    assert_eq!(saved.instance_name, "Workspace Admin");

    let reloaded = AdminSettingsRepository::get_admin_settings(&sqlite)
        .await
        .expect("settings should reload");
    assert_eq!(reloaded.instance_name, "Workspace Admin");
}
