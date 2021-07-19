use laat::LaatCompiler;
use std::path::PathBuf;
use structopt::StructOpt;
use tracing::error;

#[derive(Debug, StructOpt)]
struct Opts {
    #[structopt(subcommand)]
    command: Command,

    #[structopt(short = "f", parse(from_os_str), default_value = "LAAT.toml")]
    /// Point to your LAAT.toml file
    config_file: PathBuf
}

#[derive(Debug, StructOpt)]
enum Command {
    /// Run the LAAT compiler
    Build {

    },
    /// Clean the build folder
    Clean {

    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    if let Err(why) = run().await {
        error!("{}", why);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let opts = Opts::from_args();

    match LaatCompiler::from_path(opts.config_file).await {
        Ok(laat) => {
            match opts.command {
                Command::Build {} => {
                    laat.build().await?;
                },
                Command::Clean {} => {
                    laat.clean().await?;
                }
            }

        },
        Err(err) => error!("{}", err),
    }

    Ok(())
}
