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

    /// Map containing ./assets paths and maps them to Addon prefixed paths.
    asset_map: HashMap<PathBuf, PathBuf>,

    file_map: HashMap<PathBuf, String>,
}

impl AddonManager {
    /// Create a new Addon from an addon name and build_context
    pub fn from_context(addon: impl Into<String>, build_context: BuildContext) -> Self {
        Self {
            addon: addon.into(),
            build_context,
            asset_map: HashMap::new(),
            file_map: HashMap::new(),
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
    pub fn add_asset(
        &mut self,
        asset_path: PathBuf,
        addon_folder: Option<PathBuf>,
    ) -> Result<PathBuf> {
        let mut addon_path = PathBuf::new();
        addon_path.push(self.addon_path()); // 17th/{addon}

        if let Some(folder) = addon_folder {
            addon_path.push(folder);
        }

        if let Some(file) = asset_path.file_name() {
            addon_path.push(file); // texture.ogg
        } else {
            return Err(format!("Failed to get file name for: {:?}", asset_path).into());
        }

        self.asset_map.insert(asset_path, addon_path.clone());

        Ok(addon_path)
    }

    /// Iterates over the loaded assets, and copies them to their destined module paths. This
    /// will also create the addon folder if it doesn't already exists.
    #[instrument(err, skip(self))]
    async fn copy_assets(&self) -> Result<()> {
        let mut futs = Vec::new();

        for (asset, addon_path) in self.asset_map.clone().into_iter() {
            debug!("Copying {} > {}", asset.display(), addon_path.display());
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

    /// Set the value to write to target file
    pub fn add_file(&mut self, buffer: String, path: PathBuf) {
        let mut file_path = PathBuf::new();
        file_path.push(self.addon_path());
        file_path.push(path);

        self.file_map.insert(file_path, buffer);
    }

    #[instrument(err, skip(self))]
    async fn write_files(&self) -> Result<()> {
        for (path, string) in self.file_map.clone().into_iter() {
            debug!("Writing file: {}", path.display());
            let mut file_path = self.build_path();
            file_path.push(path);

            if let Some(parent) = file_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            let mut file = tokio::fs::File::create(file_path).await?;
            file.write_all(string.as_bytes()).await?;
        }

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

        tokio::try_join!(self.write_files(), self.copy_assets())?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PackConfig;
    use crate::config::ReleaseConfig;
    use toml::Value;

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
                header_extensions: vec![],
            },
            extra: Value::Float(0.0),
            keys_path: "keys".to_string(),
            release: ReleaseConfig {
                app_id: 0,
                workshop_id: 0,
            },
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
}
