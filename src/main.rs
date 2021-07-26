use laat::ReleaseSettings;
use laat::InitSettings;
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
    /// Create a new LAAT project
    Init(InitSettings),
    /// Create a new key pair
    Keygen {
        #[structopt(parse(from_os_str))]
        name: PathBuf,
    },
    /// Clean the build folder
    Clean {},
    /// Generate addons
    Build {},
    /// Convert addons to PBOs
    Pack {
        #[structopt(long)]
        /// Sign your PBOs after building
        sign: bool
    },
    /// Sign your PBOs
    Sign {},
    /// Publish your mod to the Steam Workshop
    Release(ReleaseSettings),
    /// Runs clean, build, pack, sign, and optionally release
    Ship {
        #[structopt(long)]
        /// Uploaded outputted mod to the Steam Workshop
        release: bool
    }
}

#[tokio::main]
async fn main() {
    if let Err(why) = run().await {
        error!("{}", why);
        std::process::exit(1);
    }
}

async fn run() -> laat::Result<()> {
    // Parse CLI Args
    let opts = Opts::from_args();

    // Set up logging
    let filter = if opts.debug {
        EnvFilter::new("DEBUG")
    } else {
        EnvFilter::new("INFO")
    };

    if let Err(why) = tracing_subscriber::fmt().with_env_filter(filter).try_init() {
        return Err(format!("Failed to set up logger: {}", why).into());
    }

    // Create LAAT Context
    let laat = if let Command::Init(init) = &opts.command {
        LaatCompiler::init(init.clone()).await
    } else {
        LaatCompiler::from_path(opts.config_file).await
    }?;

    // Run Command
    match opts.command {
        Command::Build {} => {
            laat.build().await?;
        }
        Command::Clean {} => {
            laat.clean_build().await?;
        }
        Command::Pack { sign } => {
            laat.pack(sign).await?;
        }
        Command::Keygen { name } => {
            laat.create_keys(name).await?;
        }
        Command::Sign {} => {
            laat.sign().await?;
        }
        _ => {}
    }

    Ok(())
}
