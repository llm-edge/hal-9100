// assistants-core/src/run-consumer.rs

use crate::assistant::get_run_from_db;
use crate::assistant::update_run_in_db;
use crate::anthropic::call_anthropic_api;
use redis::AsyncCommands;
use std::collections::HashMap;

pub async fn queue_consumer(pool: &PgPool) {
    let client = redis::Client::open("redis://127.0.0.1/")?;
    let mut con = client.get_async_connection().await?;

    loop {
        let run_id: i32 = con.brpop("run_queue").await?;
        let run = get_run_from_db(pool, run_id).await?;
        let result = call_anthropic_api(run.instructions, 100, None, None, None, None, None, None).await?;
        update_run_in_db(pool, run_id, result.completion).await?;
    }
}


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

