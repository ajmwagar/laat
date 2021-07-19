use std::{io, fs};
use std::path::Path;
use std::error::Error;
use crate::BuildContext;
use crate::Plugin;

pub struct AddonPlugin;

#[async_trait]
impl Plugin for AddonPlugin {
    async fn build(&self, build_context: BuildContext) -> Result<(), Box<dyn Error>> {
        copy_addons(build_context).await?;
        Ok(())
    }
}

#[instrument(err, skip(build_context))]
pub async fn copy_addons(
    build_context: BuildContext,
) -> Result<(), Box<dyn Error>> {
    copy_dir_all(build_context.addons_path, format!("{}/{}", build_context.build_path, build_context.prefix))?;

    Ok(())
}


fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}
