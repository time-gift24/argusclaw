use std::sync::Arc;

use claw::AppContext;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let claw_context = rt.block_on(AppContext::init(None)).expect("初始化失败");

    tauri::Builder::default()
        .manage(Arc::new(claw_context))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
