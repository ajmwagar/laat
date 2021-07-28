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
        let composition = load_composition(
            &mission_settings.composition,
            mission_settings.composition_offset,
        )
        .await?;

        // For each Map create mission based on settings.
        let mut missions = create_missions(&mission_settings, &build_config).await?;

        // Merge composition into mission
        missions.iter_mut().for_each(|mission| {
            if let Err(why) = mission.merge_composition(&composition) {
                warn!("Failed to merge composition: {}", why);
            }
        });

        // Save mission to addon
        let mut addon_manager =
            AddonManager::from_context(&mission_settings.addon_name, build_config.clone());

        let classes = missions
            .into_iter()
            .filter_map(|mission| {
                let path: PathBuf =
                    format!("missions/{}/mission.sqm", mission.mission_name()).into();

                let sqm = match mission.to_sqm() {
                    Ok(sqm) => sqm,
                    Err(err) => {
                        warn!("Error creating sqm: {}", err);
                        return None;
                    }
                };

                addon_manager.add_file(sqm, path.clone());

                Some((path, mission))
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
    offset: (f64, f64, f64),
}

impl Composition {
    #[instrument(err)]
    pub async fn from_path(path: &PathBuf, offset: (f64, f64, f64)) -> Result<Self> {
        let (header, composition) = tokio::join!(
            tokio::fs::File::open(format!("{}/header.sqe", path.display())),
            tokio::fs::File::open(format!("{}/composition.sqe", path.display()))
        );

        let header = Config::read(&mut header?.into_std().await, None, &Vec::new())?;
        let composition = Config::read(&mut composition?.into_std().await, None, &Vec::new())?;

        Ok(Composition {
            header,
            composition,
            offset,
        })
    }

    pub fn get_center(&self) -> (f64, f64, f64) {
        (0., 0., 0.)
    }
}

#[instrument(err)]
async fn load_composition(
    composition_path: &PathBuf,
    composition_offset: (f64, f64, f64),
) -> Result<Composition> {
    info!("Loading composition at: {:?}", composition_path);
    Ok(Composition::from_path(composition_path, composition_offset).await?)
}

#[instrument(err)]
async fn create_missions(
    mission_settings: &MissionSettings,
    build_config: &BuildContext,
) -> Result<Vec<Mission>> {
    info!("Creating missions...");
    Ok(mission_settings
        .maps
        .iter()
        .filter_map(|map| {
            Mission::new(
                build_config.prefix.clone(),
                mission_settings.mission_name.clone(),
                map.clone(),
                &mission_settings,
                &build_config,
            )
            .ok()
        })
        .collect())
}

struct Mission {
    map_name: String,
    mission_name: String,
    prefix: String,

    sqm: Config,
}

impl Mission {
    #[instrument(skip(mission_settings), err)]
    pub fn new(
        prefix: String,
        mission_name: String,
        map_name: String,
        mission_settings: &MissionSettings,
        build_config: &BuildContext,
    ) -> Result<Self> {
        let handlebars = create_handlebars()?;

        #[derive(Serialize)]
        struct MissionTemplate {
            author: String,
            respawn_delay: usize,
            mission_name: String,
        }

        let template = MissionTemplate {
            author: build_config
                .extra
                .get("author")
                .map(|v| v.to_string())
                .unwrap_or_default(),
            mission_name: mission_name.clone(),
            respawn_delay: mission_settings.respawn_delay,
        };

        let sqm = handlebars.render("mission.sqm", &template)?;

        let config = Config::read(&mut sqm.as_bytes(), None, &Vec::new())?;

        Ok(Mission {
            map_name,
            mission_name,
            prefix,
            sqm: config,
        })
    }

    #[instrument(skip(self, composition))]
    pub fn merge_composition(&mut self, composition: &Composition) -> Result<()> {
        let center: (f64, f64, f64) = composition.get_center();

        Ok(())
    }

    /// Convert this mission to SQM
    pub fn to_sqm(&self) -> Result<String> {
        let mut buffer = Vec::new();
        self.sqm.write(&mut buffer)?;

        Ok(std::str::from_utf8(&buffer)?.to_string())
    }

    /// Return the class_name for this mission
    pub fn mission_name(&self) -> String {
        format!("{}.{}", self.class_name(), self.map_name)
    }

    pub fn class_name(&self) -> String {
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
        missions: Vec<(PathBuf, Mission)>,
    ) -> Self {
        let missions = missions
            .into_iter()
            .map(|(directory, mission)| {
                let directory = format!(
                    r"{}\{}\missions\{}",
                    prefix,
                    addon_name,
                    directory
                        .parent()
                        .map(|p| p.file_name().map(|p| p.to_string_lossy().to_owned()))
                        .flatten()
                        .unwrap()
                );
                MissionClass {
                    briefing_name: format!("[{}] {}", prefix, mission.class_name()),
                    class_name: mission.class_name(),
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
