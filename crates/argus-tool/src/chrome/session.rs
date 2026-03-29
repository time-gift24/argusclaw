use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromeSession {
    pub session_id: String,
    pub current_url: String,
    pub page_title: String,
    pub last_screenshot_path: Option<PathBuf>,
}

impl ChromeSession {
    #[must_use]
    pub fn new(session_id: String, current_url: String, page_title: String) -> Self {
        Self {
            session_id,
            current_url,
            page_title,
            last_screenshot_path: None,
        }
    }
}
