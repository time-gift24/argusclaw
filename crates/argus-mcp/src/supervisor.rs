use std::time::{Duration, Instant};

use argus_protocol::McpServerStatus;

use crate::runtime::{McpRuntime, McpRuntimeConfig};

pub(crate) fn retry_delay(config: &McpRuntimeConfig, retry_attempts: u32) -> Duration {
    if retry_attempts <= 1 {
        return config.initial_retry_delay.min(config.max_retry_delay);
    }

    let mut delay = config.initial_retry_delay;
    for _ in 1..retry_attempts {
        if delay >= config.max_retry_delay {
            return config.max_retry_delay;
        }
        delay = match delay.checked_mul(2) {
            Some(next) => next.min(config.max_retry_delay),
            None => config.max_retry_delay,
        };
    }

    delay.min(config.max_retry_delay)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn should_poll_server(
    enabled: bool,
    has_session: bool,
    status: McpServerStatus,
    urgent_retry: bool,
    next_retry_at: Option<Instant>,
    last_checked_instant: Option<Instant>,
    config: &McpRuntimeConfig,
    now: Instant,
) -> bool {
    if !enabled || status == McpServerStatus::Disabled {
        return false;
    }

    if let Some(deadline) = next_retry_at {
        return now >= deadline;
    }

    if urgent_retry {
        return true;
    }

    if has_session && status == McpServerStatus::Ready {
        return last_checked_instant
            .map(|checked| now.duration_since(checked) >= config.ready_recheck_interval)
            .unwrap_or(true);
    }

    last_checked_instant.is_none()
        || matches!(
            status,
            McpServerStatus::Connecting | McpServerStatus::Retrying | McpServerStatus::Failed
        )
}

pub(crate) fn spawn_supervisor(runtime: std::sync::Arc<McpRuntime>) {
    tokio::spawn(async move {
        if let Err(error) = runtime.poll_once().await {
            tracing::warn!(%error, "initial mcp runtime poll failed");
        }

        loop {
            let delay = runtime.next_supervisor_delay();
            runtime.wait_for_supervisor_wakeup(delay).await;
            if let Err(error) = runtime.poll_once().await {
                tracing::warn!(%error, "mcp supervisor poll failed");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> McpRuntimeConfig {
        McpRuntimeConfig {
            supervisor_poll_interval: Duration::from_secs(30),
            ready_recheck_interval: Duration::from_secs(300),
            initial_retry_delay: Duration::from_secs(5),
            max_retry_delay: Duration::from_secs(60),
        }
    }

    #[test]
    fn retry_delay_grows_exponentially_and_clamps() {
        let config = config();
        assert_eq!(retry_delay(&config, 1), Duration::from_secs(5));
        assert_eq!(retry_delay(&config, 2), Duration::from_secs(10));
        assert_eq!(retry_delay(&config, 3), Duration::from_secs(20));
        assert_eq!(retry_delay(&config, 5), Duration::from_secs(60));
    }

    #[test]
    fn ready_servers_wait_for_recheck_interval() {
        let now = Instant::now();
        let config = config();

        assert!(!should_poll_server(
            true,
            true,
            McpServerStatus::Ready,
            false,
            None,
            Some(now),
            &config,
            now + Duration::from_secs(10),
        ));
        assert!(should_poll_server(
            true,
            true,
            McpServerStatus::Ready,
            false,
            None,
            Some(now),
            &config,
            now + Duration::from_secs(301),
        ));
    }

    #[test]
    fn retrying_servers_wait_for_deadline_unless_marked_urgent() {
        let now = Instant::now();
        let config = config();

        assert!(!should_poll_server(
            true,
            false,
            McpServerStatus::Retrying,
            false,
            Some(now + Duration::from_secs(30)),
            Some(now),
            &config,
            now + Duration::from_secs(10),
        ));
        assert!(should_poll_server(
            true,
            false,
            McpServerStatus::Retrying,
            true,
            None,
            Some(now),
            &config,
            now + Duration::from_secs(10),
        ));
        assert!(!should_poll_server(
            true,
            false,
            McpServerStatus::Retrying,
            true,
            Some(now + Duration::from_secs(30)),
            Some(now),
            &config,
            now + Duration::from_secs(10),
        ));
        assert!(should_poll_server(
            true,
            false,
            McpServerStatus::Retrying,
            false,
            Some(now + Duration::from_secs(30)),
            Some(now),
            &config,
            now + Duration::from_secs(31),
        ));
    }
}
