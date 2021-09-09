use std::path::Path;
use std::io::Read;
use armake2::config::ConfigArray;
use crate::context::AddonManager;
use crate::create_handlebars;
use crate::BuildContext;
use crate::Plugin;
use crate::Result;
use armake2::config::{Config, ConfigArrayElement, ConfigClass, ConfigEntry};
use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const MISSION_SETTINGS_KEY: &str = "missions";
const CBA_SETTINGS: &str = "cba_settings_hasSettingsFile = 1;";

const ON_PLAYER_DEATH: &str = r#"[player, [missionNamespace, "inventory_var"]] call BIS_fnc_saveInventory;"#;
const ON_PLAYER_RESPAWN: &str = r#"[player, [missionNamespace, "inventory_var"]] call BIS_fnc_loadInventory;"#;

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
            mission_settings.ignore_center
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


                // CBA settings
                if let Some(cba_settings_path) = &mission_settings.cba_settings_file  {
                    if let Err(why) = add_cba_settings(&cba_settings_path, &mut addon_manager, &mission) {
                        error!("Failed to add CBA Settings ({:?}) to addon: {}", &cba_settings_path, why);
                        return None;
                    }
                }

                // Keep inventory on spawn
                if mission_settings.respawn_keep_inventory {
                    keep_inventory_on_respawn(&mut addon_manager, &mission);
                }

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

#[instrument(err, skip(addon_manager, mission))]
fn add_cba_settings(cba_settings_path: &Path, addon_manager: &mut AddonManager, mission: &Mission) -> Result<()> {
    addon_manager.add_file(CBA_SETTINGS.to_string(), "description.ext".into());

    let mut cba_settings_string = String::new();
    let mut settings_file = std::fs::File::open(cba_settings_path)?;
    settings_file.read_to_string(&mut cba_settings_string)?;

    addon_manager.add_file(cba_settings_string, format!("missions/{}/cba_settings.sqf", mission.mission_name()).into());
    addon_manager.add_file(CBA_SETTINGS.to_string(), format!("missions/{}/description.ext", mission.mission_name()).into());

    Ok(())
}

fn keep_inventory_on_respawn(addon_manager: &mut AddonManager, mission: &Mission) {
    addon_manager.add_file(ON_PLAYER_RESPAWN.to_string(), format!("missions/{}/onPlayerRespawn.sqf", mission.mission_name()).into());
    addon_manager.add_file(ON_PLAYER_DEATH.to_string(), format!("missions/{}/onPlayerKilled.sqf", mission.mission_name()).into());
}

async fn add_mission_files(addon_manager: &mut AddonManager, mission_files_path: &PathBuf) -> Result<()> {

    Ok(())
}

type MapEntry = String;
type MapOffsetEntry = (String, (f32, f32, f32));

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum MapConfig {
    Map(MapEntry),
    MapOffset(MapOffsetEntry)
}

#[derive(Debug, Deserialize)]
struct MissionSettings {
    #[serde(default = "default_addon_name")]
    /// Name of the generated Addon
    addon_name: String,

    /// List of maps to create missions for
    maps: Vec<MapConfig>,

    /// Mission name
    #[serde(default = "default_mission_name")]
    mission_name: String,

    /// Delay, in seconds between death and when allowed to respawn.
    #[serde(default = "default_respawn_delay")]
    respawn_delay: usize,

    #[serde(default)]
    /// Keep inventory on respawn or not.
    respawn_keep_inventory: bool, 

    /// Composition to add to missions
    composition: PathBuf,

    #[serde(default)]
    /// X, Y, Z offset for the composition.
    composition_offset: (f32, f32, f32),

    ignore_center: bool,

    missions_folder: PathBuf,
    cba_settings_file: Option<PathBuf>
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
    offset: (f32, f32, f32),
    ignore_center: bool
}

impl Composition {
    #[instrument(err)]
    pub async fn from_path(path: &PathBuf, offset: (f32, f32, f32), ignore_center: bool) -> Result<Self> {
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
            ignore_center
        })
    }

    pub fn set_offset(&mut self, offset: (f32, f32, f32)) {
        self.offset = offset;
    }

    /// Get "center[]" from SQE, cast it into a tuple
    pub fn get_center(&self) -> Result<(f32, f32, f32)> {
        let config = self.composition.inner();

        if let Some(entries) = config.entries.clone() {
            let map: HashMap<String, ConfigEntry> = entries.into_iter().collect();

            if let Some(ConfigEntry::ArrayEntry(array)) = map.get("center") {
                debug!("Center Array: {:?}", array);

                let center = get_center_from_field(&array);

                if !self.ignore_center {
                    return Ok(center);
                }
                else {
                    return Ok((0., 0., 0.));
                }
            };
        }

        Err("Failed to get center[]".into())
    }

    pub fn get_offset(&self) -> Result<(f32, f32, f32)> {
        let (x1, y1, z1) = self.get_center()?;
        let (x2, y2, z2) = self.offset;

        Ok((x1 + x2, y1 + y2, z1 + z2))
    }

    /// Get and offset items from the SQE
    pub fn get_offseted_items(&self, offset_override: Option<(f32, f32, f32)>) -> Result<EntryList> {
        let offset = if let Some(offset_override) = offset_override {
            info!("Overriding offset...");
            offset_override
        }
        else {
            self.get_offset()?
        };

        let config = self.composition.inner();

        if let Some(entries) = config.entries.clone() {
            let map: HashMap<String, ConfigEntry> = entries.into_iter().collect();

            if let Some(ConfigEntry::ClassEntry(items)) = map.get("items") {
                if let Some(entries) = items.entries.clone() {
                    debug!("Item Classes: {}", entries.len());
                    return Ok(offset_classes(entries, offset));
                }
            };
        }

        Err("Failed to get offseted items".into())
    }
}

