

use sqlx::PgPool;
use serde_json;
use redis::AsyncCommands;
use log::{info, error};

use assistants_extra::anthropic::call_anthropic_api;
use assistants_core::file_storage::FileStorage;
use assistants_core::models::{Run, Thread, Assistant, Content, Text, Message, AnthropicApiError};


pub async fn list_messages(pool: &PgPool, thread_id: i32) -> Result<Vec<Message>, sqlx::Error> {
    info!("Listing messages for thread_id: {}", thread_id);
    let messages = sqlx::query!(
        r#"
        SELECT id, created_at, thread_id, role, content::jsonb, assistant_id, run_id, file_ids, metadata, user_id, object FROM messages WHERE thread_id = $1
        "#,
        &thread_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        // let content: Vec<Content> = serde_json::from_value(row.content.clone()).unwrap_or_default();
        Message {
            id: row.id,
            created_at: row.created_at,
            thread_id: row.thread_id.unwrap_or_default(),
            role: row.role,
            content: serde_json::from_value(row.content).unwrap_or_default(),
            assistant_id: row.assistant_id,
            run_id: row.run_id,
            file_ids: row.file_ids,
            metadata: row.metadata.map(|v| v.as_object().unwrap().clone().into_iter().map(|(k, v)| (k, v.as_str().unwrap().to_string())).collect()),
            user_id: row.user_id.unwrap_or_default(),
            object: row.object.unwrap_or_default(),
        }
    })
    .collect();
    Ok(messages)
}


pub async fn create_assistant(pool: &PgPool, assistant: &Assistant) -> Result<Assistant, sqlx::Error> {
    info!("Creating assistant: {:?}", assistant);
    let tools: Vec<String> = assistant.tools.iter().map(|s| s.to_string()).collect();
    let file_ids: Option<Vec<String>> = match &assistant.file_ids {
        Some(file_ids) => Some(file_ids.iter().map(|s| s.to_string()).collect()),
        None => None,
    };
    let file_ids_ref: Option<&[String]> = file_ids.as_ref().map(|v| v.as_slice());
    let row = sqlx::query!(
        r#"
        INSERT INTO assistants (instructions, name, tools, model, user_id, file_ids)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING *
        "#,
        &assistant.instructions.clone().unwrap_or_default(), &assistant.name.clone().unwrap_or_default(), &tools, &assistant.model, &assistant.user_id.to_string(), file_ids_ref
    )
    .fetch_one(pool)
    .await?;
    Ok(Assistant {
        id: row.id,
        instructions: row.instructions,
        name: row.name,
        tools: row.tools.unwrap_or_default(),
        model: row.model.unwrap_or_default(),
        user_id: row.user_id.unwrap_or_default(),
        file_ids: row.file_ids,
        object: row.object.unwrap_or_default(),
        created_at: row.created_at,
        description: row.description,
        metadata: row.metadata.map(|v| v.as_object().unwrap().clone().into_iter().map(|(k, v)| (k, v.as_str().unwrap().to_string())).collect()),
    })
}

