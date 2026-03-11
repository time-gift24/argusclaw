use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures_util::{StreamExt, SinkExt};
use serde_json::json;

mod error;
mod types;

pub use error::{CookieError, CookieResult};
pub use types::Cookie;

/// Get cookies for a specific domain from Chrome CDP
pub async fn get_cookies(
    cdp_ws_url: &str,
    domain: &str,
) -> CookieResult<Vec<Cookie>> {
    // Connect to CDP WebSocket
    let (ws_stream, _) = connect_async(cdp_ws_url).await?;
    let (mut write, mut read) = ws_stream.split();

    // Send Storage.getCookies command
    let request = json!({
        "id": 1,
        "method": "Storage.getCookies",
        "params": null
    });

    write.send(Message::Text(request.to_string().into())).await?;

    // Read response
    let cookies_json = if let Some(msg) = read.next().await {
        let msg = msg?;
        match msg {
            Message::Text(text) => text,
            _ => return Err(CookieError::InvalidResponse("Not text".into())),
        }
    } else {
        return Err(CookieError::InvalidResponse("No response".into()));
    };

    // Parse response
    let response: serde_json::Value = serde_json::from_str(&cookies_json)?;

    if let Some(error) = response.get("error") {
        return Err(CookieError::CdpFailed(error.to_string()));
    }

    let all_cookies = response["result"]["cookies"]
        .as_array()
        .ok_or_else(|| CookieError::InvalidResponse("Missing cookies array".into()))?;

    // Parse cookies
    let all_cookies: Vec<Cookie> = serde_json::from_value(json!(all_cookies))?;

    // Filter by domain (substring match, handles .example.com and example.com)
    let filtered: Vec<Cookie> = all_cookies
        .into_iter()
        .filter(|c| c.domain.contains(domain))
        .collect();

    Ok(filtered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cookie_domain_filter() {
        let cookies = vec![
            Cookie {
                name: "session".into(),
                value: "abc123".into(),
                domain: ".example.com".into(),
                path: "/".into(),
                expires: None,
                size: None,
                http_only: None,
                secure: None,
                session: None,
                same_site: None,
            },
            Cookie {
                name: "other".into(),
                value: "xyz".into(),
                domain: "other.com".into(),
                path: "/".into(),
                expires: None,
                size: None,
                http_only: None,
                secure: None,
                session: None,
                same_site: None,
            },
        ];

        let filtered: Vec<_> = cookies
            .into_iter()
            .filter(|c| c.domain.contains("example.com"))
            .collect();

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "session");
    }
}
