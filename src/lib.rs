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

#[macro_use]
extern crate async_trait;
#[macro_use]
extern crate tracing;

use crate::config::LaatConfig;
use crate::context::BuildContext;
use armake2::pbo::cmd_build;
use handlebars::Handlebars;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use structopt::StructOpt;
use tokio::io::AsyncWriteExt;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

pub type Result<T> = std::result::Result<T, Error>;

const PROJECT_FOLDERS: &[&str] = &["addons", "assets", "build", "release"];

#[derive(Clone, Debug, StructOpt, Serialize)]
pub struct InitSettings {
    #[structopt(parse(from_os_str))]
    /// Path to project destination
    path: PathBuf,

    #[structopt(short, long)]
    /// Prefix for the mod
    prefix: String,

    #[structopt(short, long, default_value = "Avery Wagar")]
    /// Name of the mod author
    author: String,
}

#[derive(Clone, Debug, StructOpt, Serialize)]
pub struct ReleaseSettings {
    /// Steam Username
    username: String,
    /// Steam Password
    password: String,
    /// Steam Guard Key
    gaurd_key: String,

    /// Disable changelog
    #[structopt(long)]
    no_change_log: bool,

    /// Path to changelog file
    #[structopt(short, parse(from_os_str))]
    change_log_file: Option<PathBuf>

}

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

    pub async fn pack(&self, sign: bool) -> Result<()> {
        info!("Packaging project...");
        let release_path = self.get_context().released_addon_path();

        self.setup_release_folder(&release_path).await?;
        self.create_mod_cpp(&release_path).await?;

        self.create_pbos(&release_path).await?;

        if sign {
            self.sign_pbos(&release_path).await?;
        }

        Ok(())
    }

    pub async fn create_keys(&self, name: PathBuf) -> Result<()> {
        let LaatConfig { keys_path, .. } = self.get_context();
        info!("Creating Keypair {:?}", name);

        let mut key_path = PathBuf::new();
        key_path.push(keys_path);

        tokio::fs::create_dir_all(&key_path).await?;

        key_path.push(name);

        armake2::sign::cmd_keygen(key_path)?;

        Ok(())
    }

    pub async fn get_keys(&self) -> Result<(PathBuf, PathBuf)> {
        let context = self.get_context();

        let walkdir = walkdir::WalkDir::new(&context.keys_path);

        for entry in walkdir {
            match entry {
                Ok(entry) => {
                    let file_name = entry.file_name().to_string_lossy().to_string();
                    if file_name.contains(".biprivatekey") || file_name.contains(".bikey") {
                        let mut parts = file_name.split(".");

                        if let Some(name) = parts.next() {
                            let base_path = entry
                                .path()
                                .parent()
                                .map(|p| p.to_owned())
                                .unwrap_or(context.keys_path.into());

                            let mut pubkey_path = base_path.clone();
                            pubkey_path.push(format!("{}.bikey", name));

                            let mut privkey_path = base_path.clone();
                            privkey_path.push(format!("{}.biprivatekey", name));

                            return Ok((privkey_path, pubkey_path));
                        }
                    }
                }
                Err(why) => warn!("Failed to walk dir: {}", why),
            }
        }

        Err("Keys not found!".into())
    }

    pub async fn sign(&self) -> Result<()> {
        let release_path = self.get_context().released_addon_path();

        self.sign_pbos(&release_path).await?;

        Ok(())
    }

    #[instrument(skip(self), err)]
    pub async fn sign_pbos(&self, release_path: &str) -> Result<()> {
        let (privkey_path, pubkey_path) = self.get_keys().await?;

        let walkdir = walkdir::WalkDir::new(format!("{}/addons", release_path));

        for entry in walkdir {
            match entry {
                Ok(entry) => {
                    let is_pbo = entry
                        .file_name()
                        .to_string_lossy()
                        .to_string()
                        .to_lowercase()
                        .ends_with(".pbo");

                    if is_pbo {
                        let path = entry.path().to_owned();
                        info!(?path, ?privkey_path, "Signing: {:?}", path);

                        // Sign
                        armake2::sign::cmd_sign(
                            privkey_path.clone(),
                            path,
                            None,
                            armake2::sign::BISignVersion::V2,
                        )?;
                    }
                }
                Err(why) => warn!("Error walking dir: {}", why),
            }
        }

        let file_name = pubkey_path
            .file_name()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or("key.bikey".to_string());

        info!("Copying key file: {}", file_name);

        tokio::fs::copy(pubkey_path, format!("{}/keys/{}", release_path, file_name)).await?;

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

        // Create file structure
        tokio::fs::create_dir_all(&release_path).await?;
        tokio::fs::create_dir_all(format!("{}/addons", release_path)).await?;
        tokio::fs::create_dir_all(format!("{}/keys", release_path)).await?;


        Ok(())
    }

    pub async fn create_mod_cpp(&self, release_path: &str) -> Result<()> {
        let handlebars = create_handlebars()?;
        let rendered = handlebars.render("mod.cpp", &self.get_context())?;
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

    pub async fn init(init: InitSettings) -> Result<Self> {
        let handlebars = create_handlebars()?;

        // Create LAAT.toml file
        let mut laat_toml = init.path.clone();
        laat_toml.push("LAAT.toml");
        let mut file = tokio::fs::File::create(laat_toml).await?;

        let contents = handlebars.render("laat.toml", &init)?;
        file.write_all(contents.as_bytes()).await?;

        // Create project folders
        for folder in PROJECT_FOLDERS {
            let mut path = init.path.clone();
            path.push(folder);
            tokio::fs::create_dir_all(path).await?;
        }

        // Init LAAT
        Self::from_path(init.path).await
    }
}

pub fn create_handlebars<'a>() -> Result<Handlebars<'a>> {
    let mut handlebars = Handlebars::new();

    handlebars.register_template_string(
        "music_addon",
        include_str!("../templates/music/cfg_music.ht"),
    )?;

    handlebars.register_template_string("laat.toml", include_str!("../templates/laat.toml.ht"))?;

    handlebars.register_template_string("workshop_upload.vdf", include_str!("../templates/workshop_upload.vdf.ht"))?;

    handlebars.register_template_string("mod.cpp", include_str!("../templates/mod.cpp.ht"))?;

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
            Box::new(KitPlugin),
        ]
    }

    mod music;
    pub use music::MusicPlugin;

    mod addons;
    pub use addons::AddonPlugin;

    mod customs;
    pub use customs::CustomsPlugin;

    mod kits;
    pub use kits::KitPlugin;
}

pub mod context;

mod config;