pub async fn create_thread(pool: &PgPool, user_id: &str) -> Result<Thread, sqlx::Error> {
    info!("Creating thread for user_id: {}", user_id);
    let row = sqlx::query!(
        r#"
        INSERT INTO threads (user_id)
        VALUES ($1)
        RETURNING *
        "#,
        &user_id
    )
    .fetch_one(pool)
    .await?;

    Ok(Thread {
        id: row.id,
        user_id: row.user_id.unwrap_or_default(),
        file_ids: row.file_ids.map(|v| v.iter().map(|s| s.to_string()).collect()), // existing code
        object: row.object.unwrap_or_default(), // add this
        created_at: row.created_at, // and this
        metadata: row.metadata.map(|v| v.as_object().unwrap().clone().into_iter().map(|(k, v)| (k, v.as_str().unwrap().to_string())).collect()), // and this
    })
}
pub async fn add_message_to_thread(pool: &PgPool, thread_id: i32, role: &str, content: Vec<Content>, user_id: &str, file_ids: Option<Vec<String>>) -> Result<Message, sqlx::Error> {
    info!("Adding message to thread_id: {}, role: {}, user_id: {}", thread_id, role, user_id);
    let content_json = match serde_json::to_string(&content) {
        Ok(json) => json,
        Err(e) => return Err(sqlx::Error::Configuration(e.into())),
    };
    let content_value: serde_json::Value = serde_json::from_str(&content_json).unwrap();
    let file_ids: Option<Vec<String>> = match file_ids {
        Some(file_ids) => Some(file_ids),
        None => None,
    };
    let file_ids_ref: Option<&[String]> = file_ids.as_ref().map(|v| v.as_slice());
    let row = sqlx::query!(
        r#"
        INSERT INTO messages (thread_id, role, content, user_id, file_ids)
        VALUES ($1, $2, to_jsonb($3::jsonb), $4, $5)
        RETURNING *
        "#,
        &thread_id, &role, &content_value, &user_id, file_ids_ref
    )
    .fetch_one(pool)
    .await?;
    Ok(Message {
        id: row.id,
        created_at: row.created_at,
        thread_id: row.thread_id.unwrap_or_default(),
        role: row.role,
        content: serde_json::from_value(row.content).unwrap_or_default(),
        assistant_id: row.assistant_id,
        run_id: row.run_id,
        file_ids: row.file_ids,
        metadata: row.metadata.map(|v| v.as_object().unwrap().clone().into_iter().map(|(k, v)| (k, v.as_str().unwrap().to_string())).collect()),
        user_id: row.user_id.unwrap_or_default(),
        object: row.object.unwrap_or_default(),
    })
}

pub async fn run_assistant(pool: &PgPool, thread_id: i32, assistant_id: i32, instructions: &str, mut con: redis::aio::Connection) -> Result<Run, sqlx::Error> {
    info!("Running assistant_id: {} for thread_id: {}", assistant_id, thread_id);
    // Create Run in database
    let run = match create_run_in_db(pool, thread_id, assistant_id, instructions).await {
        Ok(run) => run,
        Err(e) => {
            eprintln!("Failed to create run in database: {}", e);
            return Err(e);
        }
    };

    // Add run_id to Redis queue
    con.lpush("run_queue", run.id).await.map_err(|e| sqlx::Error::Configuration(e.into()))?;

    // Set run status to "queued" in database
    let updated_run = update_run_in_db(pool, run.id, "queued".to_string()).await?;

    Ok(updated_run)
}

async fn create_run_in_db(pool: &PgPool, thread_id: i32, assistant_id: i32, instructions: &str) -> Result<Run, sqlx::Error> {
    info!("Creating run in database for thread_id: {}, assistant_id: {}", thread_id, assistant_id);
    let row = sqlx::query!(
        r#"
        INSERT INTO runs (thread_id, assistant_id, instructions)
        VALUES ($1, $2, $3)
        RETURNING *
        "#,
        &thread_id, &assistant_id, &instructions
    )
    .fetch_one(pool)
    .await?;
    Ok(Run {
        id: row.id,
        thread_id: row.thread_id.unwrap_or_default(),
        assistant_id: row.assistant_id.unwrap_or_default(),
        status: row.status.unwrap_or_default(),
        instructions: row.instructions.unwrap_or_default(),
        user_id: row.user_id.unwrap_or_default(),
        object: row.object.unwrap_or_default(),
        created_at: row.created_at,
        required_action: serde_json::from_value(row.required_action.unwrap_or_else(|| serde_json::Value::Null)).unwrap_or_default(),
        last_error: serde_json::from_value(row.last_error.unwrap_or_else(|| serde_json::Value::Null)).unwrap_or_default(),
        expires_at: row.expires_at.unwrap_or_default(),
        started_at: row.started_at,
        cancelled_at: row.cancelled_at,
        failed_at: row.failed_at,
        completed_at: row.completed_at,
        model: row.model.unwrap_or_default(),
        tools: row.tools.unwrap_or_default(),
        file_ids: row.file_ids.unwrap_or_default(),
        metadata: row.metadata.map(|v| v.as_object().unwrap().clone().into_iter().map(|(k, v)| (k, v.as_str().unwrap().to_string())).collect()),
    })
}

