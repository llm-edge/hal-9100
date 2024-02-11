// hal-9100-core/src/bin/run-consumer.rs
// set all env var in the terminal:
// export $(cat .env | xargs)
// run the consumer:
// cargo run --package hal-9100-core --bin run_consumer

use hal_9100_core::executor::{loop_through_runs, try_run_executor};
use env_logger;
use log::{error, info};
use sqlx::postgres::PgPoolOptions;
use tokio;

#[tokio::main]
async fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    // Set up your database connection pool
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to create pool.");
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
    let client = redis::Client::open(redis_url).unwrap();
    let mut con = client.get_async_connection().await.unwrap();

    info!("Starting consumer");

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

    info!("{}", &ascii_art);

    loop_through_runs(&pool, &mut con).await;
}
