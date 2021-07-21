use tokio::task::JoinHandle;
use std::path::PathBuf;
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
    copy_dir_all(build_context.addons_path.into(), format!("{}/{}", build_context.build_path, build_context.prefix).into()).await?;

    Ok(())
}


#[async_recursion]
async fn copy_dir_all(src: PathBuf, dst: PathBuf) -> tokio::io::Result<()> {
    tokio::fs::create_dir_all(&dst).await?;
    let mut dir = tokio::fs::read_dir(src).await?;

    let mut futs = Vec::new();

    while let Ok(entry) = dir.next_entry().await {
        let dst = dst.clone();
        let fut: JoinHandle<Result<()>> = tokio::spawn(async move {
            if let Some(entry) = entry {
                let ty = entry.file_type().await?;
                if ty.is_dir() {
                    copy_dir_all(entry.path(), dst.join(entry.file_name())).await?;
                } else {
                    tokio::fs::copy(entry.path(), dst.join(entry.file_name())).await?;
                }

                Ok(())
            }
            else {
                Err(format!("Failed to get entry.").into())
            }
        });

        futs.push(fut);
    }

    futures_util::future::try_join_all(futs).await?;

    Ok(())
}
