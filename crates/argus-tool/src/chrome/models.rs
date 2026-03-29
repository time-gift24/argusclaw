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
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub screenshot_path: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

impl ChromeToolArgs {
    pub fn validate(input: serde_json::Value) -> Result<Self, ChromeToolError> {
        let mut args: Self =
            serde_json::from_value(input).map_err(|e| ChromeToolError::InvalidArguments {
                reason: e.to_string(),
            })?;

        args.url = normalized_optional_string(args.url);
        args.session_id = normalized_optional_string(args.session_id);
        args.selector = normalized_optional_string(args.selector);
        args.screenshot_path = normalized_optional_string(args.screenshot_path);

        match args.action {
            ChromeAction::Open => {
                let url =
                    args.url
                        .as_deref()
                        .ok_or_else(|| ChromeToolError::MissingRequiredField {
                            action: args.action.as_str().to_string(),
                            field: "url",
                        })?;
                if args.session_id.is_some()
                    || args.selector.is_some()
                    || args.screenshot_path.is_some()
                    || args.timeout_ms.is_some()
                {
                    return Err(ChromeToolError::InvalidArguments {
                        reason: format!(
                            "only 'url' is allowed for action '{}'",
                            args.action.as_str()
                        ),
                    });
                }
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
            ChromeAction::Wait => {
                require_session_id(&args)?;
                validate_for_wait(&args)?;
            }
            ChromeAction::ExtractText => {
                require_session_id(&args)?;
                validate_for_extract_text(&args)?;
            }
            ChromeAction::ListLinks => {
                require_session_id(&args)?;
                validate_for_list_links(&args)?;
            }
            ChromeAction::GetDomSummary => {
                require_session_id(&args)?;
                validate_for_dom_summary(&args)?;
            }
            ChromeAction::Screenshot => {
                require_session_id(&args)?;
                validate_for_screenshot(&args)?;
            }
            ChromeAction::Click => {
                validate_for_click(&args)?;
            }
        }

        Ok(args)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenArgs {
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageMetadata {
    pub final_url: String,
    pub page_title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedSession {
    pub session_id: String,
    pub final_url: String,
    pub page_title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct LinkSummary {
    pub href: String,
    pub text: String,
}

fn normalized_optional_string(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn require_session_id(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    args.session_id
        .as_deref()
        .ok_or_else(|| ChromeToolError::MissingRequiredField {
            action: args.action.as_str().to_string(),
            field: "session_id",
        })
        .map(|_| ())
}

fn validate_for_wait(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    if args.url.is_some() || args.screenshot_path.is_some() {
        return Err(ChromeToolError::InvalidArguments {
            reason: format!(
                "fields 'url' and 'screenshot_path' are not allowed for action '{}'",
                args.action.as_str()
            ),
        });
    }
    Ok(())
}

fn validate_for_extract_text(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    if args.url.is_some() || args.screenshot_path.is_some() || args.timeout_ms.is_some() {
        return Err(ChromeToolError::InvalidArguments {
            reason: format!(
                "fields 'url', 'screenshot_path', and 'timeout_ms' are not allowed for action '{}'",
                args.action.as_str()
            ),
        });
    }
    Ok(())
}

fn validate_for_list_links(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    if args.url.is_some()
        || args.selector.is_some()
        || args.screenshot_path.is_some()
        || args.timeout_ms.is_some()
    {
        return Err(ChromeToolError::InvalidArguments {
            reason: format!(
                "only 'session_id' is allowed for action '{}'",
                args.action.as_str()
            ),
        });
    }
    Ok(())
}

fn validate_for_dom_summary(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    if args.url.is_some()
        || args.selector.is_some()
        || args.screenshot_path.is_some()
        || args.timeout_ms.is_some()
    {
        return Err(ChromeToolError::InvalidArguments {
            reason: format!(
                "only 'session_id' is allowed for action '{}'",
                args.action.as_str()
            ),
        });
    }
    Ok(())
}

fn validate_for_screenshot(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    if args.url.is_some() || args.selector.is_some() || args.timeout_ms.is_some() {
        return Err(ChromeToolError::InvalidArguments {
            reason: format!(
                "fields 'url', 'selector', and 'timeout_ms' are not allowed for action '{}'",
                args.action.as_str()
            ),
        });
    }
    if args.screenshot_path.is_none() {
        return Err(ChromeToolError::MissingRequiredField {
            action: args.action.as_str().to_string(),
            field: "screenshot_path",
        });
    }
    Ok(())
}

fn validate_for_click(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    if args.url.is_some()
        || args.session_id.is_some()
        || args.selector.is_some()
        || args.screenshot_path.is_some()
        || args.timeout_ms.is_some()
    {
        return Err(ChromeToolError::InvalidArguments {
            reason: format!(
                "no extra fields are allowed for action '{}'",
                args.action.as_str()
            ),
        });
    }
    Ok(())
}
