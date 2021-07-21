//! Compiler Plugin for generating a Music addon from a folder of music.
//!
//! This plugin only works with .ogg files, and by-default will look in ./assets/music and any
//! subfolders for .ogg files
//!
//! If music is inside a subfolder, it will be catagorized by that subfolders name.
//!
//! This music will show up in the Zeus "Play Music" module, and will be prefixed by the prefix
//! defined in LAAT.toml

use crate::context::AddonManager;
use crate::Plugin;
use ogg_metadata::{read_format, OggFormat, AudioMetadata};
use std::path::Path;

use walkdir::DirEntry;
use std::path::PathBuf;
use crate::{Result, context::BuildContext, create_handlebars};

use serde::{Serialize, Deserialize};


const MUSIC_PATH: &str = r"data\Music";
const ADDON_NAME: &str = "Music";

pub struct MusicPlugin;

#[async_trait]
impl Plugin for MusicPlugin {
    async fn build(&self, build_context: BuildContext) -> Result<()> {
        build_music_addon(build_context).await
    }

    fn name(&self) -> String {
        "music".to_string()
    }
}


#[instrument(err, skip(build_context))]
pub async fn build_music_addon(
    build_context: BuildContext,
) -> Result<()> {
    let BuildContext {
        assets_path,
        prefix,
        ..
    } = build_context.clone();

    let mut manager = AddonManager::from_context(ADDON_NAME.to_string(), build_context);

    // Walkdir through ./assets/music
    let dir = walkdir::WalkDir::new(format!("{}/music", assets_path));

    let mut music_classes = Vec::new();
    let mut music_files = Vec::new();

    for entry in dir {
        match entry {
            Ok(entry) => {
                trace!("Walking entry: {}", entry.path().display());

                let file_type = entry.file_type();
                let file_name = entry.file_name().to_owned().to_string_lossy().to_string();

                // For each subfolder - create a CfgMusicClasses entry
                if file_type.is_dir() {
                    music_classes.push((file_name, entry.path().display().to_string()));
                // For each ogg file - create a CfgMusic entry which
                // references the CfgMusicClasses for it's folder.
                // Also - calculate the length (in seconds) of the ogg file, and add that into the template
                } else if file_type.is_file() && file_name.contains(".ogg") {
                    if let Ok(entry) = Track::from_dir_entry(entry, &prefix, &ADDON_NAME) {
                        music_files.push(entry);
                    }
                }
            }
            Err(why) => warn!("Error walking entry: {:?}", why),
        }
    }

    debug!("Files: {:?}", music_files);
    debug!("Classes: {:?}", music_classes);

    // Template a {prefix}_Music addon
    let music_addon = MusicAddon {
        addon_name: ADDON_NAME.to_string(),
        track_list: music_files.iter().map(|file| format!("\"{}\"", file.class_name)).collect::<Vec<_>>().join(", "),
        tracks: music_files.clone(),
        classes: music_classes.into_iter().map(|(class, _path)| {
            MusicClass {
                class_name: format!("{}{}", prefix, class),
                display_name: format!("[{}] {}", prefix, class),
            }
        }).collect(),
        prefix: prefix.clone(),
    };


    // Create the config.cpp
    let handlebars = create_handlebars()?;
    let config_cpp = handlebars.render("music_addon", &music_addon)?;

    manager.set_file(config_cpp, "config.cpp".into());

    // Copy the music files over
    for track in music_files {
        let new_path = manager.add_asset(track.path, Some("data/Music".into()))?;
    }

    manager.build_addon().await?;

    Ok(())
}


#[derive(Debug, Serialize, Deserialize)]
struct MusicAddon {
    prefix: String,
    addon_name: String,
    track_list: String,
    tracks: Vec<Track>,
    classes: Vec<MusicClass>,
}


#[derive(Debug, Serialize, Deserialize)]
struct MusicClass {
    class_name: String,
    display_name: String
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Track {
    class_name: String,
    file_name: String,
    pretty_name: String,
    duration: usize,
    music_class: String,
    path: PathBuf,
    sound_path: String
}

impl Track {
    pub fn from_dir_entry(entry: DirEntry, prefix: &str, addon_name: &str) -> Result<Self> {
        let file_name = entry.file_name().to_owned().to_string_lossy().to_string();

        let name = file_name.clone().split(".ogg").next().unwrap_or(&file_name).to_string();

        let class = entry
                .path()
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_owned()
                .to_string_lossy()
                .to_string();


        let music_file = Track {
            class_name: name.clone().replace("-", "_").replace(" ", "_"),
            pretty_name: name.replace("_", " "),
            duration: Self::get_duration_from_path(entry.path())?,
            path: entry.path().to_owned(),
            music_class: format!("{}{}", prefix, class),
            sound_path: format!(r"{}\{}\{}\{}", prefix, addon_name, MUSIC_PATH, file_name.clone()),
            file_name,
        };

        Ok(music_file)
    }

    #[instrument(err)]
    fn get_duration_from_path(path: &Path) -> Result<usize> {
        let file = std::fs::File::open(path)?;

        let formats = read_format(file)?;

        for format in formats {
            let duration = match format {
                OggFormat::Opus(metadata) => {
                    metadata.get_duration()
                },
                OggFormat::Vorbis(metadata) => {
                    metadata.get_duration()

                },
                _ => return Err("Unknown format".to_string().into())
            };

            if let Some(duration) = duration {
                return Ok(duration.as_secs() as usize);
            }
        }

        Err("Failed to get duration".to_string().into())
    }
}

