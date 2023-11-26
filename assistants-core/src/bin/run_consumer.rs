// assistants-core/src/bin/run-consumer.rs
// cargo run --package assistants-core --bin run_consumer

use assistants_core::assistant::queue_consumer;
use sqlx::postgres::PgPoolOptions;
use tokio;

#[tokio::main]
async fn main() {
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
    
    println!("Starting consumer");

    // Spawn the queue consumer as a separate async task
    // tokio::spawn(async move {
    loop {
        queue_consumer(&pool, &mut con).await;
    }
    // });
}

