use crate::Plugin;
use std::io::Write;
use ogg_metadata::{read_format, OggFormat, AudioMetadata};
use std::path::Path;
use std::error::Error;
use walkdir::DirEntry;
use std::path::PathBuf;
use crate::{context::BuildContext, create_handlebars};

use serde::{Serialize, Deserialize};


const MUSIC_PATH: &str = r"data\Music";
const ADDON_NAME: &str = "Music";

pub struct MusicPlugin;

#[async_trait]
impl Plugin for MusicPlugin {
    async fn build(&self, build_context: BuildContext) -> Result<(), Box<dyn Error>> {
        build_music_addon(build_context).await
    }
}


#[instrument(err, skip(build_context))]
pub async fn build_music_addon(
    build_context: BuildContext,
) -> Result<(), Box<dyn std::error::Error>> {
    let BuildContext {
        assets_path,
        prefix,
        build_path,
        ..
    } = build_context;

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


    let addon_path = format!("{}/{}/{}", build_path, &prefix, ADDON_NAME.to_string());

    // Create the music addon file structure, and then copy the music files over.
    std::fs::create_dir_all(&addon_path)?;
    std::fs::create_dir_all(format!("{}/data/Music", addon_path))?;

    // Create the config.cpp
    let handlebars = create_handlebars()?;
    let rendered = handlebars.render("music_addon", &music_addon)?;
    let mut file = std::fs::File::create(format!("{}/config.cpp", addon_path))?;
    file.write_fmt(format_args!("{}", rendered))?;

    // Copy the music files over
    for track in music_files {
        std::fs::copy(track.path, format!("{}/data/Music/{}", addon_path, track.file_name))?;
    }

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
    pub fn from_dir_entry(entry: DirEntry, prefix: &str, addon_name: &str) -> Result<Self, Box<dyn Error>> {
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
    fn get_duration_from_path(path: &Path) -> Result<usize, Box<dyn Error>> {
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

