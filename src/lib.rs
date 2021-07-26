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

use std::path::Path;
use futures_util::future::join_all;
use crate::config::LaatConfig;
use crate::context::BuildContext;
use armake2::pbo::cmd_build;
use handlebars::Handlebars;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use structopt::StructOpt;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

pub type Result<T> = std::result::Result<T, Error>;

const PROJECT_FOLDERS: &[&str] = &["addons", "assets", "build", "release"];
const GITIGNORE: &str = 
r"
build
release
";

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
    #[structopt(short)]
    username: String,
    /// Steam Password
    #[structopt(short)]
    password: String,
    /// Steam Guard Key
    #[structopt(short)]
    guard_key: Option<String>,

    /// Disable changelog
    #[structopt(long)]
    no_change_log: bool,

    /// Path to changelog file
    #[structopt(short, parse(from_os_str))]
    change_log_file: Option<PathBuf>,
}

/// LAAT Compiler
pub struct LaatCompiler {
    config: LaatConfig,
    /// List of compiler plugins
    plugins: HashMap<String, Box<dyn Plugin>>,
}

impl LaatCompiler {
    #[instrument(skip(self))]
    pub async fn build(&self) -> Result<()> {
        info!("Generating Arma 3 Addons...");
        self.clean_build().await?;

        for (name, plugin) in self.plugins.iter() {
            debug!("Running {}.", name);
            plugin.build(self.get_context()).await?;
        }

        info!(
            "Success! Mod has been generated at: ./{}",
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

    #[instrument(skip(self))]
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

    #[instrument(skip(self))]
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

    #[instrument(skip(self))]
    pub async fn sign(&self) -> Result<()> {
        let release_path = self.get_context().released_addon_path();

        info!("Signing PBOs...");

        self.sign_pbos(&release_path).await?;

        Ok(())
    }

    #[instrument(skip(self, release_path), err)]
    pub async fn sign_pbos(&self, release_path: &str) -> Result<()> {
        let (privkey_path, pubkey_path) = self.get_keys().await?;

        let walkdir = walkdir::WalkDir::new(format!("{}/addons", release_path));

        let mut sign_futs = Vec::new();

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
                        debug!(?path, ?privkey_path, "Signing: {:?}", path);

                        // Sign
                        let privkey_path = privkey_path.clone();

                        let fut = tokio::task::spawn_blocking(move || {
                            if let Err(why) = armake2::sign::cmd_sign(
                                privkey_path,
                                path,
                                None,
                                armake2::sign::BISignVersion::V2,
                            ) {
                                error!(?why, "Error signing PBO!");
                            }
                        });

                        sign_futs.push(fut);
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

        join_all(sign_futs).await;

        Ok(())
    }

    #[instrument(skip(self), err)]
    pub async fn create_pbos(&self, release_path: &str) -> Result<()> {
        let walkdir = walkdir::WalkDir::new(self.get_context().build_path)
            .min_depth(2)
            .max_depth(2);

        let mut pbo_futs = Vec::new();

        for entry in walkdir {
            match entry {
                Ok(entry) => {
                    if entry.file_type().is_dir() {
                        // Is Addon - make pbo
                        let LaatConfig {
                            prefix, mut pack, ..
                        } = self.get_context().clone();

                        let release_path = release_path.to_string();

                        let fut = tokio::task::spawn_blocking(move || {
                            let mut build_pbo = || {
                                debug!("Creating PBO: {}", entry.path().display());

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

                        pbo_futs.push(fut);
                    }
                }
                Err(why) => warn!("Failed walking entry: {}", why),
            }
        }

        join_all(pbo_futs).await;

        Ok(())
    }

    pub async fn setup_release_folder(&self, release_path: &str) -> Result<()> {
        info!("Clearing release directory...");

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

    #[instrument]
    pub async fn init(init: InitSettings) -> Result<Self> {
        let handlebars = create_handlebars()?;

        // Create project folders
        for folder in PROJECT_FOLDERS {
            let mut path = init.path.clone();
            path.push(folder);
            debug!("Creating folder: {:?}", path);
            tokio::fs::create_dir_all(path).await?;
        }

        // Create LAAT.toml file
        let mut laat_toml = init.path.clone();
        laat_toml.push("LAAT.toml");

        let contents = handlebars.render("laat.toml", &init)?;
        create_and_write_file(&laat_toml, contents).await?;

        let mut gitignore = init.path.clone();
        gitignore.push(".gitignore");

        create_and_write_file(&gitignore, GITIGNORE).await?;


        // Init LAAT
        Self::from_path(init.path).await
    }

    /// Release mod to Steam Workshop
    #[instrument(skip(self, release), err)]
    pub async fn release(&self, release: ReleaseSettings) -> Result<()> {
        let context = self.get_context();

        // 1. Get Changelog
        let change_log = if let Some(log_file) = release.change_log_file {
            debug!("Loading change log");
            let mut file = tokio::fs::File::open(log_file).await?;
            let mut contents = String::new();
            file.read_to_string(&mut contents).await?;

            contents
        } else if !release.no_change_log {
            debug!("Creating change log file");
            let change_file_path = PathBuf::from("/tmp/changenote.log");

            // TODO: Default changelog file

            let editor = std::env::var("EDITOR")?;

            info!("Waiting for {} to close...", editor);
            let mut editor = tokio::process::Command::new(editor)
                .arg(&change_file_path)
                .spawn()?;

            editor.wait().await?;

            debug!("Opening {:?}", change_file_path);
            let mut change_log_file = tokio::fs::File::open(&change_file_path).await?;

            debug!("Reading {:?} contents", change_file_path);

            let mut contents = String::new();
            change_log_file.read_to_string(&mut contents).await?;

            contents
        } else {
            String::new()
        };

        // 2. Strip " from changelog
        let changenotes = change_log.replace("\"", "");

        let mut content_folder: PathBuf = std::env::var("PWD")?.into();
        content_folder.push(context.released_addon_path());


        // 3. render workshop_upload.vdf
        let workshop_item = WorkshopItem {
            app_id: context.release.app_id,
            file_id: context.release.workshop_id,
            content_folder,
            changenotes,
        };

        debug!(?workshop_item, "Rendering SteamCMD VDF");
        let handlebars = create_handlebars()?;
        let rendered = handlebars.render("workshop_upload.vdf", &workshop_item)?;
        let vdf_path: PathBuf = "/tmp/workshop_upload.vdf".into();

        // Write to temp file
        debug!("Writing VDF file");
        create_and_write_file(&vdf_path, rendered).await?;

        // 4. bash "steamcmd +login steamuser steampass steamguard +workshop_build_item ${PWD}/test.vdf +quit"
        info!("Starting SteamCMD");
        let mut steamcmd = tokio::process::Command::new("steamcmd");
        let mut steamcmd = steamcmd
            .arg("+login")
            .arg(release.username)
            .arg(release.password);

        if let Some(key) = release.guard_key {
            steamcmd = steamcmd.arg(key);
        }

        steamcmd = steamcmd
            .arg("+workshop_build_item")
            .arg(vdf_path)
            .arg("+quit");

        steamcmd.spawn()?.wait().await?;

        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct WorkshopItem {
    app_id: usize,
    file_id: usize,
    content_folder: PathBuf,
    changenotes: String,
}

pub fn create_handlebars<'a>() -> Result<Handlebars<'a>> {
    let mut handlebars = Handlebars::new();

    handlebars.register_template_string(
        "music_addon",
        include_str!("../templates/music/cfg_music.ht"),
    )?;

    handlebars.register_template_string("laat.toml", include_str!("../templates/laat.toml.ht"))?;

    handlebars.register_template_string(
        "workshop_upload.vdf",
        include_str!("../templates/workshop_upload.vdf.ht"),
    )?;

    handlebars.register_template_string("mod.cpp", include_str!("../templates/mod.cpp.ht"))?;

    Ok(handlebars)
}

async fn create_and_write_file(file_path: impl AsRef<Path>, contents: impl Into<String>) -> Result<()> {
    let mut file = tokio::fs::File::create(file_path).await?;
    file.write_all(contents.into().as_bytes()).await?;

    Ok(())
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
