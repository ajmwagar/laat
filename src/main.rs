use laat::LaatCompiler;
use std::path::PathBuf;
use structopt::StructOpt;
use tracing::error;
use tracing_subscriber::EnvFilter;

#[derive(Debug, StructOpt)]
struct Opts {
    #[structopt(subcommand)]
    command: Command,

    #[structopt(short = "f", parse(from_os_str), default_value = "LAAT.toml")]
    /// Point to your LAAT.toml file
    config_file: PathBuf,

    #[structopt(long)]
    debug: bool,
}

#[derive(Debug, StructOpt)]
enum Command {
    /// Run the LAAT compiler
    Build {},
    /// Clean the build folder
    Clean {},
    /// Using armake2 - convert the build folder into the final outputted mod.
    Pack {},
}

#[tokio::main]
async fn main() {
    if let Err(why) = run().await {
        error!("{}", why);
        std::process::exit(1);
    }
}

async fn run() -> laat::Result<()> {
    let opts = Opts::from_args();

    let filter = if opts.debug {
        EnvFilter::new("DEBUG")
    } else {
        EnvFilter::new("INFO")
    };

    if let Err(why) = tracing_subscriber::fmt().with_env_filter(filter).try_init() {
        return Err(format!("Failed to set up logger: {}", why).into());
    }

    match LaatCompiler::from_path(opts.config_file).await {
        Ok(laat) => {
            match opts.command {
                Command::Build {} => {
                    laat.build().await?;
                }
                Command::Clean {} => {
                    laat.clean_build().await?;
                }
                Command::Pack {} => {
                    laat.pack().await?;
                    // armake2 build 17th/Music Music.pbo -e 'prefix=17th\Music'
                }
            }
        }
        Err(err) => error!("{}", err),
    }

    Ok(())
}
