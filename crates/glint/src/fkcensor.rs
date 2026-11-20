//! Runtime map of the community-maintained clean-track replacements.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone, Default)]
pub struct FkCensor {
    replacements: Arc<RwLock<HashMap<String, ym_api::fkcensor::Replacement>>>,
}

impl FkCensor {
    pub fn replacement(&self, track_id: &str) -> Option<ym_api::fkcensor::Replacement> {
        self.replacements.read().ok()?.get(track_id).cloned()
    }

    pub async fn refresh(&self, client: &ym_api::Client) -> bool {
        let Ok(map) = client.fkcensor_replacements().await else { return false };
        let Ok(mut current) = self.replacements.write() else { return false };
        if *current == map {
            return false;
        }
        *current = map;
        true
    }
}