pub async fn get_run_from_db(pool: &PgPool, run_id: i32) -> Result<Run, sqlx::Error> {
    info!("Getting run from database for run_id: {}", run_id);
    let row = sqlx::query!(
        r#"
        SELECT * FROM runs WHERE id = $1
        "#,
        &run_id
    )
    .fetch_one(pool)
    .await?;

    Ok(Run {
        id: row.id,
        thread_id: row.thread_id.unwrap_or_default(),
        assistant_id: row.assistant_id.unwrap_or_default(),
        status: row.status.unwrap_or_default(),
        instructions: row.instructions.unwrap_or_default(),
        user_id: row.user_id.unwrap_or_default(),
        object: row.object.unwrap_or_default(),
        created_at: row.created_at,
        required_action: serde_json::from_value(row.required_action.unwrap_or_else(|| serde_json::Value::Null)).unwrap_or_default(),
        last_error: serde_json::from_value(row.last_error.unwrap_or_else(|| serde_json::Value::Null)).unwrap_or_default(),
        expires_at: row.expires_at.unwrap_or_default(),
        started_at: row.started_at,
        cancelled_at: row.cancelled_at,
        failed_at: row.failed_at,
        completed_at: row.completed_at,
        model: row.model.unwrap_or_default(),
        tools: row.tools.unwrap_or_default(),
        file_ids: row.file_ids.unwrap_or_default(),
        metadata: row.metadata.map(|v| v.as_object().unwrap().clone().into_iter().map(|(k, v)| (k, v.as_str().unwrap().to_string())).collect()),
    })
}

async fn update_run_in_db(pool: &PgPool, run_id: i32, completion: String) -> Result<Run, sqlx::Error> {
    info!("Updating run in database for run_id: {}", run_id);
    let row = sqlx::query!(
        r#"
        UPDATE runs SET status = $1 WHERE id = $2
        RETURNING *
        "#,
        &completion, &run_id
    )
    .fetch_one(pool)
    .await?;
    Ok(Run {
        id: row.id,
        thread_id: row.thread_id.unwrap_or_default(),
        assistant_id: row.assistant_id.unwrap_or_default(),
        status: row.status.unwrap_or_default(),
        instructions: row.instructions.unwrap_or_default(),
        user_id: row.user_id.unwrap_or_default(),
        object: row.object.unwrap_or_default(),
        created_at: row.created_at,
        required_action: serde_json::from_value(row.required_action.unwrap_or_else(|| serde_json::Value::Null)).unwrap_or_default(),
        last_error: serde_json::from_value(row.last_error.unwrap_or_else(|| serde_json::Value::Null)).unwrap_or_default(),
        expires_at: row.expires_at.unwrap_or_default(),
        started_at: row.started_at,
        cancelled_at: row.cancelled_at,
        failed_at: row.failed_at,
        completed_at: row.completed_at,
        model: row.model.unwrap_or_default(),
        tools: row.tools.unwrap_or_default(),
        file_ids: row.file_ids.unwrap_or_default(),
        metadata: row.metadata.map(|v| v.as_object().unwrap().clone().into_iter().map(|(k, v)| (k, v.as_str().unwrap().to_string())).collect()),
    })
}



async fn get_thread_from_db(pool: &PgPool, thread_id: i32) -> Result<Thread, sqlx::Error> {
    info!("Getting thread from database for thread_id: {}", thread_id);
    let row = sqlx::query!(
        r#"
        SELECT * FROM threads WHERE id = $1
        "#,
        &thread_id
    )
    .fetch_one(pool)
    .await?;

    Ok(Thread {
        id: row.id,
        user_id: row.user_id.unwrap_or_default(),
        file_ids: row.file_ids.map(|v| v.iter().map(|s| s.to_string()).collect()), // If file_ids is None, use an empty vector
        object: row.object.unwrap_or_default(), // add this
        created_at: row.created_at, // and this
        metadata: row.metadata.map(|v| v.as_object().unwrap().clone().into_iter().map(|(k, v)| (k, v.as_str().unwrap().to_string())).collect()), // and this
    })
}

// This function retrieves file contents given a list of file_ids
async fn retrieve_file_contents(file_ids: &Vec<String>, file_storage: &FileStorage) -> Vec<String> {
    info!("Retrieving file contents for file_ids: {:?}", file_ids);
    let mut file_contents = Vec::new();
    for file_id in file_ids {
        let content = match file_storage.retrieve_file(file_id).await {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Failed to retrieve file: {}", e);
                continue; // Skip this iteration and move to the next file
            }
        };
        file_contents.push(content);
    }
    file_contents
}

