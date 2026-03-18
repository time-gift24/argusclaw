//! Provider command - shared between arguswing and arguswing-dev.
//!
//! This module contains the provider management commands that are common
//! to both the production and development CLI binaries.

use anyhow::{Context, Result, anyhow};
use argus_protocol::{
    LlmProviderId, LlmProviderKind, LlmProviderRecord, ProviderSecretStatus, SecretString,
};
use argus_wing::ArgusWing;
use clap::{Args, Subcommand};
use std::collections::HashMap;
use std::sync::Arc;

/// LLM 提供商管理命令。
#[derive(Debug, Subcommand)]
pub enum ProviderCommand {
    /// 列出所有已配置的提供商。
    List,
    /// 获取指定提供商的详情。
    Get {
        /// 要查询的提供商 ID。
        #[arg(long)]
        id: String,
    },
    /// 创建或更新提供商配置。
    Upsert(ProviderUpsertArgs),
    /// 设置默认提供商。
    SetDefault {
        /// 要设为默认的提供商 ID。
        #[arg(long)]
        id: String,
    },
    /// 获取当前默认提供商。
    GetDefault,
    /// 为提供商设置额外的请求头。
    SetHeader {
        /// 提供商 ID。
        #[arg(long)]
        id: String,
        /// 请求头名称。
        #[arg(long)]
        name: String,
        /// 请求头值。
        #[arg(long)]
        value: String,
    },
    /// 移除提供商的额外请求头。
    RemoveHeader {
        /// 提供商 ID。
        #[arg(long)]
        id: String,
        /// 要移除的请求头名称。
        #[arg(long)]
        name: String,
    },
}

#[derive(Debug, Args)]
pub struct ProviderUpsertArgs {
    #[arg(long)]
    pub id: String,
    #[arg(long = "display-name")]
    pub display_name: String,
    #[arg(long)]
    pub kind: String,
    #[arg(long = "base-url")]
    pub base_url: String,
    #[arg(long = "api-key")]
    pub api_key: String,
    #[arg(long)]
    pub model: String,
    #[arg(long = "default", default_value_t = false)]
    pub is_default: bool,
}

/// Display record for provider output (hides sensitive data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDisplayRecord {
    pub id: String,
    pub display_name: String,
    pub kind: String,
    pub base_url: String,
    pub models: Vec<String>,
    pub default_model: String,
    pub is_default: bool,
    pub extra_headers: HashMap<String, String>,
}

impl From<LlmProviderRecord> for ProviderDisplayRecord {
    fn from(value: LlmProviderRecord) -> Self {
        Self {
            id: value.id.to_string(),
            display_name: value.display_name,
            kind: value.kind.to_string(),
            base_url: value.base_url,
            models: value.models,
            default_model: value.default_model,
            is_default: value.is_default,
            extra_headers: value.extra_headers,
        }
    }
}

impl TryFrom<ProviderUpsertArgs> for LlmProviderRecord {
    type Error = argus_protocol::LlmProviderKindParseError;

    fn try_from(value: ProviderUpsertArgs) -> Result<Self, Self::Error> {
        // Note: With INTEGER auto-increment IDs, the ID field should be removed
        // and the database should generate it. For now, we use a placeholder.
        // TODO: Split into insert (no ID) and update (with ID) operations.
        Ok(Self {
            id: LlmProviderId::new(0), // Placeholder - will be set by database or update
            kind: value.kind.parse::<LlmProviderKind>()?,
            display_name: value.display_name,
            base_url: value.base_url,
            api_key: SecretString::new(value.api_key),
            models: vec![value.model.clone()],
            default_model: value.model,
            is_default: value.is_default,
            extra_headers: HashMap::new(),
            secret_status: ProviderSecretStatus::Ready,
        })
    }
}

