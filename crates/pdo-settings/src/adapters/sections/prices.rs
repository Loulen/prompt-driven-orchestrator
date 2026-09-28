//! The price table's state: which of the three price tiers won, where its
//! files are, and what a hand edit refused. The resolved read view lives on
//! `/stats/cost`, beside the "Sync costs" action.

use pdo_run::adapters::price_files::price_table_state;
use serde_json::{Map, Value};

use super::{json, section, HostHome};
use crate::application::ports::{BoxFuture, SettingsSection};
use crate::domain::instance_config::InstanceConfig;

pub struct PriceTableSection {
    pub home: HostHome,
}

impl SettingsSection for PriceTableSection {
    fn disclose<'a>(
        &'a self,
        _stored: &'a InstanceConfig,
    ) -> BoxFuture<'a, Result<Map<String, Value>, String>> {
        Box::pin(async move {
            let home = self.home.path();
            Ok(section([(
                "price_table",
                json(price_table_state(home.as_deref())),
            )]))
        })
    }
}
