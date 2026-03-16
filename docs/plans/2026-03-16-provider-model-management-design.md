# Provider Model Management Design

**Goal:** Let users configure one-to-many models while creating a provider, instead of requiring a save-before-models flow.

**Approach:** Keep provider persistence and model persistence separate, but make the provider dialog manage both. For unsaved providers, collect draft models locally in the dialog. After provider save succeeds, persist those draft models and reload the provider's real model list. For existing providers, keep using the existing model CRUD commands directly.

**Notes:**
- The provider ID already exists in the form, so draft model IDs can be derived at persistence time.
- The dialog should always show the model section.
- One model should be marked as default across both draft and persisted lists.
- Failures during model persistence should leave the dialog open and surface an inline error.
