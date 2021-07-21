use tokio::task::JoinHandle;
use std::path::PathBuf;
use std::{io, fs};
use crate::BuildContext;
use crate::Plugin;
use crate::Result;

pub struct AddonPlugin;

#[async_trait]
impl Plugin for AddonPlugin {
    async fn build(&self, build_context: BuildContext) -> Result<()> {
        copy_addons(build_context).await?;
        Ok(())
    }

    fn name(&self) -> String {
        "addons".to_string()
    }
}

#[instrument(err, skip(build_context))]
pub async fn copy_addons(
    build_context: BuildContext,
) -> Result<()> {
    copy_dir_all(build_context.addons_path.into(), format!("{}/{}", build_context.build_path, build_context.prefix).into())?;

    Ok(())
}


#[instrument(err)]
fn copy_dir_all(src: PathBuf, dst: PathBuf) -> tokio::io::Result<()> {
    debug!("Creating dir: {:?}", dst);

    fs::create_dir_all(&dst)?;

    let mut dir = fs::read_dir(src)?;

    while let Some(entry) = dir.next() {
            if let Ok(entry) = entry {
                // debug!("Copying: {:?}", entry);
                let ty = entry.file_type()?;
                if ty.is_dir() {
                    copy_dir_all(entry.path(), dst.join(entry.file_name()))?;
                } else {
                    fs::copy(entry.path(), dst.join(entry.file_name()))?;
                    debug!("Copied {:?}!", entry);
                }
            }
    }

    Ok(())
}
