// init
// docker run --name pg -e POSTGRES_PASSWORD=secret -d -p 5432:5432 postgres
// docker exec -it pg psql -U postgres -c "CREATE DATABASE mydatabase;"

// migrations
// docker exec -i pg psql -U postgres -d mydatabase < assistants-core/src/migrations.sql

// checks
// docker exec -it pg psql -U postgres -d mydatabase -c "\dt"


use sqlx::PgPool;

pub struct Assistant {
    pub instructions: String,
    pub name: String,
    pub tools: Vec<String>,
    pub model: String,
    pub user_id: String,
}

pub async fn create_assistant(pool: &PgPool, assistant: &Assistant) -> Result<(), sqlx::Error> {
    // Convert Vec<&str> to Vec<String>
    let tools: Vec<String> = assistant.tools.iter().map(|s| s.to_string()).collect();
    sqlx::query!(
        r#"
        INSERT INTO assistants (instructions, name, tools, model, user_id)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        &assistant.instructions, &assistant.name, &tools, &assistant.model, &assistant.user_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn create_thread(pool: &PgPool, user_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO threads (user_id)
        VALUES ($1)
        "#,
        &user_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn add_message_to_thread(pool: &PgPool, thread_id: &str, role: &str, content: &str) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO messages (thread_id, role, content)
        VALUES ($1, $2, $3)
        "#,
        &thread_id, &role, &content
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn run_assistant(pool: &PgPool, thread_id: &str, assistant_id: &str, instructions: &str) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO runs (thread_id, assistant_id, instructions)
        VALUES ($1, $2, $3)
        "#,
        &thread_id, &assistant_id, &instructions
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn check_run_status(pool: &PgPool, run_id: &str) -> Result<(), sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT status FROM runs WHERE id = $1
        "#,
        &run_id
    )
    .fetch_one(pool)
    .await?;
    Ok(row.status)
}

pub async fn display_assistant_response(pool: &PgPool, thread_id: &str, user_id: &str) -> Result<(), sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT content FROM messages WHERE thread_id = $1 AND user_id = $2 AND role = 'assistant'
        "#,
        &thread_id, &user_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dotenv::dotenv;

    #[tokio::test]
    async fn test_database_connection() {
        dotenv().ok();
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let connection = PgPool::connect(&database_url).await;
        assert!(connection.is_ok(), "Database connection failed");
    }
}

