use std::collections::HashSet;

use super::error::ChromeToolError;
use super::models::ChromeAction;

pub struct ExplorePolicy {
    allowed: HashSet<ChromeAction>,
}

impl ExplorePolicy {
    #[must_use]
    pub fn readonly() -> Self {
        let allowed = HashSet::from([
            ChromeAction::Install,
            ChromeAction::Open,
            ChromeAction::Navigate,
            ChromeAction::Wait,
            ChromeAction::ExtractText,
            ChromeAction::ListLinks,
            ChromeAction::NetworkRequests,
            ChromeAction::GetDomSummary,
            ChromeAction::NewTab,
            ChromeAction::SwitchTab,
            ChromeAction::CloseTab,
            ChromeAction::ListTabs,
        ]);
        Self { allowed }
    }

    #[must_use]
    pub fn interactive() -> Self {
        let allowed = HashSet::from([
            ChromeAction::Install,
            ChromeAction::Open,
            ChromeAction::Navigate,
            ChromeAction::Wait,
            ChromeAction::ExtractText,
            ChromeAction::ListLinks,
            ChromeAction::NetworkRequests,
            ChromeAction::GetDomSummary,
            ChromeAction::Click,
            ChromeAction::Type,
            ChromeAction::GetUrl,
            ChromeAction::GetCookies,
            ChromeAction::NewTab,
            ChromeAction::SwitchTab,
            ChromeAction::CloseTab,
            ChromeAction::ListTabs,
        ]);
        Self { allowed }
    }

    pub fn validate_action(&self, action: ChromeAction) -> Result<(), ChromeToolError> {
        if self.allowed.contains(&action) {
            Ok(())
        } else {
            Err(ChromeToolError::ActionNotAllowed {
                action: action.as_str().to_string(),
            })
        }
    }
}
