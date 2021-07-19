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

use armake2::pbo::cmd_build;
use std::io::Write;
use std::error::Error;
use std::path::PathBuf;
use crate::config::LaatConfig;
use std::collections::HashMap;
use crate::context::BuildContext;
use handlebars::Handlebars;


/// LAAT Compiler
pub struct LaatCompiler {
    config: LaatConfig,
    /// List of compiler plugins
    plugins: HashMap<String, Box<dyn Plugin>>
}

impl LaatCompiler {
    pub async fn build(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.clean_build().await?;

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

    pub async fn clean_build(&self) -> Result<(), Box<dyn Error>>{
        info!("Clearing build directory");

        if let Err(why) = tokio::fs::remove_dir_all(self.get_context().build_path).await {
            warn!("Failed to clear build folder: {}", why);
        }

        if let Err(why) = tokio::fs::create_dir_all(self.get_context().build_path).await {
            warn!("Failed to create build folder: {}", why);
        }

        Ok(())
    }

    pub async fn pack(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Packaging project...");
        let release_path = format!("{}/@{}", self.get_context().release_path, self.get_context().prefix);

        self.setup_release_folder(&release_path).await?;
        self.create_pbos(&release_path).await?;
        self.create_mod_cpp(&release_path).await?;

        Ok(())
    }

    #[instrument(skip(self), err)]
    pub async fn create_pbos(&self, release_path: &str) -> Result<(), Box<dyn Error>> {
        let walkdir = walkdir::WalkDir::new(self.get_context().build_path).min_depth(2).max_depth(2);

        for entry in walkdir {
            match entry {
                Ok(entry) => {
                    if entry.file_type().is_dir() {
                        // Is Addon - make pbo
                        let LaatConfig { prefix, mut pack, .. } = self.get_context().clone();

                        let release_path = release_path.to_string();
                        tokio::task::spawn_blocking(move || {
                            let mut build_pbo =  || {
                                info!("{}", entry.path().display());

                                let file_name = entry.file_name().to_string_lossy();
                                let pbo_name = format!("{}.pbo", file_name);

                                pack.header_extensions.push(format!("prefix={}\\{}", prefix, file_name));

                                let mut output = std::fs::File::create(format!("{}/addons/{}", release_path, pbo_name))?;

                                cmd_build(
                                    entry.path().to_owned(), 
                                    &mut output,
                                    &pack.header_extensions,
                                    &pack.excludes,
                                    &pack.include_folders
                                )?;

                                Ok(())
                            };

                            let result: Result<(), Box<dyn Error>> = build_pbo();
                            if let Err(why) = result {
                                error!("Error creating pbo: {}", why);
                            }

                        });

                    }
                },
                Err(why) => warn!("Failed walking entry: {}", why),
            }
        }

        Ok(())
    }

    pub async fn setup_release_folder(&self, release_path: &str) -> Result<(), Box<dyn Error>> {
        info!("Clearing release directory");

        if let Err(why) = tokio::fs::remove_dir_all(self.get_context().release_path).await {
            warn!("Failed to clear build folder: {}", why);
        }

        tokio::fs::create_dir_all(&release_path).await?;
        tokio::fs::create_dir_all(format!("{}/addons", release_path)).await?;
        tokio::fs::create_dir_all(format!("{}/keys", release_path)).await?;

        Ok(())
    }

    pub async fn create_mod_cpp(&self, release_path: &str) -> Result<(), Box<dyn Error>> {
        let handlebars = create_handlebars()?;
        let rendered = handlebars.render("mod_cpp", &self.get_context())?;
        let mut file = std::fs::File::create(format!("{}/mod.cpp", release_path))?;
        file.write_fmt(format_args!("{}", rendered))?;

        Ok(())
    }

    pub async fn from_path(path: PathBuf) -> Result<Self, Box<dyn Error>> {
        let config = config::get_config_from_path(path)?;

        let mut plugins = HashMap::new();

        for plugin in config.plugins.iter() {
            plugins.insert(plugin.to_string(), plugins::get_plugin(plugin)?);
        }

        Ok(Self {
            config,
            plugins
        })
    }
}

pub fn create_handlebars<'a>() -> Result<Handlebars<'a>, Box<dyn Error>> {
    let mut handlebars = Handlebars::new();

    handlebars.register_template_string("music_addon", include_str!("../templates/music/cfg_music.ht"))?;
    handlebars.register_template_string("mod_cpp", include_str!("../templates/mod.cpp.ht"))?;

    Ok(handlebars)
}

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

    pub fn get_plugin(name: &str) -> Result<Box<dyn Plugin>, Box<dyn Error>> {
        match name {
            "music" => Ok(Box::new(MusicPlugin) as Box<dyn Plugin>),
            "addons" => Ok(Box::new(AddonPlugin) as Box<dyn Plugin>),
            x @ _ => Err(format!("Unknown Plugin: {}", x).into())
        }
    }
}

pub mod context {
    pub type BuildContext = super::config::LaatConfig;
}

mod config;
