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
#[macro_use] extern crate async_recursion;
#[macro_use] extern crate tracing;

use crate::config::LaatConfig;
use crate::context::BuildContext;
use armake2::pbo::cmd_build;
use handlebars::Handlebars;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

pub type Result<T> = std::result::Result<T, Error>;

/// LAAT Compiler
pub struct LaatCompiler {
    config: LaatConfig,
    /// List of compiler plugins
    plugins: HashMap<String, Box<dyn Plugin>>,
}

impl LaatCompiler {
    pub async fn build(&self) -> Result<()> {
        self.clean_build().await?;

        for (name, plugin) in self.plugins.iter() {
            info!("Running {}.", name);
            plugin.build(self.get_context()).await?;
        }

        info!(
            "Success. Mod has been generated at: ./{}",
            self.get_context().build_path
        );

        Ok(())
    }

    fn get_context(&self) -> BuildContext {
        self.config.clone()
    }

    pub async fn clean_build(&self) -> Result<()> {
        info!("Clearing build directory");

        if let Err(why) = tokio::fs::remove_dir_all(self.get_context().build_path).await {
            warn!("Failed to clear build folder: {}", why);
        }

        if let Err(why) = tokio::fs::create_dir_all(self.get_context().build_path).await {
            warn!("Failed to create build folder: {}", why);
        }

        Ok(())
    }

    pub async fn pack(&self) -> Result<()> {
        info!("Packaging project...");
        let release_path = format!(
            "{}/@{}",
            self.get_context().release_path,
            self.get_context().prefix
        );

        self.setup_release_folder(&release_path).await?;
        self.create_pbos(&release_path).await?;
        self.create_mod_cpp(&release_path).await?;

        Ok(())
    }

    #[instrument(skip(self), err)]
    pub async fn create_pbos(&self, release_path: &str) -> Result<()> {
        let walkdir = walkdir::WalkDir::new(self.get_context().build_path)
            .min_depth(2)
            .max_depth(2);

        for entry in walkdir {
            match entry {
                Ok(entry) => {
                    if entry.file_type().is_dir() {
                        // Is Addon - make pbo
                        let LaatConfig {
                            prefix, mut pack, ..
                        } = self.get_context().clone();

                        let release_path = release_path.to_string();
                        tokio::task::spawn_blocking(move || {
                            let mut build_pbo = || {
                                info!("{}", entry.path().display());

                                let file_name = entry.file_name().to_string_lossy();
                                let pbo_name = format!("{}.pbo", file_name);

                                pack.header_extensions
                                    .push(format!("prefix={}\\{}", prefix, file_name));

                                let mut output = std::fs::File::create(format!(
                                    "{}/addons/{}",
                                    release_path, pbo_name
                                ))?;

                                cmd_build(
                                    entry.path().to_owned(),
                                    &mut output,
                                    &pack.header_extensions,
                                    &pack.excludes,
                                    &pack.include_folders,
                                )?;

                                Ok(())
                            };

                            let result: Result<()> = build_pbo();
                            if let Err(why) = result {
                                error!("Error creating pbo: {}", why);
                            }
                        });
                    }
                }
                Err(why) => warn!("Failed walking entry: {}", why),
            }
        }

        Ok(())
    }

    pub async fn setup_release_folder(&self, release_path: &str) -> Result<()> {
        info!("Clearing release directory");

        if let Err(why) = tokio::fs::remove_dir_all(self.get_context().release_path).await {
            warn!("Failed to clear build folder: {}", why);
        }

        tokio::fs::create_dir_all(&release_path).await?;
        tokio::fs::create_dir_all(format!("{}/addons", release_path)).await?;
        tokio::fs::create_dir_all(format!("{}/keys", release_path)).await?;

        Ok(())
    }

