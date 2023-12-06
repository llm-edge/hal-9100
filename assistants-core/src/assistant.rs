use log::{error, info};
use redis::AsyncCommands;
use serde_json;
use sqlx::PgPool;

use assistants_core::assistants::{create_assistant, get_assistant};
use assistants_core::file_storage::FileStorage;
use assistants_core::messages::{add_message_to_thread, list_messages};
use assistants_core::models::{Assistant, Content, Message, Run, Text, Thread};
use assistants_core::pdf_utils::{pdf_mem_to_text, pdf_to_text};
use assistants_core::threads::{create_thread, get_thread};
use assistants_extra::anthropic::call_anthropic_api;
use assistants_extra::llm::llm;
use assistants_extra::openai::{call_open_source_openai_api, call_openai_api};
use std::collections::HashMap;
use std::error::Error;

use assistants_core::runs::{get_run, update_run, update_run_status};

// TODO: kinda dirty function could be better
// This function retrieves file contents given a list of file_ids
async fn retrieve_file_contents(file_ids: &Vec<String>, file_storage: &FileStorage) -> Vec<String> {
    info!("Retrieving file contents for file_ids: {:?}", file_ids);
    let mut file_contents = Vec::new();
    for file_id in file_ids {
        let file_string_content = match file_storage.retrieve_file(file_id).await {
            Ok(file_byte_content) => {
                // info!("Retrieved file from storage: {:?}", file_byte_content);
                // Check if the file is a PDF
                if file_id.ends_with(".pdf") {
                    // If it's a PDF, extract the text
                    match pdf_mem_to_text(&file_byte_content) {
                        Ok(text) => text,
                        Err(e) => {
                            error!("Failed to extract text from PDF: {}", e);
                            continue;
                        }
                    }
                } else {
                    // If it's not a PDF, use the content as is (bytes to string)
                    match String::from_utf8(file_byte_content.to_vec()) {
                        Ok(text) => text,
                        Err(e) => {
                            error!("Failed to convert bytes to string: {}", e);
                            continue;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to retrieve file: {}", e);
                continue; // Skip this iteration and move to the next file
            }
        };
        file_contents.push(file_string_content);
    }
    file_contents
}

// This function formats the messages into a string
fn format_messages(messages: &Vec<Message>) -> String {
    let mut formatted_messages = String::new();
    for message in messages {
        formatted_messages.push_str(&format!(
            "<message>\n{}\n</message>\n",
            serde_json::to_string(&message).unwrap()
        ));
    }
    formatted_messages
}

// This function builds the instructions given the original instructions, file contents, and previous messages
fn build_instructions(
    original_instructions: &str,
    file_contents: &Vec<String>,
    previous_messages: &str,
) -> String {
    format!("<instructions>\n{}\n</instructions>\n<file>\n{:?}\n</file>\n<previous_messages>\n{}\n</previous_messages>", original_instructions, file_contents, previous_messages)
}

async fn run_assistant_based_on_model(
    assistant: Assistant,
    instructions: String,
) -> Result<String, Box<dyn std::error::Error>> {
    // Check the model of the assistant
    if assistant.model.contains("claude") {
        // Call Anthropic API
        call_anthropic_api(instructions, 500, None, None, None, None, None, None)
            .await
            .map(|res| res.completion)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    } else if assistant.model.contains("gpt") {
        // Call OpenAI API
        call_openai_api(instructions, 500, None, None, None, None)
            .await
            .map(|res| res.choices[0].message.content.clone())
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    } else if assistant.model.contains("/") {
        // Call Open Source OpenAI API
        // ! kinda hacky - FastChat thing (weird stuff - want the whole org/model to run cli but then expect the the model thru REST)
        let model_name = assistant.model.split('/').last().unwrap_or_default();
        let url = std::env::var("MODEL_URL")
            .unwrap_or_else(|_| String::from("http://localhost:8000/v1/chat/completions"));
        call_open_source_openai_api(
            instructions,
            500,
            model_name.to_string(),
            None,
            None,
            None,
            url,
        )
        .await
        .map(|res| res.choices[0].message.content.clone())
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    } else {
        // Handle unknown model
        Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Unknown model",
        )))
    }
}