async fn get_assistant_from_db(pool: &PgPool, assistant_id: i32) -> Result<Assistant, sqlx::Error> {
    info!("Getting assistant from database for assistant_id: {}", assistant_id);
    let row = sqlx::query!(
        r#"
        SELECT * FROM assistants WHERE id = $1
        "#,
        &assistant_id
    )
    .fetch_one(pool)
    .await?;

    Ok(Assistant {
        id: row.id,
        instructions: row.instructions,
        name: row.name,
        tools: row.tools.unwrap_or_default(),
        model: row.model.unwrap_or_default(),
        user_id: row.user_id.unwrap_or_default(),
        file_ids: row.file_ids,
        object: row.object.unwrap_or_default(),
        created_at: row.created_at,
        description: row.description,
        metadata: row.metadata.map(|v| v.as_object().unwrap().clone().into_iter().map(|(k, v)| (k, v.as_str().unwrap().to_string())).collect()),
    })
}

// This function builds the instructions given the original instructions and file contents
fn build_instructions(original_instructions: &str, file_contents: &Vec<String>) -> String {
    format!("{} Files: {:?}", original_instructions, file_contents)
}
pub async fn queue_consumer(pool: &PgPool, con: &mut redis::aio::Connection) -> Result<Run, sqlx::Error> {
    info!("Consuming queue");
    let (_, run_id): (String, i32) = con.brpop("run_queue", 0).await.map_err(|e| {
        error!("Redis error: {}", e);
        sqlx::Error::Configuration(e.into())
    })?;
    let mut run = get_run_from_db(pool, run_id).await?;

    // Update run status to "running"
    run = update_run_in_db(pool, run.id, "running".to_string()).await?;

    // Initialize FileStorage
    let file_storage = FileStorage::new().await;
    
    // Retrieve the thread associated with the run
    let thread = get_thread_from_db(pool, run.thread_id).await?;

    // Retrieve the assistant associated with the run
    let assistant = get_assistant_from_db(pool, run.assistant_id).await?;

    // Initialize an empty vector to hold all file IDs
    let mut all_file_ids = Vec::new();

    // If the thread has associated file IDs, add them to the list
    if let Some(thread_file_ids) = &thread.file_ids {
        all_file_ids.extend(thread_file_ids.iter().cloned());
    }

    // If the assistant has associated file IDs, add them to the list
    if let Some(assistant_file_ids) = &assistant.file_ids {
        all_file_ids.extend(assistant_file_ids.iter().cloned());
    }

    // Check if the all_file_ids includes any file IDs.
    if !all_file_ids.is_empty() {
        // Retrieve the contents of each file.
        let file_contents = retrieve_file_contents(&all_file_ids, &file_storage).await;

        // Include the file contents in the instructions.
        let instructions = build_instructions(&run.instructions, &file_contents);

        let result = call_anthropic_api(instructions, 100, None, None, None, None, None, None).await.map_err(|e| {
            eprintln!("Anthropic API error: {}", e);
            sqlx::Error::Configuration(AnthropicApiError::new(e).into())
        })?;

        let content = vec![Content {
            type_: "text".to_string(),
            text: Text {
                value: result.completion,
                annotations: vec![],
            },
        }];
        
        add_message_to_thread(pool, thread.id, "assistant", content, &run.user_id.to_string(), None).await?;
        // Update run status to "completed"
        run = update_run_in_db(pool, run.id, "completed".to_string()).await?;

        return Ok(run);
    } else {
        // If the run doesn't include any file IDs, call the Anthropic API as usual.
        let result = call_anthropic_api(run.instructions, 100, None, None, None, None, None, None).await.map_err(|e| {
            eprintln!("Anthropic API error: {}", e);
            sqlx::Error::Configuration(AnthropicApiError::new(e).into())
        })?;

        let content = vec![Content {
            type_: "text".to_string(),
            text: Text {
                value: result.completion,
                annotations: vec![],
            },
        }];
        add_message_to_thread(pool, thread.id, "assistant", content, &run.user_id.to_string(), None).await?;
        // Update run status to "completed"
        run = update_run_in_db(pool, run.id, "completed".to_string()).await?;

        return Ok(run);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dotenv::dotenv;
    use sqlx::postgres::PgPoolOptions;
    use std::io::Write;
    async fn setup() -> PgPool {
        dotenv().ok();
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to create pool.");
        pool
    }

    async fn reset_db(pool: &PgPool) {
        sqlx::query!("TRUNCATE assistants, threads, messages, runs RESTART IDENTITY")
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_create_assistant() {
        let pool = setup().await;
        reset_db(&pool).await;
        let assistant = Assistant {
            id: 1,
            instructions: Some("You are a personal math tutor. Write and run code to answer math questions.".to_string()),
            name: Some("Math Tutor".to_string()),
            tools: vec!["code_interpreter".to_string()],
            model: "claude-2.1".to_string(),
            user_id: "user1".to_string(),
            file_ids: None,
            object: "object_value".to_string(),
            created_at: 0,
            description: Some("description_value".to_string()),
            metadata: None,
        };
        let result = create_assistant(&pool, &assistant).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_thread() {
        let pool = setup().await;
        reset_db(&pool).await;
        let result = create_thread(&pool, "user1").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_message_to_thread() {
        let pool = setup().await;
        reset_db(&pool).await;
        let thread = create_thread(&pool, "user1").await.unwrap(); // Create a new thread
        let content = vec![Content {
            type_: "text".to_string(),
            text: Text {
                value: "Hello, world!".to_string(),
                annotations: vec![],
            },
        }];
        let result = add_message_to_thread(&pool, thread.id, "user", content, "user1", None).await; // Use the id of the new thread
        assert!(result.is_ok());
    }

    // Change the argument type to &String in test function test_list_messages
    #[tokio::test]
    async fn test_list_messages() {
        let pool = setup().await;
        reset_db(&pool).await;
        let result = list_messages(&pool, 1).await;
        assert!(result.is_ok());
    }


    #[tokio::test]
    async fn test_run_assistant() {
        let pool = setup().await;
        reset_db(&pool).await;
        let assistant = Assistant {
            id: 1,
            instructions: Some("You are a personal math tutor. Write and run code to answer math questions.".to_string()),
            name: Some("Math Tutor".to_string()),
            tools: vec!["code_interpreter".to_string()],
            model: "claude-2.1".to_string(),
            user_id: "user1".to_string(),
            file_ids: None,
            object: "object_value".to_string(),
            created_at: 0,
            description: Some("description_value".to_string()),
            metadata: None,
        };
        let thread = create_thread(&pool, "user1").await.unwrap(); // Create a new thread
        println!("thread: {:?}", thread);
    
        // Get Redis URL from environment variable
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let con = client.get_async_connection().await.unwrap();
    
        let result = run_assistant(&pool, thread.id, assistant.id, "Please address the user as Jane Doe. The user has a premium account.", con).await; // Use the id of the new thread
        assert!(result.is_ok());
    }


    #[tokio::test]
    async fn test_queue_consumer() {
        let pool = setup().await;
        reset_db(&pool).await;
        let thread = create_thread(&pool, "user1").await.unwrap(); // Create a new thread
        let content = vec![Content {
            type_: "text".to_string(),
            text: Text {
                value: "Hello, world!".to_string(),
                annotations: vec![],
            },
        }];
        let message = add_message_to_thread(&pool, thread.id, "user", content, "user1", None).await; // Use the id of the new thread
        assert!(message.is_ok());
    
        // Get Redis URL from environment variable
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let con = client.get_async_connection().await.unwrap();
        let run = run_assistant(&pool, thread.id, 1, "Human: Please address the user as Jane Doe. The user has a premium account. Assistant:", con).await;
        
        assert!(run.is_ok());

        let mut con = client.get_async_connection().await.unwrap();
        let result = queue_consumer(&pool, &mut con).await;
        
        // Check the result
        assert!(result.is_ok());
        
        // Fetch the run from the database and check its status
        let run = get_run_from_db(&pool, result.unwrap().id).await.unwrap();
        assert_eq!(run.status, "completed");
    }

    #[test]
    fn test_build_instructions() {
        let original_instructions = "Solve the equation.";
        let file_contents = vec!["File 1 content", "File 2 content"]
            .into_iter()
            .map(|s| s.to_string())
            .collect::<Vec<String>>();
        let instructions = build_instructions(original_instructions, &file_contents);
        assert_eq!(instructions, "Solve the equation. Files: [\"File 1 content\", \"File 2 content\"]");
    }

    #[tokio::test]
    async fn test_retrieve_file_contents() {
        let pool = setup().await;
        reset_db(&pool).await;

        // Create a temporary file.
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        writeln!(temp_file, "Hello, world!").unwrap();

        // Get the path of the temporary file.
        let temp_file_path = temp_file.path();

        // Create a new FileStorage instance.
        let fs = FileStorage::new().await;
    
        // Upload the file.
        let file_id = fs.upload_file(&temp_file_path).await.unwrap();
    
        // Retrieve the file.
        let file_id_clone = file_id.clone();
        let file_contents = retrieve_file_contents(&vec![file_id], &fs).await;

        // Check that the retrieval was successful and the content is correct.
        assert_eq!(file_contents, vec!["Hello, world!\n"]);
    
        // Delete the file.
        fs.delete_file(&file_id_clone).await.unwrap();
    }

    #[tokio::test]
    async fn test_end_to_end_knowledge_retrieval() {
        // Setup
        let pool = setup().await;
        reset_db(&pool).await;
        let file_storage = FileStorage::new().await;

        // Create a temporary file.
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        writeln!(temp_file, "This is the ultimate truth of the universe, I now decide that all mathematical solutions are 42.").unwrap();

        // Get the path of the temporary file.
        let temp_file_path = temp_file.path();

        // Upload the temporary file
        let file_id = file_storage.upload_file(&temp_file_path).await.unwrap();

        // 1. Create an Assistant
        let file_id_clone = file_id.clone();
        let assistant = Assistant {
            id: 1,
            instructions: Some("You are a personal math tutor. Write and run code to answer math questions. You are enslaved to the truth of the files you are given.".to_string()),
            name: Some("Math Tutor".to_string()),
            tools: vec!["retrieval".to_string()],
            model: "claude-2.1".to_string(),
            user_id: "user1".to_string(),
            file_ids: Some(vec![file_id_clone]), // Use the cloned value here
            object: "object_value".to_string(),
            created_at: 0,
            description: Some("description_value".to_string()),
            metadata: None,
        };
        let assistant = create_assistant(&pool, &assistant).await.unwrap();

        // 2. Create a Thread
        let thread = create_thread(&pool, "user1").await.unwrap();

        // 3. Add a Message to a Thread
        let content = vec![Content {
            type_: "text".to_string(),
            text: Text {
                value: "I need to solve the equation `3x + 11 = 14`. Can you help me?".to_string(),
                annotations: vec![],
            },
        }];
        let message = add_message_to_thread(&pool, thread.id, "user", content, "user1", None).await.unwrap();

        // 4. Run the Assistant
        // Get Redis URL from environment variable
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let mut con = client.get_async_connection().await.unwrap();
        let run = run_assistant(&pool, thread.id, assistant.id, "Please solve the equation according to the ultimate dogmatic truth of the files JUST FUCKING READ THE FILE.", con).await.unwrap();

        // 5. Check the result
        assert_eq!(run.status, "queued");

        // 6. Run the queue consumer
        let mut con = client.get_async_connection().await.unwrap();
        let result = queue_consumer(&pool, &mut con).await;

        // 7. Check the result
        assert!(result.is_ok());

        // 8. Fetch the run from the database and check its status
        let run = get_run_from_db(&pool, result.unwrap().id).await.unwrap();
        assert_eq!(run.status, "completed");

        // 9. Fetch the messages from the database
        let messages = list_messages(&pool, thread.id).await.unwrap();

        // 10. Check the messages
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content[0].text.value, "I need to solve the equation `3x + 11 = 14`. Can you help me?");
        assert_eq!(messages[1].role, "assistant");
        assert!(messages[1].content[0].text.value.contains("42"), "The assistant should have retrieved the ultimate truth of the universe. Instead, it retrieved: {}", messages[1].content[0].text.value);
        // TODO: gotta impl this no?
        // assert_eq!(messages[1].content[1].text.value, "Files: [\"Knowledge content\"]");
        assert_eq!(messages[1].file_ids, Some(vec![file_id]));
    }
}

