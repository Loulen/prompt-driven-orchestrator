//! `GET /stats/absorptions`: every absorption of the instance, grouped by
//! absorbent.

use std::sync::Arc;

use crate::application::ports::AbsorptionStore;
use crate::domain::absorption::list::{group, AbsorptionList};

pub struct ListAbsorptions {
    store: Arc<dyn AbsorptionStore>,
}

impl ListAbsorptions {
    pub fn new(store: Arc<dyn AbsorptionStore>) -> Self {
        Self { store }
    }

    pub async fn execute(&self) -> anyhow::Result<AbsorptionList> {
        Ok(group(self.store.list().await?))
    }
}
