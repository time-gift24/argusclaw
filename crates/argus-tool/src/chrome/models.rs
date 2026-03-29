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
        let args: Self =
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
                Url::parse(url).map_err(|e| ChromeToolError::InvalidArguments {
                    reason: format!(
                        "field 'url' is invalid for action '{}': {e}",
                        args.action.as_str()
                    ),
                })?;
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