    pub async fn create_mod_cpp(&self, release_path: &str) -> Result<()> {
        let handlebars = create_handlebars()?;
        let rendered = handlebars.render("mod_cpp", &self.get_context())?;
        let mut file = std::fs::File::create(format!("{}/mod.cpp", release_path))?;
        file.write_fmt(format_args!("{}", rendered))?;

        Ok(())
    }

    pub async fn from_path(path: PathBuf) -> Result<Self> {
        let config = config::get_config_from_path(path).await?;

        let mut plugins = HashMap::new();

        for plugin in config.plugins.iter() {
            plugins.insert(plugin.to_string(), plugins::get_plugin(plugin)?);
        }

        Ok(Self { config, plugins })
    }
}

pub fn create_handlebars<'a>() -> Result<Handlebars<'a>> {
    let mut handlebars = Handlebars::new();

    handlebars.register_template_string(
        "music_addon",
        include_str!("../templates/music/cfg_music.ht"),
    )?;
    handlebars.register_template_string("mod_cpp", include_str!("../templates/mod.cpp.ht"))?;

    Ok(handlebars)
}

use plugins::Plugin;
pub mod plugins {
    use super::context::BuildContext;
    use crate::Result;

    #[async_trait]
    pub trait Plugin {
        async fn build(&self, _: BuildContext) -> Result<()>;
        fn name(&self) -> String;
    }

    pub fn get_plugin(name: &str) -> Result<Box<dyn Plugin>> {
        plugins()
            .into_iter()
            .find(|p| p.name() == name)
            .map(|p| Ok(p))
            .unwrap_or(Err(format!("Unknown Plugin: {}", name).into()))
    }

    pub fn plugins() -> Vec<Box<dyn Plugin>> {
        vec![
            Box::new(MusicPlugin),
            Box::new(AddonPlugin),
            Box::new(CustomsPlugin),
        ]
    }

    mod music;
    pub use music::MusicPlugin;

    mod addons;
    pub use addons::AddonPlugin;

    mod customs;
    pub use customs::CustomsPlugin;
}

pub mod context {
    use crate::Result;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tokio::io::AsyncWriteExt;

    pub type BuildContext = super::config::LaatConfig;

    /// An Addon manager that takes the BuildContext and Addon name. Then prepares prefixed paths
    /// for asset paths that are passed to it, then copies all the assets over into the build
    /// folder. It also manages the config.cpp creation.
    pub struct AddonManager {
        /// Addon Name (i.e. "Music", "Customs", etc.)
        addon: String,

        /// Build context from LAAT
        build_context: BuildContext,

        /// String containing contents of config_cpp
        config_cpp: String,

        /// Map containing ./assets paths and maps them to Addon prefixed paths.
        assets: HashMap<PathBuf, PathBuf>,
    }

    impl AddonManager {
        /// Create a new Addon from an addon name and build_context
        pub fn from_context(addon: String, build_context: BuildContext) -> Self {
            Self {
                addon,
                build_context,
                config_cpp: String::new(),
                assets: HashMap::new(),
            }
        }

        /// Return the name of this addon
        pub fn addon_name(&self) -> String {
            self.addon.to_owned()
        }

        /// Returns the prefixed addon path
        pub fn addon_path(&self) -> PathBuf {
            format!(r"{}/{}", self.build_context.prefix, self.addon).into()
        }

        pub fn build_path(&self) -> PathBuf {
            self.build_context.build_path.clone().into()
        }

        /// Add an asset to the AssetManager
        ///
        /// Returns the new path for the asset, once copied to the module.
        pub fn add_asset(&mut self, asset_path: PathBuf, addon_folder: Option<PathBuf>) -> Result<PathBuf> {
            let mut addon_path = PathBuf::new();
            addon_path.push(self.addon_path()); // 17th/{addon}

            if let Some(folder) = addon_folder {
                addon_path.push(folder);
            }

            if let Some(file) = asset_path.file_name() {
                addon_path.push(file); // texture.ogg
            }
            else {
                return Err(format!("Failed to get file name for: {:?}", asset_path).into());
            }

            self.assets.insert(asset_path, addon_path.clone());

            Ok(addon_path)
        }

