use async_openai::types::{
    AssistantTools, FunctionCall, MessageContent, MessageContentTextObject, MessageRole,
    RequiredAction, RunStatus, RunToolCallObject, SubmitToolOutputs, TextData,
};
use log::{error, info};
use redis::AsyncCommands;
use serde_json::{self, json};
use sqlx::PgPool;

use assistants_core::assistants::{create_assistant, get_assistant};
use assistants_core::file_storage::FileStorage;
use assistants_core::messages::{add_message_to_thread, list_messages};
use assistants_core::models::{Assistant, Message, Run, Thread};
use assistants_core::pdf_utils::{pdf_mem_to_text, pdf_to_text};
use assistants_core::threads::{create_thread, get_thread};
use assistants_extra::anthropic::call_anthropic_api;
use assistants_extra::llm::llm;
use assistants_extra::openai::{call_open_source_openai_api, call_openai_api};
use std::collections::{HashMap, HashSet};
use std::error::Error;

use assistants_core::runs::{get_run, update_run, update_run_status};

use assistants_core::function_calling::ModelConfig;

use assistants_core::function_calling::create_function_call;

use assistants_core::runs::get_tool_calls;

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
            serde_json::json!({
                "role": message.inner.role,
                "content": message.inner.content
            })
        ));
    }
    formatted_messages
}

// This function builds the instructions given the original instructions, file contents, and previous messages
fn build_instructions(
    original_instructions: &str,
    file_contents: &Vec<String>,
    previous_messages: &str,
    tools: &str,
) -> String {
    let mut instructions = format!(
        "<instructions>\n{}\n</instructions>\n",
        original_instructions
    );

    if !file_contents.is_empty() {
        instructions += &format!("<file>\n{:?}\n</file>\n", file_contents);
    }

    if !tools.is_empty() {
        instructions += &format!("<tools>\n{}\n</tools>\n", tools);
    }

    instructions += &format!(
        "<previous_messages>\n{}\n</previous_messages>",
        previous_messages
    );

    instructions
}

