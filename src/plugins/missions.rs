use crate::context::AddonManager;
use crate::create_handlebars;
use crate::BuildContext;
use crate::Plugin;
use crate::Result;
use armake2::config::Config;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;

const MISSION_SETTINGS_KEY: &str = "missions";

#[derive(Debug)]
pub struct MissionPlugin;

#[async_trait]
impl Plugin for MissionPlugin {
    #[instrument(err, skip(build_config))]
    async fn build(&self, build_config: BuildContext) -> Result<()> {

        // Extract MissionSettings from BuildContext
        let mission_settings = MissionSettings::from_build_config(&build_config)?;

        // Load composition file
        let composition = load_composition(&mission_settings.composition).await?;

        // For each Map create mission based on settings.
        let mut missions = create_missions(&mission_settings, &build_config).await?;

        // Merge composition into mission
        missions
            .iter_mut()
            .for_each(|mission| mission.merge_composition(&composition));

        // Save mission to addon
        let mut addon_manager =
            AddonManager::from_context(&mission_settings.addon_name, build_config.clone());

        let classes = missions
            .iter()
            .map(|mission| {
                let path: PathBuf = format!("missions/{}/mission.sqm", mission.mission_name()).into();

                addon_manager.add_file(mission.to_sqm(), path.clone());

                (path, mission.mission_name())
            })
            .collect::<Vec<_>>();

        // Write config exposing Missions
        info!("Writing config.cpp...");
        let handlebars = create_handlebars()?;

        let addon = Addon::from_parts(build_config.prefix, mission_settings.addon_name, classes);
        let config_cpp = handlebars.render("missions_addon", &addon)?;

        addon_manager.add_file(config_cpp, "config.cpp".into());

        info!("Building Addon...");
        addon_manager.build_addon().await?;

        Ok(())
    }

    fn name(&self) -> String {
        "missions".to_string()
    }
}

#[derive(Debug, Deserialize)]
struct MissionSettings {
    #[serde(default = "default_addon_name")]
    /// Name of the generated Addon
    addon_name: String,

    /// List of maps to create missions for
    maps: Vec<String>,

    /// Mission name
    #[serde(default = "default_mission_name")]
    mission_name: String,

    /// Delay, in seconds between death and when allowed to respawn.
    #[serde(default = "default_respawn_delay")]
    respawn_delay: usize,

    /// Composition to add to missions
    composition: PathBuf,

    #[serde(default)]
    /// X, Y, Z offset for the composition.
    composition_offset: (f64, f64, f64),
}

impl MissionSettings {
    pub fn from_build_config(build_config: &BuildContext) -> Result<MissionSettings> {
        if let Some(mission_settings) = build_config.extra.get(MISSION_SETTINGS_KEY) {
            let mission_settings: MissionSettings = mission_settings.clone().try_into()?;

            Ok(mission_settings)
        } else {
            Err(format!(
                "Failed to get field: {} from LAAT.toml",
                MISSION_SETTINGS_KEY
            )
            .into())
        }
    }
}

fn default_addon_name() -> String {
    "Missions".to_string()
}

fn default_mission_name() -> String {
    "ZeusMission".to_string()
}

fn default_respawn_delay() -> usize {
    2
}

struct Composition {
    header: Config,
    composition: Config,
}

impl Composition {
    #[instrument(err)]
    pub async fn from_path(path: &PathBuf) -> Result<Self> {
        let (header, composition) = tokio::join!(
            tokio::fs::File::open(format!("{}/header.sqe", path.display())),
            tokio::fs::File::open(format!("{}/composition.sqe", path.display()))
        );

        let header = Config::read(&mut header?.into_std().await, None, &Vec::new())?;
        let composition = Config::read(&mut composition?.into_std().await, None, &Vec::new())?;

        Ok(Composition {
            header,
            composition
        })
    }
}

#[instrument(err)]
async fn load_composition(composition_path: &PathBuf) -> Result<Composition> {
    info!("Loading composition at: {:?}", composition_path);
    Ok(Composition::from_path(composition_path).await?)
}

#[instrument(err)]
async fn create_missions(mission_settings: &MissionSettings, build_config: &BuildContext) -> Result<Vec<Mission>> {
    info!("Creating missions...");
    Ok(mission_settings.maps.iter().map(|map| Mission {
        map_name: map.clone(),
        mission_name: mission_settings.mission_name.clone(),
        prefix: build_config.prefix.clone()
    }).collect())
}

#[derive(Serialize)]
struct Mission {
    map_name: String,
    mission_name: String,
    prefix: String,
}

impl Mission {
    pub fn merge_composition(&mut self, composition: &Composition) {
    }

    /// Convert this mission to SQM
    pub fn to_sqm(&self) -> String {
        String::new()
    }

    /// Return the class_name for this mission
    pub fn class_name(&self) -> String {
        format!("{}.{}", self.mission_name(), self.map_name)
    }

    pub fn mission_name(&self) -> String {
        format!("{}_{}{}", self.prefix, self.map_name, self.mission_name,)
    }
}

#[derive(Serialize)]
struct Addon {
    prefix: String,
    addon_name: String,
    missions: Vec<MissionClass>,
}

impl Addon {
    pub fn from_parts(
        prefix: String,
        addon_name: String,
        missions: Vec<(PathBuf, String)>,
    ) -> Self {
        let missions = missions
            .into_iter()
            .map(|(directory, class_name)| {
                let directory = format!(
                    r"{}\{}\{}",
                    prefix,
                    addon_name,
                    directory
                        .parent()
                        .map(|p| p.to_string_lossy().to_owned())
                        .unwrap()
                );
                MissionClass {
                    briefing_name: format!("[{}] {}", prefix, class_name),
                    class_name,
                    directory,
                }
            })
            .collect();

        Addon {
            prefix,
            addon_name,
            missions,
        }
    }
}

#[derive(Serialize)]
struct MissionClass {
    class_name: String,
    briefing_name: String,
    directory: String,
}
