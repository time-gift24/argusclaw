//! LLM command - development only.

use std::io::{self, Write};
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Subcommand;
use argus_wing::ArgusWing;
use argus_protocol::LlmProviderId;
use futures_util::StreamExt;

use crate::{StreamRenderState, finish_stream_output, render_stream_event};

/// LLM 补全测试命令。
#[derive(Debug, Subcommand)]
pub enum LlmCommand {
    /// 向 LLM 提供商发送补全请求。
    Complete {
        /// 使用的提供商 ID（默认为默认提供商）。
        #[arg(long)]
        provider: Option<String>,
        /// 启用流式输出。
        #[arg(long, default_value_t = false)]
        stream: bool,
        /// 发送给 LLM 的提示词。
        prompt: String,
    },
}

/// Run LLM command.
pub async fn run_llm_command(wing: Arc<ArgusWing>, command: LlmCommand) -> Result<()> {
    match command {
        LlmCommand::Complete {
            provider,
            stream,
            prompt,
        } => {
            let provider_id = provider
                .map(|id| {
                    id.parse::<i64>()
                        .map(LlmProviderId::new)
                        .with_context(|| format!("Invalid provider id: {}", id))
                })
                .transpose()?;
            if stream {
                let mut events = wing.stream_text(provider_id.as_ref(), prompt).await?;
                let mut render_state = StreamRenderState::default();
                let mut stdout = io::stdout();

                while let Some(event) = events.next().await {
                    let event = event?;
                    if let Some(chunk) = render_stream_event(&mut render_state, &event) {
                        write!(stdout, "{chunk}").context("failed to write stream output")?;
                        stdout.flush().context("failed to flush stream output")?;
                    }
                }

                if let Some(suffix) = finish_stream_output(&render_state) {
                    write!(stdout, "{suffix}").context("failed to write stream output")?;
                    stdout.flush().context("failed to flush stream output")?;
                }
            } else {
                let content = wing.complete_text(provider_id.as_ref(), prompt).await?;
                println!("{content}");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::LlmCommand;
    use clap::Parser;

    #[test]
    fn parses_llm_complete_command_with_provider_selector_and_streaming() {
        let cli = super::super::DevCli::parse_from([
            "cli",
            "llm",
            "complete",
            "--provider",
            "openai",
            "--stream",
            "say hello",
        ]);

        match cli.command {
            super::super::DevCommand::Llm(LlmCommand::Complete {
                provider,
                stream,
                prompt,
            }) => {
                assert_eq!(provider.as_deref(), Some("openai"));
                assert!(stream);
                assert_eq!(prompt, "say hello");
            }
            _ => panic!("llm complete command should parse"),
        }
    }
}
