use std::net::IpAddr;

use argus_protocol::is_blocked_ip;
use serde::Deserialize;
use serde_json::Value;
use url::Url;

use super::error::ChromeToolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromeAction {
    Install,
    Open,
    Wait,
    ExtractText,
    ListLinks,
    NetworkRequests,
    GetDomSummary,
    Click,
    Type,
    GetUrl,
    GetCookies,
}

impl ChromeAction {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Open => "open",
            Self::Wait => "wait",
            Self::ExtractText => "extract_text",
            Self::ListLinks => "list_links",
            Self::NetworkRequests => "network_requests",
            Self::GetDomSummary => "get_dom_summary",
            Self::Click => "click",
            Self::Type => "type",
            Self::GetUrl => "get_url",
            Self::GetCookies => "get_cookies",
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
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub max_requests: Option<u32>,
    #[serde(default)]
    pub text: Option<String>,
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
        args.text = normalized_optional_string(args.text);

        match args.action {
            ChromeAction::Install => {
                validate_for_install(&args)?;
            }
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
                    || args.timeout_ms.is_some()
                    || args.max_requests.is_some()
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
                if let Ok(ip) = host.parse::<IpAddr>()
                    && (ip.is_unspecified() || is_blocked_ip(ip))
                {
                    return Err(ChromeToolError::InvalidArguments {
                        reason: format!(
                            "field 'url' is invalid for action '{}': host '{}' is not allowed",
                            args.action.as_str(),
                            host
                        ),
                    });
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
            ChromeAction::NetworkRequests => {
                require_session_id(&args)?;
                validate_for_network_requests(&args)?;
            }
            ChromeAction::GetDomSummary => {
                require_session_id(&args)?;
                validate_for_dom_summary(&args)?;
            }
            ChromeAction::Click => {
                require_session_id(&args)?;
                validate_for_click(&args)?;
            }
            ChromeAction::Type => {
                require_session_id(&args)?;
                validate_for_type(&args)?;
            }
            ChromeAction::GetUrl => {
                require_session_id(&args)?;
                validate_for_get_url(&args)?;
            }
            ChromeAction::GetCookies => {
                require_session_id(&args)?;
                validate_for_get_cookies(&args)?;
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CookieSummary {
    pub name: String,
    pub value: String,
    pub domain: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NetworkRequestSummary {
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub request_headers: Value,
    pub response_headers: Value,
    pub error: Option<String>,
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
    if args.url.is_some() || args.selector.is_some() || args.max_requests.is_some() {
        return Err(ChromeToolError::InvalidArguments {
            reason: format!(
                "fields 'url', 'selector', and 'max_requests' are not allowed for action '{}'",
                args.action.as_str()
            ),
        });
    }
    Ok(())
}

fn validate_for_install(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    if args.url.is_some()
        || args.session_id.is_some()
        || args.selector.is_some()
        || args.timeout_ms.is_some()
        || args.max_requests.is_some()
        || args.text.is_some()
    {
        return Err(ChromeToolError::InvalidArguments {
            reason: format!(
                "only 'action' is allowed for action '{}'",
                args.action.as_str()
            ),
        });
    }
    Ok(())
}

fn validate_for_extract_text(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    reject_fields(
        args,
        &["url", "text", "timeout_ms", "max_requests"],
        "fields 'url', 'text', 'timeout_ms', and 'max_requests' are not allowed",
    )
}

fn validate_for_list_links(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    reject_fields(
        args,
        &["url", "selector", "text", "timeout_ms", "max_requests"],
        "only 'session_id' is allowed",
    )
}

fn validate_for_network_requests(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    if let Some(max_requests) = args.max_requests && max_requests == 0 {
        return Err(ChromeToolError::InvalidArguments {
            reason: format!(
                "field 'max_requests' must be greater than 0 for action '{}'",
                args.action.as_str()
            ),
        });
    }

    reject_fields(
        args,
        &["url", "selector", "text", "timeout_ms"],
        "only 'session_id' and 'max_requests' are allowed",
    )
}

fn validate_for_dom_summary(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    reject_fields(
        args,
        &["url", "selector", "text", "timeout_ms", "max_requests"],
        "only 'session_id' is allowed",
    )
}

fn validate_for_click(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    require_field("selector", &args.selector, args.action.as_str())?;
    reject_fields(
        args,
        &["url", "text", "timeout_ms", "max_requests"],
        "only 'session_id' and 'selector' are allowed",
    )
}

fn validate_for_type(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    require_field("selector", &args.selector, args.action.as_str())?;
    require_field("text", &args.text, args.action.as_str())?;
    reject_fields(
        args,
        &["url", "timeout_ms", "max_requests"],
        "only 'session_id', 'selector', and 'text' are allowed",
    )
}

fn validate_for_get_url(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    reject_fields(
        args,
        &["url", "selector", "text", "timeout_ms", "max_requests"],
        "only 'session_id' is allowed",
    )
}

fn validate_for_get_cookies(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    reject_fields(
        args,
        &["url", "selector", "text", "timeout_ms", "max_requests"],
        "only 'session_id' is allowed",
    )
}

fn require_field(
    field: &'static str,
    value: &Option<String>,
    action: &str,
) -> Result<(), ChromeToolError> {
    if value.is_none() {
        return Err(ChromeToolError::MissingRequiredField {
            action: action.to_string(),
            field,
        });
    }
    Ok(())
}

fn reject_fields(
    args: &ChromeToolArgs,
    fields: &[&str],
    message: &str,
) -> Result<(), ChromeToolError> {
    let present: Vec<&str> = fields
        .iter()
        .filter(|&&f| match f {
            "url" => args.url.is_some(),
            "selector" => args.selector.is_some(),
            "text" => args.text.is_some(),
            "timeout_ms" => args.timeout_ms.is_some(),
            "max_requests" => args.max_requests.is_some(),
            _ => false,
        })
        .copied()
        .collect();
    if present.is_empty() {
        Ok(())
    } else {
        Err(ChromeToolError::InvalidArguments {
            reason: format!("{message} for action '{}'", args.action.as_str()),
        })
    }
}