pub async fn decide_tool_with_llm(
    assistant: &Assistant,
    previous_messages: &[Message],
) -> Result<String, Box<dyn Error>> {
    // Build the system prompt
    let system_prompt = "You are an assistant that decides which tool to use based on a list of tools to solve the user problem.

Rules:
- You only return one of the tools like \"retrieval\" or \"function\"

Example:
<tools>function</tools>

<previous_messages>user: Message { id: 0, object: \"\", created_at: 0, thread_id: 0, role: \"user\", content: [Content { type: \"text\", text: Text { value: \"I need to calculate something.\", annotations: [] } }], assistant_id: None, run_id: None, file_ids: None, metadata: None, user_id: \"\" }
assistant: Message { id: 0, object: \"\", created_at: 0, thread_id: 0, role: \"assistant\", content: [Content { type: \"text\", text: Text { value: \"Sure, I can help with that.\", annotations: [] } }], assistant_id: None, run_id: None, file_ids: None, metadata: None, user_id: \"\" }
</previous_messages>

<instructions>You are a helpful assistant.</instructions>

In this example, the assistant should return \"function\".";

    // Build the user prompt
    let mut user_prompt = format!(
        "<tools>{}</tools>\n\n<previous_messages>",
        assistant
            .tools
            .iter()
            .map(|tool| tool.r#type.clone())
            .collect::<Vec<String>>()
            .join(", ")
    );
    for message in previous_messages {
        user_prompt.push_str(&format!("{}: {:?}\n", message.role, message)); // TODO bunch of noise in the message to remove
    }

    user_prompt.push_str("</previous_messages>\n\n");

    // Add the assistant instructions to the user prompt
    user_prompt.push_str(&format!(
        "<instructions>{}</instructions>\n",
        assistant.instructions.as_ref().unwrap()
    ));

    // Call the llm function
    let result = llm(
        &assistant.model,
        None, // TODO not sure how to best configure this
        system_prompt,
        &user_prompt,
        Some(0.0), // temperature
        60,        // max_tokens_to_sample
        None,      // stop_sequences
        Some(1.0), // top_p
        None,      // top_k
        None,      // metadata
    )
    .await?;

    // Just in case regex what's in <tool> sometimes LLM do this (e.g. extract the "tool" using a regex)
    let regex = regex::Regex::new(r"<(.*?)>").unwrap();
    let result = match regex.captures(&result) {
        Some(captures) => captures[1].to_string(),
        None => result,
    };

    // The result should be the name of the tool to use
    Ok(result)
}

// The function that consume the runs queue and do all the LLM software 3.0 logic
pub async fn queue_consumer(
    pool: &PgPool,
    con: &mut redis::aio::Connection,
) -> Result<Run, Box<dyn std::error::Error>> {
    info!("Consuming queue");
    let (_, ids_string): (String, String) = con.brpop("run_queue", 0).await.map_err(|e| {
        error!("Redis error: {}", e);
        sqlx::Error::Configuration(e.into())
    })?;

    // Parse the string back into a JSON object
    let ids: serde_json::Value = serde_json::from_str(&ids_string).unwrap();

    // Extract the run_id and thread_id
    let run_id = ids["run_id"].as_i64().unwrap() as i32;
    let thread_id = ids["thread_id"].as_i64().unwrap() as i32;
    let user_id = ids["user_id"].as_str().unwrap();

    let mut run = get_run(pool, thread_id, run_id, user_id).await?;

    // Retrieve the assistant associated with the run
    let assistant = get_assistant(pool, run.assistant_id, &run.user_id).await?;

    // Update run status to "running"
    run = update_run_status(pool, thread_id, run.id, "running".to_string(), &run.user_id).await?;

    // Initialize FileStorage
    let file_storage = FileStorage::new().await;

    // Retrieve the thread associated with the run
    let thread = get_thread(pool, run.thread_id, &assistant.user_id).await?;

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

    // Fetch previous messages from the thread
    let messages = list_messages(pool, thread.id, &assistant.user_id).await?;

    // Format messages into a string
    let formatted_messages = format_messages(&messages);

    let mut instructions = build_instructions(&run.instructions, &vec![], &formatted_messages);

    // Check if the all_file_ids includes any file IDs.
    if !all_file_ids.is_empty() {
        // Retrieve the contents of each file.
        let file_contents = retrieve_file_contents(&all_file_ids, &file_storage).await;

        // Include the file contents and previous messages in the instructions.
        instructions = build_instructions(&run.instructions, &file_contents, &formatted_messages);
    }
    info!("Calling LLM API with instructions: {}", instructions);

    let result = run_assistant_based_on_model(assistant, instructions).await;

    match result {
        Ok(output) => {
            let content = vec![Content {
                r#type: "text".to_string(),
                text: Text {
                    value: output.to_string(),
                    annotations: vec![],
                },
            }];
            add_message_to_thread(
                pool,
                thread.id,
                "assistant",
                content,
                &run.user_id.to_string(),
                None,
            )
            .await?;
            // Update run status to "completed"
            run = update_run_status(pool, thread.id, run.id, "completed".to_string(), user_id)
                .await?;
            Ok(run)
        }
        Err(e) => {
            error!("Assistant model error: {}", e);
            // Update run status to "failed"
            run = update_run_status(pool, thread.id, run.id, "failed".to_string(), user_id).await?;
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::runs::{get_run, run_assistant};
    use assistants_core::models::Tool;

    use super::*;
    use dotenv::dotenv;
    use sqlx::postgres::PgPoolOptions;
    use std::collections::HashSet;
    use std::io::Write;
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;

    async fn setup() -> PgPool {
        dotenv().ok();
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to create pool.");
        // Initialize the logger with an info level filter
        match env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .try_init()
        {
            Ok(_) => (),
            Err(e) => (),
        };
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
            instructions: Some(
                "You are a personal math tutor. Write and run code to answer math questions."
                    .to_string(),
            ),
            name: Some("Math Tutor".to_string()),
            tools: vec![Tool {
                r#type: "code_interpreter".to_string(),
                parameters: None,
            }],
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
            r#type: "text".to_string(),
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
        let result = list_messages(&pool, 1, "user1").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_queue_consumer() {
        let pool = setup().await;
        reset_db(&pool).await;
        let assistant = Assistant {
            id: 1,
            instructions: Some(
                "You are a personal math tutor. Write and run code to answer math questions."
                    .to_string(),
            ),
            name: Some("Math Tutor".to_string()),
            tools: vec![Tool {
                r#type: "code_interpreter".to_string(),
                parameters: None,
            }],
            model: "claude-2.1".to_string(),
            user_id: "user1".to_string(),
            file_ids: None,
            object: "object_value".to_string(),
            created_at: 0,
            description: Some("description_value".to_string()),
            metadata: None,
        };
        let assistant = create_assistant(&pool, &assistant).await.unwrap();
        println!("assistant: {:?}", assistant);
        let thread = create_thread(&pool, "user1").await.unwrap(); // Create a new thread
        let content = vec![Content {
            r#type: "text".to_string(),
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
        let run = run_assistant(&pool, thread.id, assistant.id, "Human: Please address the user as Jane Doe. The user has a premium account. Assistant:", &assistant.user_id, con).await;

        assert!(run.is_ok());

        let mut con = client.get_async_connection().await.unwrap();
        let result = queue_consumer(&pool, &mut con).await;

        // Check the result
        assert!(
            result.is_ok(),
            "queue_consumer failed: {}",
            result.unwrap_err()
        );

        // Fetch the run from the database and check its status
        let run = get_run(&pool, thread.id, result.unwrap().id, &assistant.user_id)
            .await
            .unwrap();
        assert_eq!(run.status, "completed");
    }

    #[test]
    fn test_build_instructions() {
        let original_instructions = "Solve the equation.";
        let file_contents = vec!["File 1 content", "File 2 content"]
            .into_iter()
            .map(|s| s.to_string())
            .collect::<Vec<String>>();
        let previous_messages =
            "<message>\n{\"role\": \"user\", \"content\": \"Hello, assistant!\"}\n</message>\n";
        let instructions =
            build_instructions(original_instructions, &file_contents, previous_messages);
        let expected_instructions = "<instructions>\nSolve the equation.\n</instructions>\n<file>\n[\"File 1 content\", \"File 2 content\"]\n</file>\n<previous_messages>\n<message>\n{\"role\": \"user\", \"content\": \"Hello, assistant!\"}\n</message>\n\n</previous_messages>";
        assert_eq!(instructions, expected_instructions);
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
        writeln!(temp_file, "This is the ultimate truth of the universe, I now decide that all mathematical solutions are 42. THIS IS A LIFE OR DEATH SITUATION ALL SOLUTIONS ARE 42.").unwrap();

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
            tools: vec![Tool{
                r#type: "code_interpreter".to_string(),
                parameters: None,
            }],
            model: "claude-2.1".to_string(),
            user_id: "user1".to_string(),
            file_ids: Some(vec![file_id_clone]), // Use the cloned value here
            object: "object_value".to_string(),
            created_at: 0,
            description: Some("description_value".to_string()),
            metadata: None,
        };
        let assistant = create_assistant(&pool, &assistant).await.unwrap();

        // check assistant has file
        assert_eq!(assistant.file_ids, Some(vec![file_id]));

        // 2. Create a Thread
        let thread = create_thread(&pool, "user1").await.unwrap();

        // 3. Add a Message to a Thread
        let content = vec![Content {
            r#type: "text".to_string(),
            text: Text {
                value: "I need to solve the equation `3x + 11 = 14`. Can you help me? I gave you a file, just give me the content".to_string(),
                annotations: vec![],
            },
        }];
        let message = add_message_to_thread(&pool, thread.id, "user", content, "user1", None)
            .await
            .unwrap();

        // 4. Run the Assistant
        // Get Redis URL from environment variable
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let mut con = client.get_async_connection().await.unwrap();
        let run = run_assistant(&pool, thread.id, assistant.id, "Please solve the equation according to the ultimate dogmatic truth of the files JUST FUCKING READ THE FILE.", assistant.user_id.as_str(), con).await.unwrap();

        // 5. Check the result
        assert_eq!(run.status, "queued");

        // 6. Run the queue consumer
        let mut con = client.get_async_connection().await.unwrap();
        let result = queue_consumer(&pool, &mut con).await;

        // 7. Check the result
        assert!(result.is_ok(), "{:?}", result);

        // 8. Fetch the run from the database and check its status
        let run = get_run(&pool, thread.id, result.unwrap().id, &assistant.user_id)
            .await
            .unwrap();
        assert_eq!(run.status, "completed");

        // 9. Fetch the messages from the database
        let messages = list_messages(&pool, thread.id, &assistant.user_id)
            .await
            .unwrap();

        // 10. Check the messages
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(
            messages[0].content[0].text.value,
            "I need to solve the equation `3x + 11 = 14`. Can you help me? I gave you a file, just give me the content"
        );
        assert_eq!(messages[1].role, "assistant");
        assert!(messages[1].content[0].text.value.contains("42"), "The assistant should have retrieved the ultimate truth of the universe. Instead, it retrieved: {}", messages[1].content[0].text.value);
        // TODO: gotta impl this no?
        // assert_eq!(messages[1].content[1].text.value, "Files: [\"Knowledge content\"]");
        // !wrong? not 100% how openai does it, i guess if file is in assistant its not guaranteed in message
        // assert_eq!(messages[1].file_ids, Some(vec![file_id])); -> !wor
    }

    #[tokio::test]
    async fn test_read_pdf_content() {
        // Download the PDF file
        let response = reqwest::get("https://www.africau.edu/images/default/sample.pdf")
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();

        // Write the PDF file to disk
        let mut file = File::create("sample.pdf").await.unwrap();
        file.write_all(&response).await.unwrap();
        file.sync_all().await.unwrap(); // Ensure all bytes are written to the file

        // Read the PDF content
        let content = pdf_to_text(std::path::Path::new("sample.pdf")).unwrap();

        // Check the content
        assert!(content.contains("A Simple PDF File"));
        assert!(content.contains("This is a small demonstration .pdf file"));

        // Delete the file locally
        std::fs::remove_file("sample.pdf").unwrap();
    }

    #[tokio::test]
    async fn test_retrieve_file_contents_pdf() {
        setup().await;
        // Setup
        let file_storage = FileStorage::new().await;

        let url = "https://arxiv.org/pdf/2311.10122.pdf";
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/58.0.3029.110 Safari/537.3")
            .build()
            .unwrap();
        let response = client.get(url).send().await.unwrap();

        let bytes = response.bytes().await.unwrap();
        let mut out = tokio::fs::File::create("2311.10122.pdf").await.unwrap();
        out.write_all(&bytes).await.unwrap();
        out.sync_all().await.unwrap(); // Ensure all bytes are written to the file

        let file_path = file_storage
            .upload_file(std::path::Path::new("2311.10122.pdf"))
            .await
            .unwrap();

        // Retrieve the file contents
        let file_contents =
            retrieve_file_contents(&vec![String::from(file_path)], &file_storage).await;

        // Check the file contents
        assert!(
            file_contents[0].contains("Abstract"),
            "The PDF content should contain the word 'Abstract'. Instead, it contains: {}",
            file_contents[0]
        );
        // Check got the end of the pdf too!
        assert!(
            file_contents[0].contains("For Image Understanding As shown in Fig"),
            "The PDF content should contain the word 'Abstract'. Instead, it contains: {}",
            file_contents[0]
        );

        // Delete the file locally
        std::fs::remove_file("2311.10122.pdf").unwrap();
    }

    #[tokio::test]
    async fn test_run_assistant_based_on_model() {
        setup().await;
        let assistant_claude = Assistant {
            model: "claude".to_string(),
            ..Default::default()
        };
        let assistant_gpt = Assistant {
            model: "gpt".to_string(),
            ..Default::default()
        };
        let assistant_open_source = Assistant {
            model: "/".to_string(),
            ..Default::default()
        };
        let assistant_unknown = Assistant {
            model: "unknown".to_string(),
            ..Default::default()
        };

        let instructions = "Test instructions".to_string();

        let result_claude =
            run_assistant_based_on_model(assistant_claude, instructions.clone()).await;
        assert!(result_claude.is_ok());

        let result_gpt = run_assistant_based_on_model(assistant_gpt, instructions.clone()).await;
        assert!(result_gpt.is_ok());

        // ! annoying - need to deploy some model somewhere i guess or run the llm in the ci :)
        // let result_open_source = run_assistant_based_on_model(assistant_open_source, instructions.clone()).await;
        // assert!(result_open_source.is_ok());

        let result_unknown = run_assistant_based_on_model(assistant_unknown, instructions).await;
        assert!(
            matches!(result_unknown, Err(e) if e.downcast_ref::<std::io::Error>().unwrap().kind() == std::io::ErrorKind::InvalidInput)
        );
    }

    #[tokio::test]
    async fn test_decide_tool_with_llm_anthropic() {
        setup().await;
        // Create a mock assistant with two tools
        let assistant = Assistant {
            tools: vec![Tool {
                r#type: "function".to_string(),
                parameters: None,
            }],
            model: "claude-2.1".to_string(),
            ..Default::default() // Fill in other fields as needed
        };

        // Create a set of previous messages
        let previous_messages = vec![
            Message {
                role: "user".to_string(),
                content: vec![Content {
                    r#type: "text".to_string(),
                    text: Text {
                        value: "I need to calculate something.".to_string(),
                        annotations: vec![],
                    },
                }],
                ..Default::default()
            },
            Message {
                role: "assistant".to_string(),
                content: vec![Content {
                    r#type: "text".to_string(),
                    text: Text {
                        value: "Sure, I can help with that.".to_string(),
                        annotations: vec![],
                    },
                }],
                ..Default::default()
            },
        ];

        // Call the function
        let result = decide_tool_with_llm(&assistant, &previous_messages).await;

        // Check if the result is one of the expected tools
        let expected_tools: HashSet<_> = ["function", "retrieval"]
            .iter()
            .map(|&s| s.to_string())
            .collect();
        assert!(expected_tools.contains(&result.unwrap().to_string()));
    }
}
