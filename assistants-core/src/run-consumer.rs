// assistants-core/src/run-consumer.rs

use assistants_core::assistant::queue_consumer;


#[tokio::main]
async fn main() {
    // Set up your database connection pool
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to create pool.");

    // Spawn the queue consumer as a separate async task
    task::spawn(queue_consumer(pool));

    // Rest of your main function here
}

