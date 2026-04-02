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
    Navigate,
    Close,
    Restart,
    Wait,
    ExtractText,
    ListLinks,
    NetworkRequests,
    GetDomSummary,
    Click,
    Type,
    GetUrl,
    GetCookies,
    NewTab,
    SwitchTab,
    CloseTab,
    ListTabs,
}

impl ChromeAction {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Open => "open",
            Self::Navigate => "navigate",
            Self::Close => "close",
            Self::Restart => "restart",
            Self::Wait => "wait",
            Self::ExtractText => "extract_text",
            Self::ListLinks => "list_links",
            Self::NetworkRequests => "network_requests",
            Self::GetDomSummary => "get_dom_summary",
            Self::Click => "click",
            Self::Type => "type",
            Self::GetUrl => "get_url",
            Self::GetCookies => "get_cookies",
            Self::NewTab => "new_tab",
            Self::SwitchTab => "switch_tab",
            Self::CloseTab => "close_tab",
            Self::ListTabs => "list_tabs",
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
    #[serde(default)]
    pub tab_id: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
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
        args.tab_id = normalized_optional_string(args.tab_id);
        args.domain = normalized_optional_string(args.domain);

        match args.action {
            ChromeAction::Install => {
                validate_for_install(&args)?;
            }
            ChromeAction::Open => {
                validate_for_open(&mut args)?;
            }
            ChromeAction::Navigate => {
                validate_for_navigate(&mut args)?;
            }
            ChromeAction::Close => {
                require_session_id(&args)?;
                validate_for_close(&args)?;
            }
            ChromeAction::Restart => {
                require_session_id(&args)?;
                validate_for_restart(&mut args)?;
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
            ChromeAction::NewTab => {
                require_session_id(&args)?;
                validate_for_new_tab(&mut args)?;
            }
            ChromeAction::SwitchTab => {
                require_session_id(&args)?;
                validate_for_switch_tab(&args)?;
            }
            ChromeAction::CloseTab => {
                require_session_id(&args)?;
                validate_for_close_tab(&args)?;
            }
            ChromeAction::ListTabs => {
                require_session_id(&args)?;
                validate_for_list_tabs(&args)?;
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TabInfo {
    pub tab_id: String,
    pub url: String,
    pub title: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTabResult {
    pub tab_id: String,
    pub url: String,
    pub page_title: String,
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
    allow_only_fields(args, &["session_id", "timeout_ms"])
}

fn validate_for_close(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    allow_only_fields(args, &["session_id"])
}

fn validate_for_open(args: &mut ChromeToolArgs) -> Result<(), ChromeToolError> {
    let url = args
        .url
        .as_deref()
        .ok_or_else(|| ChromeToolError::MissingRequiredField {
            action: args.action.as_str().to_string(),
            field: "url",
        })?;
    allow_only_fields(args, &["url"])?;
    let url = validate_url_for_action(args.action.as_str(), url)?;
    args.url = Some(url);
    Ok(())
}

fn validate_for_restart(args: &mut ChromeToolArgs) -> Result<(), ChromeToolError> {
    require_session_id(args)?;
    let url = args
        .url
        .as_deref()
        .ok_or_else(|| ChromeToolError::MissingRequiredField {
            action: args.action.as_str().to_string(),
            field: "url",
        })?;
    allow_only_fields(args, &["session_id", "url"])?;
    let url = validate_url_for_action(args.action.as_str(), url)?;
    args.url = Some(url);
    Ok(())
}

fn validate_for_navigate(args: &mut ChromeToolArgs) -> Result<(), ChromeToolError> {
    require_session_id(args)?;
    let url = args
        .url
        .as_deref()
        .ok_or_else(|| ChromeToolError::MissingRequiredField {
            action: args.action.as_str().to_string(),
            field: "url",
        })?;
    allow_only_fields(args, &["session_id", "url"])?;
    let url = validate_url_for_action(args.action.as_str(), url)?;
    args.url = Some(url);
    Ok(())
}

fn validate_url_for_action(action: &str, url: &str) -> Result<String, ChromeToolError> {
    let parsed = Url::parse(url).map_err(|e| ChromeToolError::InvalidArguments {
        reason: format!("field 'url' is invalid for action '{action}': {e}"),
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ChromeToolError::InvalidArguments {
            reason: format!(
                "field 'url' is invalid for action '{action}': scheme '{}' is not allowed",
                parsed.scheme()
            ),
        });
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| ChromeToolError::InvalidArguments {
            reason: format!("field 'url' is invalid for action '{action}': host is missing"),
        })?;
    if host.eq_ignore_ascii_case("localhost") {
        return Err(ChromeToolError::InvalidArguments {
            reason: format!(
                "field 'url' is invalid for action '{action}': host '{host}' is not allowed"
            ),
        });
    }
    if let Ok(ip) = host.parse::<IpAddr>()
        && (ip.is_unspecified() || is_blocked_ip(ip))
    {
        return Err(ChromeToolError::InvalidArguments {
            reason: format!(
                "field 'url' is invalid for action '{action}': host '{host}' is not allowed"
            ),
        });
    }
    Ok(url.to_string())
}

fn validate_for_install(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    allow_only_fields(args, &[])
}

fn validate_for_extract_text(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    allow_only_fields(args, &["session_id", "selector"])
}

fn validate_for_list_links(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    allow_only_fields(args, &["session_id"])
}

fn validate_for_network_requests(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    if let Some(max_requests) = args.max_requests
        && max_requests == 0
    {
        return Err(ChromeToolError::InvalidArguments {
            reason: format!(
                "field 'max_requests' must be greater than 0 for action '{}'",
                args.action.as_str()
            ),
        });
    }

    allow_only_fields(args, &["session_id", "max_requests"])
}

fn validate_for_dom_summary(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    allow_only_fields(args, &["session_id"])
}

fn validate_for_click(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    require_field("selector", &args.selector, args.action.as_str())?;
    allow_only_fields(args, &["session_id", "selector"])
}

fn validate_for_type(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    require_field("selector", &args.selector, args.action.as_str())?;
    require_field("text", &args.text, args.action.as_str())?;
    allow_only_fields(args, &["session_id", "selector", "text"])
}

fn validate_for_get_url(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    allow_only_fields(args, &["session_id"])
}

fn validate_for_get_cookies(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    allow_only_fields(args, &["session_id", "domain"])
}

fn validate_for_new_tab(args: &mut ChromeToolArgs) -> Result<(), ChromeToolError> {
    let url = args
        .url
        .as_deref()
        .ok_or_else(|| ChromeToolError::MissingRequiredField {
            action: args.action.as_str().to_string(),
            field: "url",
        })?;
    allow_only_fields(args, &["session_id", "url"])?;
    let url = validate_url_for_action(args.action.as_str(), url)?;
    args.url = Some(url);
    Ok(())
}

fn validate_for_switch_tab(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    require_field("tab_id", &args.tab_id, args.action.as_str())?;
    allow_only_fields(args, &["session_id", "tab_id"])
}

fn validate_for_close_tab(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    require_field("tab_id", &args.tab_id, args.action.as_str())?;
    allow_only_fields(args, &["session_id", "tab_id"])
}

fn validate_for_list_tabs(args: &ChromeToolArgs) -> Result<(), ChromeToolError> {
    allow_only_fields(args, &["session_id"])
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

fn allow_only_fields(args: &ChromeToolArgs, allowed: &[&str]) -> Result<(), ChromeToolError> {
    let unexpected: Vec<&str> = present_fields(args)
        .into_iter()
        .filter(|field| !allowed.contains(field))
        .collect();

    if unexpected.is_empty() {
        Ok(())
    } else {
        Err(ChromeToolError::InvalidArguments {
            reason: format!(
                "{} for action '{}'",
                allowed_fields_message(allowed),
                args.action.as_str()
            ),
        })
    }
}

fn present_fields(args: &ChromeToolArgs) -> Vec<&'static str> {
    let mut fields = Vec::new();

    if args.url.is_some() {
        fields.push("url");
    }
    if args.session_id.is_some() {
        fields.push("session_id");
    }
    if args.selector.is_some() {
        fields.push("selector");
    }
    if args.timeout_ms.is_some() {
        fields.push("timeout_ms");
    }
    if args.max_requests.is_some() {
        fields.push("max_requests");
    }
    if args.text.is_some() {
        fields.push("text");
    }
    if args.tab_id.is_some() {
        fields.push("tab_id");
    }
    if args.domain.is_some() {
        fields.push("domain");
    }

    fields
}

fn allowed_fields_message(allowed: &[&str]) -> String {
    if allowed.is_empty() {
        "only 'action' is allowed".to_string()
    } else {
        format!("only {} are allowed", quoted_field_list(allowed))
    }
}

fn quoted_field_list(fields: &[&str]) -> String {
    match fields {
        [] => String::new(),
        [field] => format!("'{field}'"),
        [first, second] => format!("'{first}' and '{second}'"),
        _ => {
            let mut quoted: Vec<String> = fields.iter().map(|field| format!("'{field}'")).collect();
            let last = quoted.pop().unwrap_or_default();
            format!("{}, and {last}", quoted.join(", "))
        }
    }
}
