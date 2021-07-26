//! Compiler Plugin for building kit/armor boxes
//!

use std::collections::HashMap;
use crate::context::AddonManager;
use super::{Plugin, BuildContext};
use crate::Result;
use serde::{Serialize, Deserialize};
use tokio::io::AsyncReadExt;

const ADDON_NAME: &str = "Kits";

#[derive(Debug)]
pub struct KitPlugin;

/// Item entry from a kit - count, item class, location
type MultiItemEntry = (usize, String, Location);

type ItemEntry = (String, Location);

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum Item {
    Multi(MultiItemEntry),
    Single(ItemEntry)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Location {
    Vest,
    Backpack,
    Uniform,
    Link,
    AddAssign
}

#[derive(Debug, Serialize, Deserialize)]
struct Kit {
    weapons: Vec<String>,
    vest: String,
    backpack: String,

    #[serde(default)]
    traits: Vec<String>,

    #[serde(default)]
    items: Vec<Item>,
    #[serde(default)]
    components: Vec<String>
}

#[derive(Debug, Serialize, Deserialize)]
struct Component(Vec<Item>);

#[derive(Debug, Serialize, Deserialize)]
struct KitFile {
    kits: HashMap<String, Kit>,
    components: HashMap<String, Component>
}

#[async_trait]
impl Plugin for KitPlugin {
    #[instrument(err)]
    async fn build(&self, build_config: BuildContext) -> Result<()> {
        // Load component map
        let kit_file = load_kit_config(&build_config).await?;

        debug!("Kit File: {:?}", kit_file);

        let mut manager = AddonManager::from_context(ADDON_NAME.to_string(), build_config);

        // Create kits from components
        //
        // Create SQFs for loading kits, with proper error reporting if not enough space etc.
        //
        // Map SQFs to CfgFunctions.
        //

        // Create box templates with useractions

        Ok(())
    }

    fn name(&self) -> String {
        "kits".to_string()
    }
}

const FILE_FIELD: &str = "kits.file";
const DEFAULT_FILE: &str = "kits.toml";

async fn load_kit_config(build_config: &BuildContext) -> Result<KitFile> {
    let file_path = build_config.extra.get(FILE_FIELD).map(|v| v.as_str()).flatten().unwrap_or(DEFAULT_FILE);

    let mut kit_file = tokio::fs::File::open(file_path).await?;
    let mut contents = String::new();
    kit_file.read_to_string(&mut contents).await?;

    let kit_file = toml::from_str(&contents)?;

    Ok(kit_file)
}