fn get_center_from_field(array: &ConfigArray) -> (f32, f32, f32) {
    let map_elem = |x: &ConfigArrayElement| {
        match x {
            ConfigArrayElement::FloatElement(x) => *x,
            ConfigArrayElement::IntElement(x) => *x as f32,
            _ => 0.
        }
    };

    let x = array.elements.get(0).map(map_elem).unwrap_or_default();
    let y = array.elements.get(1).map(map_elem).unwrap_or_default();
    let z = array.elements.get(2).map(map_elem).unwrap_or_default();

    (x, y ,z)
}

type EntryList = Vec<(String, ConfigEntry)>;

/// Offset classes recursively
#[instrument(skip(entries, composition_offset))]
fn offset_classes(entries: EntryList, composition_offset: (f32, f32, f32)) -> EntryList {
    let offsets = [
        composition_offset.0,
        composition_offset.1,
        composition_offset.2,
    ];

    entries
        .into_iter()
        .map(|(name, entry)| {
            let entry = if let ConfigEntry::ClassEntry(mut class) = entry {
                    // Offset
                    class.entries = class.entries.map(|mut entries| {
                        entries.iter_mut().find(|(name, _)| name == "position").map(
                            |(name, entry)| {
                                if let ConfigEntry::ArrayEntry(position) = entry {
                                    position.elements = position
                                        .elements
                                        .iter_mut()
                                        .enumerate()
                                        .map(|(idx, el)| add_to_element(el.clone(), offsets[idx]))
                                        .collect();

                                    (name, entry)
                                } else {
                                    (name, entry)
                                }
                            },
                        );

                        entries
                    });

                    // Recurse
                    class.entries = class
                        .entries
                        .map(|entries| offset_classes(entries, composition_offset));

                ConfigEntry::ClassEntry(class)
            } else {
                entry
            };

            (name, entry)
        })
        .collect()
}

fn add_to_element(element: ConfigArrayElement, increment: f32) -> ConfigArrayElement {
    match element {
        ConfigArrayElement::StringElement(_) => {}
        ConfigArrayElement::FloatElement(float) => {
            return ConfigArrayElement::FloatElement(float + increment);
        }
        ConfigArrayElement::IntElement(int) => {
            return ConfigArrayElement::FloatElement(int as f32 + increment);
        }
        ConfigArrayElement::ArrayElement(_) => {}
    }

    element
}

#[instrument(err)]
async fn load_composition(
    composition_path: &PathBuf,
    composition_offset: (f32, f32, f32),
    ignore_center: bool
) -> Result<Composition> {
    info!("Loading composition at: {:?}", composition_path);
    Ok(Composition::from_path(composition_path, composition_offset, ignore_center).await?)
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
    offset_override: Option<(f32, f32, f32)>,

    sqm: Config,
}

impl Mission {
    #[instrument(skip(mission_settings), err)]
    pub fn new(
        prefix: String,
        mission_name: String,
        map: MapConfig,
        mission_settings: &MissionSettings,
        build_config: &BuildContext,
    ) -> Result<Self> {
        let handlebars = create_handlebars()?;

        let (map_name, offset_override) = match map {
            MapConfig::Map(map_name) => (map_name, None),
            MapConfig::MapOffset((map_name, offset)) => (map_name, Some(offset)),
        };

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
            offset_override,
            mission_name,
            prefix,
            sqm: config,
        })
    }

    #[instrument(skip(self, composition))]
    pub fn merge_composition(&mut self, composition: &Composition) -> Result<()> {
        let items = composition.get_offseted_items(self.offset_override)?;

        let mut class = self.sqm.inner_mut();

        // Mission.Entities = items

        class.entries = class.entries.clone().map(|entries| {
            entries.into_iter().map(|(name, config)| {
                if name == "Mission" {
                    if let ConfigEntry::ClassEntry(mut mission) = config {
                        let parent = mission.parent.clone();

                        mission.entries = mission.entries.map(|entries| {
                            let mut map: HashMap<String, ConfigEntry> = entries.into_iter().collect();
                            let entities = ConfigEntry::ClassEntry(ConfigClass {
                                parent,
                                is_external: false,
                                is_deletion: false,
                                entries: Some(items.clone())
                            });

                            map.insert("Entities".to_string(), entities);

                            map.into_iter().collect()
                        });

                        return (name, ConfigEntry::ClassEntry(mission));
                    }
                }

                (name, config)
            }).collect()
        });

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