pub fn render_provider_output(record: &ProviderDisplayRecord) -> String {
    let headers_str = if record.extra_headers.is_empty() {
        String::new()
    } else {
        let headers: String = record
            .extra_headers
            .iter()
            .map(|(k, v)| format!("  {k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("\nextra_headers:\n{headers}")
    };

    let models_str = record.models.join(", ");

    format!(
        "id: {}\ndisplay_name: {}\nkind: {}\nbase_url: {}\nmodels: {}\ndefault_model: {}\nis_default: {}{}",
        record.id,
        record.display_name,
        record.kind,
        record.base_url,
        models_str,
        record.default_model,
        record.is_default,
        headers_str
    )
}

/// Validates that a header name is valid for HTTP headers.
/// Header names must be ASCII and cannot contain spaces, control characters, or delimiters.
fn validate_header_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("header name cannot be empty"));
    }

    for ch in name.chars() {
        if !ch.is_ascii() {
            return Err(anyhow!(
                "header name must be ASCII, found non-ASCII character"
            ));
        }
        // HTTP header names cannot contain these characters
        if matches!(ch, '\r' | '\n' | ':' | ' ' | '\t' | '\0'..='\x1f') {
            return Err(anyhow!("header name contains invalid character: {:?}", ch));
        }
    }

    Ok(())
}

/// Run provider command.
pub async fn run_provider_command(wing: Arc<ArgusWing>, command: ProviderCommand) -> Result<()> {
    match command {
        ProviderCommand::List => {
            for provider in wing.list_providers().await? {
                println!("{}", render_provider_output(&provider.into()));
                println!();
            }
        }
        ProviderCommand::Get { id } => {
            let id: i64 = id.parse().context("provider id must be an integer")?;
            let provider = wing.get_provider_record(LlmProviderId::new(id)).await?;
            println!("{}", render_provider_output(&provider.into()));
        }
        ProviderCommand::Upsert(args) => {
            // TODO: With INTEGER auto-increment IDs, upsert should be split into
            // insert (no ID) and update (requires ID). For now, parse ID as i64.
            let id: i64 = args.id.parse().context("provider id must be an integer")?;
            let mut record = LlmProviderRecord::try_from(args)?;
            record.id = LlmProviderId::new(id);
            wing.upsert_provider(record).await?;
        }
        ProviderCommand::SetDefault { id } => {
            let id: i64 = id.parse().context("provider id must be an integer")?;
            wing.set_default_provider(LlmProviderId::new(id)).await?;
        }
        ProviderCommand::GetDefault => {
            let provider = wing.get_default_provider_record().await?;
            println!("{}", render_provider_output(&provider.into()));
        }
        ProviderCommand::SetHeader { id, name, value } => {
            validate_header_name(&name)?;
            let provider_id: i64 = id.parse().context("provider id must be an integer")?;
            let provider_id = LlmProviderId::new(provider_id);
            let mut record = wing.get_provider_record(provider_id).await?;
            record.extra_headers.insert(name.clone(), value);
            wing.upsert_provider(record).await?;
            println!("Set header `{name}` on provider `{provider_id}`");
        }
        ProviderCommand::RemoveHeader { id, name } => {
            let provider_id: i64 = id.parse().context("provider id must be an integer")?;
            let provider_id = LlmProviderId::new(provider_id);
            let mut record = wing.get_provider_record(provider_id).await?;
            if record.extra_headers.remove(&name).is_some() {
                wing.upsert_provider(record).await?;
                println!("Removed header `{name}` from provider `{provider_id}`");
            } else {
                println!("Header `{name}` not found on provider `{provider_id}`");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_provider_output_hides_api_keys() {
        let output = render_provider_output(&ProviderDisplayRecord {
            id: "openai".to_string(),
            display_name: "OpenAI".to_string(),
            kind: "openai-compatible".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            models: vec!["gpt-4o-mini".to_string()],
            default_model: "gpt-4o-mini".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
        });

        assert!(output.contains("OpenAI"));
        assert!(output.contains("gpt-4o-mini"));
        assert!(!output.contains("sk-"));
        assert!(!output.contains("api_key"));
    }

    #[test]
    fn provider_upsert_args_reject_invalid_provider_kinds() {
        let args = ProviderUpsertArgs {
            id: "1".to_string(), // Now a string that will be parsed to i64
            display_name: "Test".to_string(),
            kind: "invalid-kind".to_string(),
            base_url: "https://example.com/v1".to_string(),
            api_key: "sk-test".to_string(),
            model: "test-model".to_string(),
            is_default: false,
        };

        let error =
            LlmProviderRecord::try_from(args).expect_err("invalid provider kind should fail");
        assert!(error.to_string().contains("invalid provider kind"));
    }
}
