//! Compiler Plugin for building retexture addons
//!
//! The idea is to reduce boilerplate from needing to redefine the same Cfg to retexture the same
//! model once per unit member.

use crate::context::AddonManager;
use super::{Plugin, BuildContext};
use crate::Result;

const ADDON_NAME: &str = "Customs";

pub struct CustomsPlugin;

#[async_trait]
impl Plugin for CustomsPlugin {
    async fn build(&self, build_config: BuildContext) -> Result<()> {

        let mut manager = AddonManager::from_context(ADDON_NAME.to_string(), build_config);


        Ok(())
    }

    fn name(&self) -> String {
        "customs".to_string()
    }
}
