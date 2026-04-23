#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminSettingsRecord {
    pub instance_name: String,
}

impl Default for AdminSettingsRecord {
    fn default() -> Self {
        Self {
            instance_name: "ArgusWing".to_string(),
        }
    }
}