async fn run_assistant_based_on_model(
    assistant: Assistant,
    instructions: String,
) -> Result<String, Box<dyn std::error::Error>> {
    // Check the model of the assistant
    if assistant.inner.model.contains("claude") {
        // Call Anthropic API
        call_anthropic_api(instructions, 500, None, None, None, None, None, None)
            .await
            .map(|res| res.completion)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    } else if assistant.inner.model.contains("gpt") {
        // Call OpenAI API
        call_openai_api(instructions, 500, None, None, None, None)
            .await
            .map(|res| res.choices[0].message.content.clone())
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    } else if assistant.inner.model.contains("/") {
        // Call Open Source OpenAI API
        // ! kinda hacky - FastChat thing (weird stuff - want the whole org/model to run cli but then expect the the model thru REST)
        let model_name = assistant.inner.model.split('/').last().unwrap_or_default();
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
) -> Result<Vec<String>, Box<dyn Error>> {
    // Build the system prompt
    let system_prompt = "You are an assistant that decides which tool to use based on a list of tools to solve the user problem.

Rules:
- You only return one of the tools like \"<retrieval>\" or \"<function>\" or both.
- Do not return \"tools\"
- Feel free to use MORE tools rather than LESS
- The tool names must be one of the tools available e.g. only retrieval or function atm, nothing else OR A HUMAN WILL DIE
- Your answer must be very concise and make sure to surround the tool by <>, do not say anything but the tool name with the <> around it.

Example:
<user>
<tools>{\"description\":\"useful to call functions in the user's product, which would provide you later some additional context about the user's problem\",\"function\":{\"arguments\":{\"type\":\"object\"},\"description\":\"A function that compute the purpose of life according to the fundamental laws of the universe.\",\"name\":\"compute_purpose_of_life\"},\"name\":\"function\"}
---
{\"description\":\"useful to retrieve information from files\",\"name\":\"retrieval\"}</tools>

<previous_messages>User: [Text(MessageContentTextObject { type: \"text\", text: TextData { value: \"I need to know the purpose of life, you can give me two answers.\", annotations: [] } })]
</previous_messages>

<instructions>You help me by using the tools you have.</instructions>

</user>

In this example, the assistant should return \"<function>,<retrieval>\".

Another example:
<user>
<tools>{\"description\":\"useful to call functions in the user's product, which would provide you later some additional context about the user's problem\",\"function\":{\"arguments\":{\"type\":\"object\"},\"description\":\"A function that compute the cosine similarity between two vectors.\",\"name\":\"compute_cosine_similarity\"},\"name\":\"function\"}
---
{\"description\":\"useful to retrieve information from files\",\"name\":\"retrieval\"}</tools>

<previous_messages>User: [Text(MessageContentTextObject { type: \"text\", text: TextData { value: \"Given these two vectors, how similar are they?\", annotations: [] } })]
</previous_messages>

<instructions>You help me by using the tools you have.</instructions>

</user>
Another example:
<user>
<tools>{\"description\":\"useful to call functions in the user's product, which would provide you later some additional context about the user's problem\",\"function\":{\"arguments\":{\"type\":\"object\"},\"description\":\"A function that retrieves the customer's order history.\",\"name\":\"get_order_history\"},\"name\":\"function\"}
---
{\"description\":\"useful to retrieve information from files\",\"name\":\"retrieval\"}</tools>

<previous_messages>User: [Text(MessageContentTextObject { type: \"text\", text: TextData { value: \"Can you tell me what my best selling products are?\", annotations: [] } })]
</previous_messages>

<instructions>You help me by using the tools you have.</instructions>

</user>

In this example, the assistant should return \"<function>,<retrieval>\".

Your answer will be used to use the tool so it must be very concise and make sure to surround the tool by <>, do not say anything but the tool name with the <> around it.";

    let tools = assistant.inner.tools.clone();
    println!("tools: {:?}", tools);
    // Build the user prompt
    let tools_as_string = tools
        .iter()
        .map(|t| {
            serde_json::to_string(&match t {
                AssistantTools::Code(_) => json!({"name": "code_interpreter", "description": "useful for complex math problems"}),
                AssistantTools::Retrieval(_) => json!({"name": "retrieval", "description": "useful to retrieve information from files"}),
                AssistantTools::Function(e) => 
                    json!({
                        "name": "function",
                        "description": "useful to call functions in the user's product, which would provide you later some additional context about the user's problem",
                        "function": {
                            "name": e.function.name,
                            "description": e.function.description,
                            "arguments": e.function.parameters,
                        }
                    })
            }).unwrap()
        })
        .collect::<Vec<String>>();
    let tools_as_string = tools_as_string.join("\n---\n");
    let mut user_prompt = format!("<tools>{}</tools>\n\n<previous_messages>", tools_as_string);
    for message in previous_messages {
        user_prompt.push_str(&format!(
            "{:?}: {:?}\n",
            message.inner.role, message.inner.content
        ));
        // TODO bunch of noise in the message to remove
    }

    user_prompt.push_str("</previous_messages>\n\n");

    // Add the assistant instructions to the user prompt
    user_prompt.push_str(&format!(
        "<instructions>{}</instructions>\n",
        assistant.inner.instructions.as_ref().unwrap()
    ));

    // Call the llm function
    let result = llm(
        &assistant.inner.model,
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
    let mut results = Vec::new();
    for captures in regex.captures_iter(&result) {
        results.push(captures[1].to_string());
    }
    // if there is a , in the <> just split it, remove spaces
    results = results
        .iter()
        .flat_map(|r| r.split(',').map(|s| s.trim().to_string()))
        .collect::<Vec<String>>();

    // remove non alphanumeric chars
    results = results
        .iter()
        .map(|r| {
            r.chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
        })
        .collect::<Vec<String>>();

    Ok(results
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>())
}

// The function that consume the runs queue and do all the LLM software 3.0 logic
pub async fn queue_consumer(
    // TODO: split in smaller functions if possible
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
    let run_id = ids["run_id"].as_str().unwrap();
    let thread_id = ids["thread_id"].as_str().unwrap();
    let user_id = ids["user_id"].as_str().unwrap();

    info!("Retrieving run");
    let mut run = get_run(pool, thread_id, run_id, user_id).await?;

    info!("Retrieving assistant {:?}", run.inner.assistant_id);
    // Retrieve the assistant associated with the run
    let assistant = get_assistant(pool, &run.inner.assistant_id.unwrap(), &run.user_id).await?;

    // Update run status to "running"
    run = update_run_status(
        pool,
        thread_id,
        &run.inner.id,
        RunStatus::InProgress,
        &run.user_id,
        None,
    )
    .await?;

    // Initialize FileStorage
    let file_storage = FileStorage::new().await;

    // Retrieve the thread associated with the run
    info!("Retrieving thread {}", run.inner.thread_id);
    let thread = get_thread(pool, &run.inner.thread_id, &assistant.user_id).await?;

    // Fetch previous messages from the thread
    let messages = list_messages(pool, &thread.inner.id, &assistant.user_id).await?;

    // Format messages into a string
    let formatted_messages = format_messages(&messages);
    info!("Formatted messages: {}", formatted_messages);

    let mut tools = String::new();

    // Check if the run has a required action
    if let Some(required_action) = &run.inner.required_action {
        // If the required action type is "submit_tool_outputs", fetch the tool calls from the database
        // if required_action.r#type == "submit_tool_outputs" { ! // dont care for now
        info!(
            "Retrieving tool calls {:?}",
            required_action.submit_tool_outputs
        );
        // TODO: if user send just part of the function result and not all should error
        let tool_calls_db = get_tool_calls(
            pool,
            required_action
                .submit_tool_outputs
                .tool_calls
                .iter()
                .map(|t| t.id.as_str())
                .collect(),
        )
        .await?;

        // Use the tool call data to build the prompt like Input "functions" Output ""..."" DUMB MODE
        tools = required_action
            .submit_tool_outputs
            .tool_calls
            .iter()
            .zip(&tool_calls_db)
            .map(|(input, output)| {
                format!(
                    "<input>{:?}</input>\n\n<output>{:?}</output>",
                    input.function, output.output
                )
            })
            .collect::<Vec<String>>()
            .join("\n");

        info!("Tools: {}", tools);
        // }
    }

    // Decide which tool to use
    let tools_decision = decide_tool_with_llm(&assistant, &messages).await?;
    info!("Tools decision: {:?}", tools_decision);

    let mut instructions = build_instructions(
        &run.inner.instructions,
        &vec![],
        &formatted_messages,
        &tools,
    );

    let model = assistant.inner.model.clone();
    // Call create_function_call here
    let model_config = ModelConfig {
        model_name: model,
        model_url: None,
        user_prompt: formatted_messages.clone(), // TODO: assuming this is the user prompt. Should it be just last message? Or more custom?
        temperature: Some(0.0),
        max_tokens_to_sample: 200,
        stop_sequences: None,
        top_p: Some(1.0),
        top_k: None,
        metadata: None,
    };

    // for each tool
    for tool_decision in tools_decision {
        // TODO: can prob optimise thru parallelism
        match tool_decision.as_str() {
            "function" => {
                info!("Using function tool");
                // skip this if tools is not empty (e.g. if there are required_action (s))
                if !run.inner.required_action.is_none() {
                    info!("Skipping function call because there is a required action");
                    continue;
                }
                run = update_run_status(
                    // TODO: unclear if the pending is properly placed here https://platform.openai.com/docs/assistants/tools/function-calling
                    pool,
                    thread_id,
                    &run.inner.id,
                    RunStatus::Queued,
                    &run.user_id,
                    None,
                )
                .await?;
                info!("Generating function to call");

                let function_results =
                    create_function_call(&pool, user_id, model_config.clone()).await?;

                info!("Function results: {:?}", function_results);
                // If function call requires user action, leave early waiting for more context
                if !function_results.is_empty() {
                    // Update run status to "requires_action"
                    run = update_run_status(
                        pool,
                        thread_id,
                        &run.inner.id,
                        RunStatus::RequiresAction,
                        &run.user_id,
                        Some(RequiredAction {
                            r#type: "submit_tool_outputs".to_string(),
                            submit_tool_outputs: SubmitToolOutputs {
                                tool_calls: function_results
                                    .iter()
                                    .map(|f| RunToolCallObject {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        r#type: "function".to_string(), // TODO hardcoded
                                        function: FunctionCall {
                                            name: f.clone().name,
                                            arguments: f.clone().arguments,
                                        },
                                    })
                                    .collect::<Vec<RunToolCallObject>>(),
                            },
                        }),
                    )
                    .await?;
                    info!(
                        "Run updated to requires_action with {:?}",
                        run.inner.required_action
                    );
                    return Ok(run);
                }
            }
            "retrieval" => {
                // Call file retrieval here
                // Initialize an empty vector to hold all file IDs
                let mut all_file_ids = Vec::new();

                // If the run has associated file IDs, add them to the list
                all_file_ids.extend(run.inner.file_ids.iter().cloned());

                // If the assistant has associated file IDs, add them to the list
                all_file_ids.extend(assistant.inner.file_ids.iter().cloned());

                // Check if the all_file_ids includes any file IDs.
                if !all_file_ids.is_empty() {
                    info!("Retrieving file contents for file_ids: {:?}", all_file_ids);
                    // Retrieve the contents of each file.
                    let file_contents = retrieve_file_contents(&all_file_ids, &file_storage).await;

                    // Include the file contents and previous messages in the instructions.
                    instructions = build_instructions(
                        &run.inner.instructions,
                        &file_contents,
                        &formatted_messages,
                        &tools,
                    );
                }
            }
            _ => {
                // Handle unknown tool
                error!("Unknown tool: {}", tool_decision);
                // TODO Update run status to "failed"
            }
        }
    }

    info!("Calling LLM API with instructions: {}", instructions);

    let result = run_assistant_based_on_model(assistant, instructions).await;

    match result {
        Ok(output) => {
            info!("LLM API output: {}", output);
            let content = vec![MessageContent::Text(MessageContentTextObject {
                r#type: "text".to_string(),
                text: TextData {
                    value: output.to_string(),
                    annotations: vec![],
                },
            })];
            add_message_to_thread(
                pool,
                &thread.inner.id,
                MessageRole::Assistant,
                content,
                &run.user_id.to_string(),
                None,
            )
            .await?;
            // Update run status to "completed"
            run = update_run_status(
                pool,
                &thread.inner.id,
                &run.inner.id,
                RunStatus::Completed,
                user_id,
                None,
            )
            .await?;
            Ok(run)
        }
        Err(e) => {
            error!("Assistant model error: {}", e);
            // Update run status to "failed"
            run = update_run_status(
                pool,
                &thread.inner.id,
                &run.inner.id,
                RunStatus::Failed,
                user_id,
                None,
            )
            .await?;
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use assistants_core::runs::{get_run, run_assistant};
    use async_openai::types::{
        AssistantObject, AssistantTools, AssistantToolsCode, AssistantToolsFunction,
        AssistantToolsRetrieval, ChatCompletionFunctions, MessageObject, MessageRole,
    };
    use serde_json::json;
    use sqlx::types::Uuid;

    use crate::models::SubmittedToolCall;
    use crate::runs::{create_run, submit_tool_outputs};

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
    async fn reset_redis() -> redis::RedisResult<()> {
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url)?;
        let mut con = client.get_async_connection().await?;
        redis::cmd("FLUSHALL").query_async(&mut con).await?;
        Ok(())
    }
    async fn reset_db(pool: &PgPool) {
        // TODO should also purge minio
        sqlx::query!(
            "TRUNCATE assistants, threads, messages, runs, functions, tool_calls RESTART IDENTITY"
        )
        .execute(pool)
        .await
        .unwrap();
        reset_redis().await.unwrap();
    }

    #[tokio::test]
    async fn test_create_assistant() {
        let pool = setup().await;
        reset_db(&pool).await;
        let assistant = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "You are a personal math tutor. Write and run code to answer math questions."
                        .to_string(),
                ),
                name: Some("Math Tutor".to_string()),
                tools: vec![AssistantTools::Code(AssistantToolsCode {
                    r#type: "code_interpreter".to_string(),
                })],
                model: "claude-2.1".to_string(),
                file_ids: vec![],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let result = create_assistant(&pool, &assistant).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_thread() {
        let pool = setup().await;
        reset_db(&pool).await;
        let result = create_thread(&pool, &Uuid::default().to_string()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_message_to_thread() {
        let pool = setup().await;
        reset_db(&pool).await;
        let thread = create_thread(&pool, &Uuid::default().to_string())
            .await
            .unwrap(); // Create a new thread
        let content = vec![MessageContent::Text(MessageContentTextObject {
            r#type: "text".to_string(),
            text: TextData {
                value: "Hello world".to_string(),
                annotations: vec![],
            },
        })];
        let result = add_message_to_thread(
            &pool,
            &thread.inner.id,
            MessageRole::User,
            content,
            &Uuid::default().to_string(),
            None,
        )
        .await; // Use the id of the new thread
        assert!(result.is_ok());
    }

    // Change the argument type to &String in test function test_list_messages
    #[tokio::test]
    async fn test_list_messages() {
        let pool = setup().await;
        reset_db(&pool).await;
        let result = list_messages(&pool, "0", &Uuid::default().to_string()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore] // TODO: this test is just bad
    async fn test_queue_consumer() {
        let pool = setup().await;
        reset_db(&pool).await;
        let assistant = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "You are a personal math tutor. Write and run code to answer math questions."
                        .to_string(),
                ),
                name: Some("Math Tutor".to_string()),
                tools: vec![AssistantTools::Code(AssistantToolsCode {
                    r#type: "code_interpreter".to_string(),
                })],
                model: "claude-2.1".to_string(),
                file_ids: vec![],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let assistant = create_assistant(&pool, &assistant).await.unwrap();
        println!("assistant: {:?}", assistant);
        let thread = create_thread(&pool, &Uuid::default().to_string())
            .await
            .unwrap(); // Create a new thread
        let content = vec![MessageContent::Text(MessageContentTextObject {
            r#type: "text".to_string(),
            text: TextData {
                value: "Hello world".to_string(),
                annotations: vec![],
            },
        })];
        let message = add_message_to_thread(
            &pool,
            &thread.inner.id,
            MessageRole::User,
            content,
            &Uuid::default().to_string(),
            None,
        )
        .await; // Use the id of the new thread
        assert!(message.is_ok());
        let run = create_run(&pool, &thread.inner.id,& assistant.inner.id, "Human: Please address the user as Jane Doe. The user has a premium account. Assistant:", &assistant.user_id).await;

        // Get Redis URL from environment variable
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let con = client.get_async_connection().await.unwrap();
        let run = run_assistant(&pool, &thread.inner.id, &assistant.inner.id, "Human: Please address the user as Jane Doe. The user has a premium account. Assistant:", &assistant.user_id, con).await;

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
        let run = get_run(
            &pool,
            &thread.inner.id,
            &result.unwrap().inner.id,
            &assistant.user_id,
        )
        .await
        .unwrap();
        assert_eq!(run.inner.status, RunStatus::Completed);
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
            build_instructions(original_instructions, &file_contents, previous_messages, "");
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
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "You are a personal math tutor. Write and run code to answer math questions."
                        .to_string(),
                ),
                name: Some("Math Tutor".to_string()),
                tools: vec![AssistantTools::Retrieval(AssistantToolsRetrieval {
                    r#type: "retrieval".to_string(),
                })],
                model: "claude-2.1".to_string(),
                file_ids: vec![file_id_clone],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let assistant = create_assistant(&pool, &assistant).await.unwrap();

        // check assistant has file
        assert_eq!(assistant.inner.file_ids, vec![file_id]);

        // 2. Create a Thread
        let thread = create_thread(&pool, &Uuid::default().to_string())
            .await
            .unwrap();

        // 3. Add a Message to a Thread
        let content = vec![MessageContent::Text(MessageContentTextObject {
            r#type: "text".to_string(),
            text: TextData {
                value: "I need to solve the equation `3x + 11 = 14`. Can you help me? I gave you a file, just give me the content".to_string(),
                annotations: vec![],
            },
        })];
        let message = add_message_to_thread(
            &pool,
            &thread.inner.id,
            MessageRole::User,
            content,
            &Uuid::default().to_string(),
            None,
        )
        .await
        .unwrap();

        // 4. Run the Assistant
        // Get Redis URL from environment variable
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let mut con = client.get_async_connection().await.unwrap();
        let run = run_assistant(&pool, &thread.inner.id, &assistant.inner.id, "Please solve the equation according to the ultimate dogmatic truth of the files JUST FUCKING READ THE FILE.", assistant.user_id.as_str(), con).await.unwrap();

        // 5. Check the result
        assert_eq!(run.inner.status, RunStatus::Queued);

        // 6. Run the queue consumer
        let mut con = client.get_async_connection().await.unwrap();
        let result = queue_consumer(&pool, &mut con).await;

        // 7. Check the result
        assert!(result.is_ok(), "{:?}", result);

        // 8. Fetch the run from the database and check its status
        let run = get_run(
            &pool,
            &thread.inner.id,
            &result.unwrap().inner.id,
            &assistant.user_id,
        )
        .await
        .unwrap();
        assert_eq!(run.inner.status, RunStatus::Completed);

        // 9. Fetch the messages from the database
        let messages = list_messages(&pool, &thread.inner.id, &assistant.user_id)
            .await
            .unwrap();

        // 10. Check the messages
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].inner.role, MessageRole::User);
        if let MessageContent::Text(text_object) = &messages[0].inner.content[0] {
            assert_eq!(
                text_object.text.value,
                "I need to solve the equation `3x + 11 = 14`. Can you help me? I gave you a file, just give me the content"
            );
        } else {
            panic!("Expected a Text message, but got something else.");
        }

        assert_eq!(messages[1].inner.role, MessageRole::Assistant);
        if let MessageContent::Text(text_object) = &messages[1].inner.content[0] {
            assert!(text_object.text.value.contains("42"), "The assistant should have retrieved the ultimate truth of the universe. Instead, it retrieved: {}", text_object.text.value);
        } else {
            panic!("Expected a Text message, but got something else.");
        }
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
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "You are a personal math tutor. Write and run code to answer math questions."
                        .to_string(),
                ),
                name: Some("Math Tutor".to_string()),
                tools: vec![AssistantTools::Code(AssistantToolsCode {
                    r#type: "code_interpreter".to_string(),
                })],
                model: "claude-2.1".to_string(),
                file_ids: vec![],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let assistant_gpt = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "You are a personal math tutor. Write and run code to answer math questions."
                        .to_string(),
                ),
                name: Some("Math Tutor".to_string()),
                tools: vec![AssistantTools::Code(AssistantToolsCode {
                    r#type: "code_interpreter".to_string(),
                })],
                model: "gpt".to_string(),
                file_ids: vec![],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let assistant_open_source = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "You are a personal math tutor. Write and run code to answer math questions."
                        .to_string(),
                ),
                name: Some("Math Tutor".to_string()),
                tools: vec![AssistantTools::Code(AssistantToolsCode {
                    r#type: "code_interpreter".to_string(),
                })],
                model: "open-source/llama-2-70b-chat".to_string(),
                file_ids: vec![],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let assistant_unknown = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "You are a personal math tutor. Write and run code to answer math questions."
                        .to_string(),
                ),
                name: Some("Math Tutor".to_string()),
                tools: vec![AssistantTools::Code(AssistantToolsCode {
                    r#type: "code_interpreter".to_string(),
                })],
                model: "unknown".to_string(),
                file_ids: vec![],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
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
        let mut functions = ChatCompletionFunctions {
            description: Some("A calculator function".to_string()),
            name: "calculator".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "a": {
                        "type": "number",
                        "description": "The first number."
                    },
                    "b": {
                        "type": "number",
                        "description": "The second number."
                    }
                }
            }),
        };

        let assistant = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "You are a personal math tutor. Write and run code to answer math questions."
                        .to_string(),
                ),
                name: Some("Math Tutor".to_string()),
                tools: vec![AssistantTools::Function(AssistantToolsFunction {
                    r#type: "function".to_string(),
                    function: functions,
                })],
                model: "claude-2.1".to_string(),
                file_ids: vec![],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };

        // Create a set of previous messages
        let previous_messages = vec![Message {
            inner: MessageObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                thread_id: "".to_string(),
                role: MessageRole::User,
                content: vec![MessageContent::Text(MessageContentTextObject {
                    r#type: "text".to_string(),
                    text: TextData {
                        value: "I need to calculate something.".to_string(),
                        annotations: vec![],
                    },
                })],
                assistant_id: None,
                run_id: None,
                file_ids: vec![],
                metadata: None,
            },
            user_id: "".to_string(),
        }];
        // Call the function
        let result = decide_tool_with_llm(&assistant, &previous_messages).await;
        let mut result = result.unwrap();
        // Check if the result is one of the expected tools
        let mut expected_tools = vec!["function".to_string(), "retrieval".to_string()];
        assert_eq!(result.sort(), expected_tools.sort());
    }

    #[tokio::test]
    async fn test_decide_tool_with_llm_open_source() {
        setup().await;
        let mut functions = ChatCompletionFunctions {
            description: Some("A calculator function".to_string()),
            name: "calculator".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "a": {
                        "type": "number",
                        "description": "The first number."
                    },
                    "b": {
                        "type": "number",
                        "description": "The second number."
                    }
                }
            }),
        };
        let assistant = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "You are a personal math tutor. Write and run code to answer math questions."
                        .to_string(),
                ),
                name: Some("Math Tutor".to_string()),
                tools: vec![AssistantTools::Function(AssistantToolsFunction {
                    r#type: "function".to_string(),
                    function: functions,
                })],
                model: "open-source/mistral-7b-instruct".to_string(),
                file_ids: vec![],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };

        let previous_messages = vec![Message {
            inner: MessageObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                thread_id: "".to_string(),
                role: MessageRole::User,
                content: vec![MessageContent::Text(MessageContentTextObject {
                    r#type: "text".to_string(),
                    text: TextData {
                        value: "I need to calculate something.".to_string(),
                        annotations: vec![],
                    },
                })],
                assistant_id: None,
                run_id: None,
                file_ids: vec![],
                metadata: None,
            },
            user_id: "".to_string(),
        }];
        // ! HACK
        std::env::set_var("MODEL_URL", "https://api.perplexity.ai/chat/completions");

        // Call the decide_tool_with_llm function using the open-source LLM
        let result = decide_tool_with_llm(&assistant, &previous_messages).await;

        let mut result = result.unwrap();
        // Check if the result is one of the expected tools
        let mut expected_tools = vec!["function".to_string(), "retrieval".to_string()];
        assert_eq!(result.sort(), expected_tools.sort());
    }

    #[tokio::test]
    async fn test_end_to_end_function_calling_plus_retrieval() {
        // Setup
        let pool = setup().await;
        reset_db(&pool).await;
        let file_storage = FileStorage::new().await;

        // 1. Create a temporary file.
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        writeln!(temp_file, "The purpose of life is 43.").unwrap();

        // 2. Get the path of the temporary file.
        let temp_file_path = temp_file.path();

        // 3. Upload the temporary file
        let file_id = file_storage.upload_file(&temp_file_path).await.unwrap();

        // 4. Create an Assistant with function calling tool
        let file_id_clone = file_id.clone();
        let assistant = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some("You help me by using the tools you have.".to_string()),
                name: Some("Purpose of Life universal calculator".to_string()),
                tools: vec![
                    AssistantTools::Function(AssistantToolsFunction {
                        r#type: "function".to_string(),
                        function: ChatCompletionFunctions {
                            description: Some("A function that compute the purpose of life according to the fundamental laws of the universe.".to_string()),
                            name: "compute_purpose_of_life".to_string(),
                            parameters: json!({
                                "type": "object",
                            }),
                        },
                    }),
                    AssistantTools::Retrieval(AssistantToolsRetrieval {
                        r#type: "retrieval".to_string(),
                    }),
                ],
                model: "claude-2.1".to_string(),
                file_ids: vec![file_id_clone],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("An assistant that computes the purpose of life based on the tools of the universe.".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string()
        };
        let assistant = create_assistant(&pool, &assistant).await.unwrap();

        // 5. Create a Thread
        let thread = create_thread(&pool, &Uuid::default().to_string())
            .await
            .unwrap();

        // 6. Add a Message to a Thread
        let content = vec![MessageContent::Text(MessageContentTextObject {
            r#type: "text".to_string(),
            text: TextData {
                value: 
                "I need to know the purpose of life, you can give me two answers. Please use the context you get from FILES and FUNCTIONS to answer my question. Do not base yourself on your own knowledge."
                    .to_string(),
                annotations: vec![],
            },
        })];
        let message = add_message_to_thread(
            &pool,
            &thread.inner.id,
            MessageRole::User,
            content,
            &Uuid::default().to_string(),
            None,
        )
        .await
        .unwrap();

        // 7. Run the Assistant
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let mut con = client.get_async_connection().await.unwrap();
        let run = run_assistant(
            &pool,
            &thread.inner.id,
            &assistant.inner.id,
            "You help me by using the tools you have.",
            assistant.user_id.as_str(),
            con,
        )
        .await
        .unwrap();

        // 8. Check the result
        assert_eq!(run.inner.status, RunStatus::Queued);

        // 9. Run the queue consumer
        let mut con = client.get_async_connection().await.unwrap();
        let result = queue_consumer(&pool, &mut con).await;

        // 10. Check the result
        assert!(result.is_ok(), "{:?}", result);

        // 11. Fetch the run from the database and check its status
        let run = get_run(
            &pool,
            &thread.inner.id,
            &result.unwrap().inner.id,
            &assistant.user_id,
        )
        .await
        .unwrap();
        assert_eq!(run.inner.status, RunStatus::RequiresAction);

        // 12. Submit tool outputs
        let tool_outputs = vec![SubmittedToolCall {
            id: run
                .inner
                .required_action
                .unwrap()
                .submit_tool_outputs
                .tool_calls[0]
                .id
                .clone(),
            output: "The purpose of life is 42.".to_string(),
            run_id: run.inner.id.clone(),
            created_at: 0,
            user_id: assistant.user_id.clone(),
        }];
        submit_tool_outputs(
            &pool,
            &thread.inner.id,
            &run.inner.id,
            assistant.user_id.clone().as_str(),
            tool_outputs,
            con,
        )
        .await
        .unwrap();

        // 13. Run the queue consumer again
        let mut con = client.get_async_connection().await.unwrap();
        let result = queue_consumer(&pool, &mut con).await;

        // 14. Check the result
        assert!(result.is_ok(), "{:?}", result);

        // 15. Fetch the run from the database and check its status
        let run = get_run(
            &pool,
            &thread.inner.id,
            &result.unwrap().inner.id,
            &assistant.user_id,
        )
        .await
        .unwrap();
        assert_eq!(run.inner.status, RunStatus::Completed);

        // 16. Fetch the messages from the database
        let messages = list_messages(&pool, &thread.inner.id, &assistant.user_id)
            .await
            .unwrap();

        // 17. Check the messages
        assert_eq!(messages.len(), 2);
        if let MessageContent::Text(text_object) = &messages[0].inner.content[0] {
            assert_eq!(
                text_object.text.value,
                "I need to know the purpose of life, you can give me two answers. Please use the context you get from FILES and FUNCTIONS to answer my question. Do not base yourself on your own knowledge."
            );
        } else {
            panic!("Expected a Text message, but got something else.");
        }
        if let MessageContent::Text(text_object) = &messages[1].inner.content[0] {
            assert_eq!(text_object.text.value.contains("42"), true, "The assistant should have retrieved the ultimate truth of the universe. Instead, it retrieved: {}", text_object.text.value);
            assert_eq!(text_object.text.value.contains("43"), true, "The assistant should have retrieved the ultimate truth of the universe. Instead, it retrieved: {}", text_object.text.value);
        } else {
            panic!("Expected a Text message, but got something else.");
        }

        assert_eq!(messages[1].inner.role, MessageRole::Assistant);

    }
}