        /// Iterates over the loaded assets, and copies them to their destined module paths. This
        /// will also create the addon folder if it doesn't already exists.
        async fn copy_assets(&self) -> Result<()> {
            let mut futs = Vec::new();

            for (asset, addon_path) in self.assets.clone().into_iter() {
                let mut dest = self.build_path();
                dest.push(addon_path);

                let fut = tokio::spawn(async move {
                    if let Some(parent) = dest.parent() {
                        if let Err(why) = tokio::fs::create_dir_all(parent).await {
                            error!("Failed to create folder: {:?}. Error: {}", parent, why);
                        }
                    }

                    if let Err(why) = tokio::fs::copy(&asset, &dest).await {
                        error!("Failed to copy {:?} to {:?}. Error: {}", asset, dest, why);
                    }
                });

                futs.push(fut);
            }

            futures_util::future::join_all(futs).await;

            Ok(())
        }

        /// Set the value to write to addon/config.cpp
        pub fn set_config_cpp(&mut self, config_cpp: String) {
            self.config_cpp = config_cpp;
        }

        /// Returns the addon path for addon's config.cpp file.
        pub fn get_config_cpp_path(&self) -> PathBuf {
            let mut config_cpp = PathBuf::new();
            config_cpp.push(self.addon_path());
            config_cpp.push("config.cpp");

            config_cpp
        }

        async fn write_config_cpp(&self) -> Result<()> {
            let mut file_path = self.build_path();
            file_path.push(self.get_config_cpp_path());

            let mut config_cpp = tokio::fs::File::create(file_path).await?;
            config_cpp.write(self.config_cpp.as_bytes()).await?;

            Ok(())
        }

        async fn create_addon_folder(&self) -> Result<()> {
            let mut addon_dir = self.build_path();
            addon_dir.push(self.addon_path());

            tokio::fs::create_dir_all(addon_dir).await?;

            Ok(())
        }

        /// Build the addon
        pub async fn build_addon(&self) -> Result<()> {
            self.create_addon_folder().await?;

            tokio::try_join!(self.write_config_cpp(), self.copy_assets())?;

            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use toml::Value;
        use crate::config::PackConfig;
        use super::*;

        fn build_context() -> BuildContext {
            BuildContext {
                prefix: "LAAT".to_string(),
                name: "LAAT Test Mod".to_string(),
                build_path: "build".to_string(),
                assets_path: "assets".to_string(),
                addons_path: "addons".to_string(),
                release_path: "release".to_string(),
                plugins: vec![],
                pack: PackConfig {
                    include_folders: vec![],
                    excludes: vec![],
                    header_extensions: vec![]
                },
                extra: Value::Float(0.0),
            }
        }
        

        #[test]
        fn test_asset_pathing() -> Result<()> {
            let mut manager = AddonManager::from_context("Test".to_string(), build_context());

            let addon_path = manager.add_asset("./asset/test.txt".into(), None)?;

            let expected_path: PathBuf = "LAAT/Test/test.txt".into();

            assert_eq!(addon_path, expected_path);

            Ok(())
        }

        #[test]
        fn test_asset_pathing_with_folder() -> Result<()> {
            let mut manager = AddonManager::from_context("Test".to_string(), build_context());

            let addon_path = manager.add_asset("./asset/test.txt".into(), Some("data/text".into()))?;

            let expected_path: PathBuf = "LAAT/Test/data/text/test.txt".into();

            assert_eq!(addon_path, expected_path);

            Ok(())
        }

        #[test]
        fn test_get_config_cpp_path() -> Result<()> {
            let manager = AddonManager::from_context("Test".to_string(), build_context());

            let config_cpp_path = manager.get_config_cpp_path();

            let expected_path: PathBuf = "LAAT/Test/config.cpp".into();

            assert_eq!(config_cpp_path, expected_path);

            Ok(())

        }
    }
}

mod config;
