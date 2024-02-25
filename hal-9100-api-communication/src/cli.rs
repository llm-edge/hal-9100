use clap::{ArgAction, Parser, Subcommand};
use dotenv::dotenv;
use hal_9100_api_communication::{models::AppState, routes::router::app};
use hal_9100_core::{executor::loop_through_runs, file_storage::FileStorage};
use hal_9100_extra::{config::Hal9100Config, llm::HalLLMClient};
use log::{error, info};
use sqlx::postgres::PgPoolOptions;
use std::{net::SocketAddr, num::NonZeroU16, path::PathBuf, sync::Arc, time::Duration};

#[derive(Parser, Debug)]
#[command(
    name = "hal-9100",
    about = "The HAL-9100 CLI",
    version,
    rename_all = "kebab-case"
)]
pub struct RootOpts {
    /// Read configuration from one file.
    /// File format is detected from the file name.
    /// If zero file are specified, the deprecated default config path
    /// `/etc/hal-9100/hal-9100.toml` is targeted.
    #[arg(id = "config", short, long, env = "HAL_9100_CONFIG")]
    pub config_path: Option<PathBuf>,

    /// Enable more detailed internal logging. Repeat to increase level. Overridden by `--quiet`.
    #[arg(short, long, action = ArgAction::Count)]
    pub verbose: u8,

    /// Reduce detail of internal logging. Repeat to reduce further. Overrides `--verbose`.
    #[arg(short, long, action = ArgAction::Count)]
    pub quiet: u8,

    /// Watch for changes in configuration file, and reload accordingly.
    #[arg(short, long, env = "HAL_9100_WATCH_CONFIG")]
    pub watch_config: bool,

    /// Port to listen on.
    #[arg(short, long, env = "PORT", default_value = "3000")]
    pub port: Option<NonZeroU16>, // TODO: move to api config

    /// Executor mode
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Starts the HTTP server
    Api,
    /// Listens to the Redis queue
    Executor,
}

impl RootOpts {
    fn new() -> Self {
        dotenv().ok(); // Load .env file if it exists

        RootOpts::parse()
    }
}

#[tokio::main]
async fn main() {
    let opts = RootOpts::new();

    // Read configuration from file(s)
    let config_path = if opts.config_path.is_some() {
        opts.config_path.unwrap()
    } else {
        PathBuf::from("./hal-9100.toml")
    };
    // Load configuration and override with environment variables
    let config = Hal9100Config::load_and_override_with_env(config_path).await;

    env_logger::builder()
        .filter_level(match opts.verbose - opts.quiet {
            0 => log::LevelFilter::Info,
            1 => log::LevelFilter::Debug,
            2 => log::LevelFilter::Trace,
            _ => log::LevelFilter::Error,
        })
        .init();

    // set up connection pool
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .idle_timeout(Duration::from_secs(3))
        .connect(&config.database_url.clone())
        .await
        .expect("can't connect to database");

    let ascii_art = r"
               ___           ___           ___              
              /\__\         /\  \         /\__\             
             /:/  /        /::\  \       /:/  /             
            /:/__/        /:/\:\  \     /:/  /              
           /::\  \ ___   /::\~\:\  \   /:/  /               
          /:/\:\  /\__\ /:/\:\ \:\__\ /:/__/                
          \/__\:\/:/  / \/__\:\/:/  / \:\  \                
               \::/  /       \::/  /   \:\  \               
               /:/  /        /:/  /     \:\  \              
              /:/  /        /:/  /       \:\__\             
              \/__/         \/__/         \/__/             
      ___                    ___                    ___     
     /\  \                  /\  \                  /\  \    
     \:\  \                 \:\  \                 \:\  \   
      \:\  \                 \:\  \                 \:\  \  
      /::\  \                /::\  \                /::\  \ 
     /:/\:\__\              /:/\:\__\              /:/\:\__\
    /:/  \/__/             /:/  \/__/             /:/  \/__/
   /:/  /                 /:/  /                 /:/  /     
   \/__/                  \/__/                  \/__/      
                                                            
    ";

    info!("{}", ascii_art);
    match opts.command {
        Commands::Api => {
            let app_state = AppState {
                hal_9100_config: Arc::new(config),
                pool: Arc::new(pool),
                file_storage: Arc::new(FileStorage::new().await),
            };

            let app = app(app_state);
            let port = opts.port.unwrap().get();
            let addr = SocketAddr::from(([0, 0, 0, 0], port));
            info!("Starting HAL-9100 on {}", addr);

            let server = axum::Server::bind(&addr).serve(app.into_make_service());
            let graceful_shutdown = server.with_graceful_shutdown(shutdown_signal());
            if let Err(e) = graceful_shutdown.await {
                error!("server error: {}", e);
            }
        }
        Commands::Executor => {
            let redis_url = config.redis_url.clone();
            let client = redis::Client::open(redis_url).unwrap();
            let mut con = client.get_async_connection().await.unwrap();

            info!("Starting hal-9100-executor");
            let llm_client = HalLLMClient::new(
                "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string(),
                config.model_url,
                config.model_api_key.unwrap_or_default(),
            );
            loop_through_runs(&pool, &mut con, llm_client).await;
        }
    }
}

async fn shutdown_signal() {
    // Wait for the SIGINT or SIGTERM signal
    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .unwrap()
        .recv()
        .await
        .unwrap();

    info!("signal received, starting graceful shutdown");
}
