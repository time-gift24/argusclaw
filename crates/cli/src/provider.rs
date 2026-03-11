//! Provider command - shared between argusclaw and argusclaw-dev.
//!
//! This module contains the provider management commands that are common
//! to both the production and development CLI binaries.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand};
use claw::AppContext;
use claw::db::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderRecord, LlmProviderSummary, SecretString,
};

#[cfg(feature = "dev")]
use crate::dev::config;

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
    /// 从 TOML 配置文件导入提供商。
    #[cfg(feature = "dev")]
    Import {
        /// TOML 配置文件路径。
        #[arg(long)]
        file: String,
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
    pub model: String,
    pub is_default: bool,
    pub extra_headers: HashMap<String, String>,
}

impl From<LlmProviderSummary> for ProviderDisplayRecord {
    fn from(value: LlmProviderSummary) -> Self {
        Self {
            id: value.id.to_string(),
            display_name: value.display_name,
            kind: value.kind.to_string(),
            base_url: value.base_url,
            model: value.model,
            is_default: value.is_default,
            extra_headers: value.extra_headers,
        }
    }
}

impl From<LlmProviderRecord> for ProviderDisplayRecord {
    fn from(value: LlmProviderRecord) -> Self {
        Self {
            id: value.id.to_string(),
            display_name: value.display_name,
            kind: value.kind.to_string(),
            base_url: value.base_url,
            model: value.model,
            is_default: value.is_default,
            extra_headers: value.extra_headers,
        }
    }
}

impl TryFrom<ProviderUpsertArgs> for LlmProviderRecord {
    type Error = claw::db::DbError;

    fn try_from(value: ProviderUpsertArgs) -> Result<Self, Self::Error> {
        Ok(Self {
            id: LlmProviderId::new(value.id),
            kind: value.kind.parse::<LlmProviderKind>()?,
            display_name: value.display_name,
            base_url: value.base_url,
            api_key: SecretString::new(value.api_key),
            model: value.model,
            is_default: value.is_default,
            extra_headers: HashMap::new(),
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

    format!(
        "id: {}\ndisplay_name: {}\nkind: {}\nbase_url: {}\nmodel: {}\nis_default: {}{}",
        record.id,
        record.display_name,
        record.kind,
        record.base_url,
        record.model,
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
pub async fn run_provider_command(ctx: AppContext, command: ProviderCommand) -> Result<()> {
    match command {
        ProviderCommand::List => {
            for provider in ctx.llm_manager().list_providers().await? {
                println!("{}", render_provider_output(&provider.into()));
                println!();
            }
        }
        ProviderCommand::Get { id } => {
            let provider = ctx.get_provider_record(&LlmProviderId::new(id)).await?;
            println!("{}", render_provider_output(&provider.into()));
        }
        ProviderCommand::Upsert(args) => {
            let record = LlmProviderRecord::try_from(args).map_err(|e| anyhow!(e.to_string()))?;
            ctx.upsert_provider(record).await?;
        }
        ProviderCommand::SetDefault { id } => {
            ctx.set_default_provider(&LlmProviderId::new(id)).await?;
        }
        ProviderCommand::GetDefault => {
            let provider = ctx.get_default_provider_record().await?;
            println!("{}", render_provider_output(&provider.into()));
        }
        ProviderCommand::SetHeader { id, name, value } => {
            validate_header_name(&name)?;
            let provider_id = LlmProviderId::new(&id);
            let mut record = ctx.get_provider_record(&provider_id).await?;
            record.extra_headers.insert(name.clone(), value);
            ctx.upsert_provider(record).await?;
            println!("Set header `{name}` on provider `{id}`");
        }
        ProviderCommand::RemoveHeader { id, name } => {
            let provider_id = LlmProviderId::new(&id);
            let mut record = ctx.get_provider_record(&provider_id).await?;
            if record.extra_headers.remove(&name).is_some() {
                ctx.upsert_provider(record).await?;
                println!("Removed header `{name}` from provider `{id}`");
            } else {
                println!("Header `{name}` not found on provider `{id}`");
            }
        }
        #[cfg(feature = "dev")]
        ProviderCommand::Import { file } => {
            let contents = std::fs::read_to_string(Path::new(&file))
                .with_context(|| format!("failed to read provider import file `{file}`"))?;
            let config: config::ProviderImportFile =
                toml::from_str(&contents).context("failed to parse provider import toml")?;
            let records = config.into_records().map_err(|e| anyhow!(e.to_string()))?;
            ctx.import_providers(records).await?;
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
            model: "gpt-4o-mini".to_string(),
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
            id: "test".to_string(),
            display_name: "Test".to_string(),
            kind: "invalid-kind".to_string(),
            base_url: "https://example.com/v1".to_string(),
            api_key: "sk-test".to_string(),
            model: "test-model".to_string(),
            is_default: false,
        };

        let error =
            LlmProviderRecord::try_from(args).expect_err("invalid provider kind should fail");
        assert!(error.to_string().contains("invalid llm provider kind"));
    }
}
