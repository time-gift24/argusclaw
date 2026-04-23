use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct MutationResponse<T> {
    pub item: T,
}

impl<T> MutationResponse<T> {
    #[must_use]
    pub fn new(item: T) -> Self {
        Self { item }
    }
}
