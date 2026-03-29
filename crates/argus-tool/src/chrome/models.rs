use std::net::IpAddr;

use argus_protocol::is_blocked_ip;
use serde::Deserialize;
use url::Url;

use super::error::ChromeToolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromeAction {
    Open,
    Wait,
    ExtractText,
    ListLinks,
    GetDomSummary,
    Screenshot,
    Click,
}

impl ChromeAction {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Wait => "wait",
            Self::ExtractText => "extract_text",
            Self::ListLinks => "list_links",
            Self::GetDomSummary => "get_dom_summary",
            Self::Screenshot => "screenshot",
            Self::Click => "click",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeToolArgs {
    pub action: ChromeAction,
    #[serde(default)]
    pub url: Option<String>,
}

impl ChromeToolArgs {
    pub fn validate(input: serde_json::Value) -> Result<Self, ChromeToolError> {
        let mut args: Self =
            serde_json::from_value(input).map_err(|e| ChromeToolError::InvalidArguments {
                reason: e.to_string(),
            })?;

        let url = args.url.as_deref().map(str::trim).filter(|value| !value.is_empty());
        match args.action {
            ChromeAction::Open => {
                let url = url.ok_or_else(|| ChromeToolError::MissingRequiredField {
                    action: args.action.as_str().to_string(),
                    field: "url",
                })?;
                let parsed = Url::parse(url).map_err(|e| ChromeToolError::InvalidArguments {
                    reason: format!(
                        "field 'url' is invalid for action '{}': {e}",
                        args.action.as_str()
                    ),
                })?;
                if !matches!(parsed.scheme(), "http" | "https") {
                    return Err(ChromeToolError::InvalidArguments {
                        reason: format!(
                            "field 'url' is invalid for action '{}': scheme '{}' is not allowed",
                            args.action.as_str(),
                            parsed.scheme()
                        ),
                    });
                }
                let host = parsed
                    .host_str()
                    .ok_or_else(|| ChromeToolError::InvalidArguments {
                        reason: format!(
                            "field 'url' is invalid for action '{}': host is missing",
                            args.action.as_str()
                        ),
                    })?;
                if host.eq_ignore_ascii_case("localhost") {
                    return Err(ChromeToolError::InvalidArguments {
                        reason: format!(
                            "field 'url' is invalid for action '{}': host '{}' is not allowed",
                            args.action.as_str(),
                            host
                        ),
                    });
                }
                if let Ok(ip) = host.parse::<IpAddr>() {
                    if ip.is_unspecified() || is_blocked_ip(ip) {
                        return Err(ChromeToolError::InvalidArguments {
                            reason: format!(
                                "field 'url' is invalid for action '{}': host '{}' is not allowed",
                                args.action.as_str(),
                                host
                            ),
                        });
                    }
                }
                args.url = Some(url.to_string());
            }
            _ => {
                if args.url.is_some() {
                    return Err(ChromeToolError::InvalidArguments {
                        reason: format!(
                            "field 'url' is not allowed for action '{}'",
                            args.action.as_str()
                        ),
                    });
                }
            }
        }

        Ok(args)
    }
}
