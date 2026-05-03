//! Narrow repair hook for legacy agent template persistence cleanup.

use async_trait::async_trait;

use crate::error::DbError;

#[async_trait]
pub trait TemplateRepairRepository: Send + Sync {
    async fn repair_placeholder_ids(&self) -> Result<(), DbError>;
}
