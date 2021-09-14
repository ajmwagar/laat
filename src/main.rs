use laat::InitSettings;
use laat::LaatCompiler;
use laat::ReleaseSettings;
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
    Build {
        /// Plugin to filter too
        plugin: Option<String>,
    },
    /// Convert addons to PBOs
    Pack {
        #[structopt(long)]
        /// Sign your PBOs after building
        sign: bool,
        #[structopt(long)]
        /// Build with windows filenames
        windows: bool,
    },
    /// Sign your PBOs
    Sign {},
    /// Publish your mod to the Steam Workshop
    Release(ReleaseSettings),
    /// Runs clean, build, pack, sign, and optionally release
    Ship {
        #[structopt(long)]
        /// Build with windows filenames
        windows: bool,
    },
}

#[tokio::main]
async fn main() {
    if let Err(why) = run().await {
        error!("{}", why);
        std::process::exit(1);
    }
}

fn setup_panic_hook() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        error!("PANIC: {}", info);
        default_panic(info);
        error!("FATAL! Exiting.");
        std::process::exit(1);
    }));
}

async fn run() -> laat::Result<()> {
    let opts = Opts::from_args();

    setup_panic_hook();
    init_logging(&opts)?;

    run_command(opts).await
}

fn init_logging(opts: &Opts) -> laat::Result<()> {
    // Set up logging
    let filter = if opts.debug {
        EnvFilter::new("laat=debug")
    } else {
        EnvFilter::new("laat=info")
    };

    if let Err(why) = tracing_subscriber::fmt().with_env_filter(filter).try_init() {
        return Err(format!("Failed to set up logger: {}", why).into());
    }

    Ok(())
}

async fn run_command(opts: Opts) -> laat::Result<()> {
    let laat = if let Command::Init(init) = &opts.command {
        LaatCompiler::init(init.clone()).await
    } else {
        LaatCompiler::from_path(opts.config_file).await
    }?;

    match opts.command {
        Command::Build { plugin } => {
            laat.build(plugin).await?;
        }
        Command::Clean {} => {
            laat.clean_build().await?;
        }
        Command::Pack { sign, windows } => {
            laat.pack(sign, windows).await?;
        }
        Command::Keygen { name } => {
            laat.create_keys(name).await?;
        }
        Command::Sign {} => {
            laat.sign().await?;
        }
        Command::Release(release) => {
            laat.release(release).await?;
        }
        Command::Ship { windows } => {
            laat.build(None).await?;
            laat.pack(true, windows).await?;
        }
        _ => {}
    }
    
    Ok(())
}
