// Init
// Small questoinaire
// Then build template

// Build/Release
// Load TOML files
// Parse TOML
//
// Convert Assets using addon converters into templates
// 1. Music (easy)
// 2. Kit Box/MOS
// 3. Rank Armor
// 4. Roster/Custom Armor
// Write templates and file structure
// Copy `addons` folders into generated build folder
//
// Potentially chain into armake2 for PBO building

// Potentially build some editing tools that automatically edit the TOMLs (CLI/GUI)


#[macro_use] extern crate async_trait;
#[macro_use] extern crate tracing;

use std::error::Error;
use std::path::PathBuf;
use crate::config::LaatConfig;
use std::collections::HashMap;
use crate::context::BuildContext;


/// LAAT Compiler
pub struct LaatCompiler {
    config: LaatConfig,
    /// List of compiler plugins
    plugins: HashMap<String, Box<dyn Plugin>>
}

impl LaatCompiler {
    pub async fn build(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.clean().await?;

        for (name, plugin) in self.plugins.iter() {
            info!("Running {}.", name);
            plugin.build(self.get_context()).await?;
        }

        info!("Success. Mod has been generated at: ./{}", self.get_context().build_path);

        Ok(())
    }

    fn get_context(&self) -> BuildContext {
        self.config.clone()
    }

    pub async fn clean(&self) -> Result<(), Box<dyn Error>>{
        info!("Clearing build directory");

        if let Err(why) = tokio::fs::remove_dir_all(self.get_context().build_path).await {
            warn!("Failed to clear build folder: {}", why);
        }

        Ok(())
    }

    pub async fn from_path(path: PathBuf) -> Result<Self, Box<dyn Error>> {
        let config = config::get_config_from_path(path)?;

        let mut plugins = HashMap::new();

        plugins.insert("AddonPlugin".to_string(), Box::new(AddonPlugin) as Box<dyn Plugin>);
        plugins.insert("MusicPlugin".to_string(), Box::new(MusicPlugin) as Box<dyn Plugin>);

        Ok(Self {
            config,
            plugins
        })
    }
}

use crate::plugins::{MusicPlugin, AddonPlugin};
use plugins::Plugin;
pub mod plugins {
    use std::error::Error;
    use super::context::BuildContext;

    #[async_trait]
    pub trait Plugin {
        async fn build(&self, _: BuildContext) -> Result<(), Box<dyn Error>>;
    }

    mod music;
    pub use music::MusicPlugin;

    mod addons;
    pub use addons::AddonPlugin;
}

pub mod context {
    pub type BuildContext = super::config::LaatConfig;
}

mod config;
