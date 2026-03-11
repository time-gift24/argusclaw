use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires: Option<f64>,
    pub size: Option<i64>,
    #[serde(rename = "httpOnly")]
    pub http_only: Option<bool>,
    pub secure: Option<bool>,
    pub session: Option<bool>,
    #[serde(rename = "sameSite")]
    pub same_site: Option<String>,
}