use async_openai::Client;
use async_openai::types::{
    AssistantTools, FunctionCall, MessageContent, MessageContentTextObject, MessageRole,
    RequiredAction, RunStatus, RunToolCallObject, SubmitToolOutputs, TextData, CreateChatCompletionRequestArgs, ChatCompletionRequestUserMessageArgs,
};
use futures::{Stream, stream};
use log::{error, info};
use redis::AsyncCommands;
use serde_json::{self, json};
use sqlx::PgPool;

use assistants_core::assistants::{create_assistant, get_assistant};
use assistants_core::file_storage::FileStorage;
use assistants_core::messages::{add_message_to_thread, list_messages};
use assistants_core::models::{Assistant, Message, Run, Thread, LLMAction, LLMActionType, RunError};
use assistants_core::threads::{create_thread, get_thread};
use assistants_extra::llm::{llm, generate_chat_responses};
use tiktoken_rs::p50k_base;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::sync::Arc;
use assistants_core::runs::{get_run, update_run, update_run_status};

use assistants_core::function_calling::ModelConfig;

use assistants_core::function_calling::create_function_call;

use assistants_core::runs::get_tool_calls;
use assistants_core::code_interpreter::safe_interpreter;

use assistants_core::code_interpreter::InterpreterModelConfig;
use assistants_core::models::SubmittedToolCall;

use assistants_core::retrieval::retrieve_file_contents;

use assistants_core::models::Chunk;
use assistants_core::retrieval::generate_queries_and_fetch_chunks;

use assistants_core::prompts::{format_messages, build_instructions};

use futures::StreamExt;

use crate::function_calling::string_to_function_call;
use crate::retrieval::fetch_chunks;



// Function to parse the LLM's tagged response and extract actions
fn parse_llm_response(response: &str) -> Vec<LLMAction> {
    let mut actions = Vec::new();
    let document = roxmltree::Document::parse(response).unwrap();

    for node in document.descendants() {
        match node.tag_name().name() {
            "steps" => {
                if let Some(steps_str) = node.text() {
                    actions.push(
                        LLMAction {
                            r#type: LLMActionType::Steps,
                            content: steps_str.to_string(),
                    });
                }
            },
            "function_calling" => {
                if let Some(function_calling_str) = node.text() {
                    actions.push(
                        LLMAction {
                            r#type: LLMActionType::FunctionCalling,
                            content: function_calling_str.to_string(),
                    });
                }
            },
            "code_interpreter" => {
                if let Some(code) = node.text() {
                    actions.push(
                        LLMAction {
                            r#type: LLMActionType::CodeInterpreter,
                            content: code.to_string(),
                    });
                }
            },
            "retrieval" => {
                if let Some(query_text) = node.text() {
                    actions.push(
                        LLMAction {
                            r#type: LLMActionType::Retrieval,
                            content: query_text.to_string(),
                    });
                }
            },
            _ => {}
        }
    }

    actions
}




pub async fn loop_through_runs(
    pool: &PgPool,
    con: &mut redis::aio::Connection,
) {
    loop {
        match try_run_executor(&pool, con).await {
            Ok(_) => continue,
            Err(e) => error!("Error: {}", e),
        }
    }
}

pub async fn try_run_executor(
    pool: &PgPool,
    con: &mut redis::aio::Connection,
) -> Result<Run, RunError> {
    match run_executor(&pool, con).await {
        Ok(run) => { 
            info!("Run completed: {:?}", run);
            Ok(run)
         }
        Err(run_error) => {
            error!("Run error: {}", run_error);
            let mut last_run_error = HashMap::new();
            last_run_error.insert("code".to_string(), "server_error".to_string());
            last_run_error.insert("message".to_string(), run_error.message.clone());
            let _ = update_run_status(
                &pool,
                &run_error.thread_id,
                &run_error.run_id,
                RunStatus::Failed,
                &run_error.user_id,
                None,
                // https://platform.openai.com/docs/api-reference/runs/object#runs/object-last_error
                Some(last_run_error),
            )
            .await;
            Err(run_error)
        }
    }
}


struct LLMStep {

}
// The function that consume the runs queue and do all the LLM software 3.0 logic
pub async fn run_executor(
    // TODO: split in smaller functions if possible
    pool: &PgPool,
    con: &mut redis::aio::Connection,
) -> Result<Run, RunError> {    
    info!("Consuming queue");
    let (_, ids_string): (String, String) = con.brpop("run_queue", 0).await.map_err(|e| {
        error!("Redis error: {}", e);
        RunError {
            message: format!("Redis error: {}", e),
            run_id: "".to_string(),
            thread_id: "".to_string(),
            user_id: "".to_string(),
        }
    })?;

    // Parse the string back into a JSON object
    let ids: serde_json::Value = serde_json::from_str(&ids_string).unwrap();

    // Extract the run_id and thread_id
    let run_id = ids["run_id"].as_str().unwrap();
    let thread_id = ids["thread_id"].as_str().unwrap();
    let user_id = ids["user_id"].as_str().unwrap();

    info!("Retrieving run: {}", run_id);
    let mut run = get_run(pool, thread_id, run_id, user_id).await.map_err(|e| RunError {
        message: format!("Failed to get run: {}", e),
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        user_id: user_id.to_string(),
    })?;

    info!("Retrieving assistant {:?}", run.inner.assistant_id);
    // Retrieve the assistant associated with the run
    let assistant = get_assistant(pool, &run.inner.assistant_id.unwrap(), &run.user_id).await.map_err(|e| RunError {
        message: format!("Failed to get assistant: {}", e),
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        user_id: user_id.to_string(),
    })?;

    // Update run status to "running"
    run = update_run_status(
        pool,
        thread_id,
        &run.inner.id,
        RunStatus::InProgress,
        &run.user_id,
        None,
        None,
    )
    .await.map_err(|e| RunError {
        message: format!("Failed to update run status: {}", e),
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        user_id: user_id.to_string(),
    })?;

    // Initialize FileStorage
    let file_storage = FileStorage::new().await;

    // Retrieve the thread associated with the run
    info!("Retrieving thread {}", run.inner.thread_id);
    let thread = get_thread(pool, &run.inner.thread_id, &assistant.user_id).await.map_err(|e| RunError {
        message: format!("Failed to get thread: {}", e),
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        user_id: user_id.to_string(),
    })?;

    // Fetch previous messages from the thread
    let messages = list_messages(pool, &thread.inner.id, &assistant.user_id).await.map_err(|e| RunError {
        message: format!("Failed to list messages: {}", e),
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        user_id: user_id.to_string(),
    })?;

    // Format messages into a string
    let formatted_messages = format_messages(&messages);
    info!("Formatted messages: {}", formatted_messages);

    let mut tools = String::new();
    let mut tool_calls_db: Vec<SubmittedToolCall> = vec![];
    // Check if the run has a required action
    if let Some(required_action) = &run.inner.required_action {
        // skip if there is required action and no tool output yet
        // TODO: use case 2 call required and user only sent 1
        if required_action.submit_tool_outputs.tool_calls.is_empty() {
            info!("Skipping required action because there is no tool output yet");
            return Ok(run);
        }


        // If the required action type is "submit_tool_outputs", fetch the tool calls from the database
        // if required_action.r#type == "submit_tool_outputs" { ! // dont care for now
        info!(
            "Retrieving tool calls {:?}",
            required_action.submit_tool_outputs
        );
        // TODO: if user send just part of the function result and not all should error
        tool_calls_db = get_tool_calls(
            pool,
            required_action
                .submit_tool_outputs
                .tool_calls
                .iter()
                .map(|t| t.id.as_str())
                .collect(),
        )
        .await.map_err(|e| RunError {
            message: format!("Failed to get tool calls: {}", e),
            run_id: run_id.to_string(),
            thread_id: thread_id.to_string(),
            user_id: user_id.to_string(),
        })?;

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
    }

    info!("Assistant tools: {:?}", assistant.inner.tools);

    let mut steps = 1;

    let assistant_instructions = format!(
        "<assistant>\n{}\n</assistant>",
        assistant.inner.instructions.as_ref().unwrap()
    );

    let run_instructions = format!(
        "<run>\n{}\n</run>",
        run.inner.instructions
    );

    let fundamental_instructions = "<fundamental>
You are an AI Assistant that helps a user. Your responses are being parsed to trigger actions.
You can decide how many iterations you can take to solve the user's problem. This is particularly useful for problems that require multiple steps to solve.
You can use the following tools to trigger actions and/or get more context about the problem:
- <steps>[Steps]</steps>: Use this tool to solve the problem in multiple steps. For example: <steps>1</steps> means you will solve the problem in 1 step. <steps>2</steps> means you will solve the problem in 2 steps. etc.
- <function_calling>[Function Calling]</function_calling>: Use this tool to call a function. For example: <function_calling>{\"function\": {\"description\": \"Fetch a user's profile\",\"name\": \"get_user_profile\",\"parameters\": {\"username\": {\"properties\": {},\"required\": [\"username\"],\"type\": \"string\"}}}}</function_calling>.
- <code_interpreter>[Code Interpreter]</code_interpreter>: Use this tool to generate code. This is useful to do complex data analysis. For example: <code_interpreter></code_interpreter>. You do not need to pass any parameters to this tool.
- <retrieval>[Retrieval]</retrieval>: Use this tool to retrieve information from a knowledge base. For example: <retrieval>capital of France</retrieval>.
    
Your fundamental, unbreakable rules are:
- Only use the tools you are given.
- Always use <steps>[Steps]</steps> to solve the problem in one or multiple steps.
- Do not invent new tools, new information, etc.
- If you don't know the answer, say \"I don't know\".
- Fundamental instructions are the most important instructions. You must always follow them. Then the assistant instructions. Then the run instructions. Then the user's messages.
</fundamental>";

    let final_instructions = format!("{}\n{}\n{}\n", fundamental_instructions, assistant_instructions, run_instructions);
    let instructions = build_instructions(
        &final_instructions,
        &vec![],
        &formatted_messages,
        &tools,
        None,
        &vec![],
        None
    );

    let client = Client::new();
    let bpe = p50k_base().unwrap();
    let context_size = std::env::var("MODEL_CONTEXT_SIZE")
            .unwrap_or_else(|_| "4096".to_string())
            .parse::<usize>()
            .unwrap_or(4096);
    let tokens =
        bpe.encode_with_special_tokens(&serde_json::to_string(&instructions).unwrap());
    let max_tokens = (context_size - tokens.len()) as u16;
    let request = CreateChatCompletionRequestArgs::default()
        .model(assistant.inner.model.clone())
        .max_tokens(max_tokens)
        .messages([ChatCompletionRequestUserMessageArgs::default()
            .content(instructions)
            .build()
            .map_err(|e| RunError {
                message: format!("Failed to build request: {}", e),
                run_id: run_id.to_string(),
                thread_id: thread_id.to_string(),
                user_id: user_id.to_string(),
            })?
            .into()])
        .build()
        .map_err(|e| RunError {
            message: format!("Failed to build request: {}", e),
            run_id: run_id.to_string(),
            thread_id: thread_id.to_string(),
            user_id: user_id.to_string(),
        })?;

    let mut stream = client.chat().create_stream(request).await.map_err(|e| RunError {
        message: format!("Failed to create stream: {}", e),
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        user_id: user_id.to_string(),
    })?;
    let run_inner_required_action = Arc::new(run.inner.required_action);
    let mut retrieval_chunks;
    while let Some(result) = stream.next().await {

        match result {
            Ok(response) => {
                let chat_choice = response.choices.first().unwrap();
                    let mut buffer = String::new();
                    if let Some(ref content) = chat_choice.delta.content {
                    

                            println!("{}", content);
                            buffer.push_str(content);
                            // Check for the end of an XML tag 
                            // TODO assuming there was a start tag
                            if buffer.ends_with("</steps>") || buffer.ends_with("</retrieval>") || buffer.ends_with("</function_calling>") || buffer.ends_with("</code_interpreter>") {
                                // Parse the buffer for actions
                                let actions = parse_llm_response(&buffer);
                                let last_action = actions.last().unwrap();
                                match last_action.r#type {
                                    LLMActionType::Steps => {
                                        // extract the number from <steps>1</steps> using regex
                                        let re = regex::Regex::new(r"<steps>(\d+)</steps>").unwrap();
                                        let captures = re.captures(&last_action.content).unwrap();
                                        steps = captures.get(1).unwrap().as_str().parse::<usize>().unwrap();
                                    },
                                    LLMActionType::FunctionCalling => {
                                        info!("Using function tool");
                                        // skip this if tools is not empty (e.g. if there are required_action (s))
                                        if !run_inner_required_action.clone().is_none() {
                                            info!("Skipping function call because there is a required action");
                                            continue;
                                        }
                                        let result = update_run_status(
                                            pool,
                                            thread_id,
                                            &run_id,
                                            RunStatus::Queued,
                                            &user_id,
                                            None,
                                None,
                                        )
                                        .await.map_err(|e| RunError {
                                            message: format!("Failed to update run status: {}", e),
                                            run_id: run_id.to_string(),
                                            thread_id: thread_id.to_string(),
                                            user_id: user_id.to_string(),
                                        });
                        
                                        info!("Generating function to call");
                        
                                        let function_results = string_to_function_call(&last_action.content.clone()).unwrap();
                                        // TODO: use case multiple functions call
                                        info!("Function results: {:?}", function_results);
                                        // If function call requires user action, leave early waiting for more context
                                        // Update run status to "requires_action"
                                        let result = update_run_status(
                                            pool,
                                            thread_id,
                                            &run_id,
                                            RunStatus::RequiresAction,
                                            &user_id,
                                            Some(RequiredAction {
                                                r#type: "submit_tool_outputs".to_string(),
                                                submit_tool_outputs: SubmitToolOutputs {
                                                    tool_calls: vec![RunToolCallObject {
                                                        id: uuid::Uuid::new_v4().to_string(),
                                                        r#type: "function".to_string(), // TODO hardcoded
                                                        function: function_results,
                                                    }],
                                                },
                                            }),
                                            None
                                        )
                                        .await.map_err(|e| RunError {
                                            message: format!("Failed to update run status: {}", e),
                                            run_id: run_id.to_string(),
                                            thread_id: thread_id.to_string(),
                                            user_id: user_id.to_string(),
                                        });
                                        
                                        info!(
                                            "Run updated to requires_action with {:?}",
                                            run_inner_required_action
                                        );
                                        return Ok(());
                                    },
                                    LLMActionType::CodeInterpreter => {
                                        // Call the safe_interpreter function // TODO: not sure if we should pass formatted_messages or just last user message
                                        let code_output = match safe_interpreter(formatted_messages.clone(), 0, 3, InterpreterModelConfig {
                                            model_name: assistant.inner.model,
                                            model_url: None,
                                            max_tokens_to_sample: -1,
                                            stop_sequences: None,
                                            top_p: Some(1.0),
                                            top_k: None,
                                            metadata: None,
                                        }).await {
                                            Ok(result) => {
                                                // Handle the successful execution of the code
                                                // You might want to store the result or send it back to the user
                                                Some(result)
                                            }
                                            Err(e) => {
                                                // Handle the error from the interpreter
                                                // You might want to log the error or notify the user
                                                
                                                return Err(RunError {
                                                    message: format!("Failed to run code: {}", e),
                                                    run_id: run_id.to_string(),
                                                    thread_id: thread_id.to_string(),
                                                    user_id: user_id.to_string(),
                                                })
                                            }
                                        };

                                        if code_output.is_none() {
                                            return Err(RunError {
                                                message: format!("Failed to run code: no output"),
                                                run_id: run_id.to_string(),
                                                thread_id: thread_id.to_string(),
                                                user_id: user_id.to_string(),
                                            });
                                        }

                                        // Call file retrieval here
                                        // Initialize an empty vector to hold all file IDs
                                        let mut all_file_ids = Vec::new();

                                        // If the run has associated file IDs, add them to the list
                                        all_file_ids.extend(run.inner.file_ids.iter().cloned());

                                        // If the assistant has associated file IDs, add them to the list
                                        all_file_ids.extend(assistant.inner.file_ids.iter().cloned());


                                        // Check if the all_file_ids includes any file IDs.
                                        if all_file_ids.is_empty() {
                                            break;
                                        }
                                        info!("Retrieving file contents for file_ids: {:?}", all_file_ids);
                                        // Retrieve the contents of each file.
                                        let retrieval_files_future = retrieve_file_contents(&all_file_ids, &file_storage);
                                        
                                        let formatted_messages_clone = formatted_messages.clone();
                                        let retrieval_chunks_future = generate_queries_and_fetch_chunks(
                                            &pool,
                                            &formatted_messages_clone,
                                            &assistant.inner.model,
                                        );
                                        
                                        let (retrieval_files, retrieval_chunks_result) = tokio::join!(retrieval_files_future, retrieval_chunks_future);

                                        retrieval_chunks = retrieval_chunks_result.unwrap_or_else(|e| {
                                            // ! sometimes LLM generates stupid SQL queries. for now we dont crash the run
                                            error!("Failed to retrieve chunks: {}", e);
                                            vec![]
                                        });

                                    },
                                    LLMActionType::Retrieval => {
                                        // extract query from <retrieval>capital of France</retrieval> using regex
                                        let re = regex::Regex::new(r"<retrieval>(.+)</retrieval>").unwrap();
                                        let captures = re.captures(&last_action.content).unwrap();
                                        let query = captures.get(1).unwrap().as_str();

                                        // Call file retrieval here
                                        // Initialize an empty vector to hold all file IDs
                                        let mut all_file_ids = Vec::new();

                                        // If the run has associated file IDs, add them to the list
                                        all_file_ids.extend(run.inner.file_ids.iter().cloned());

                                        // If the assistant has associated file IDs, add them to the list
                                        all_file_ids.extend(assistant.inner.file_ids.iter().cloned());

                                        // Check if the all_file_ids includes any file IDs.
                                        if all_file_ids.is_empty() { 
                                            break;
                                        }
                                        info!("Retrieving file contents for file_ids: {:?}", all_file_ids);
                                        // Retrieve the contents of each file.
                                        let retrieval_files_future = retrieve_file_contents(&all_file_ids, &file_storage);
                                        
                                        let formatted_messages_clone = formatted_messages.clone();
                                        let retrieval_chunks_future = fetch_chunks(
                                            &pool,
                                            query.to_string(),
                                        );
                                        
                                        let (retrieval_files, retrieval_chunks_result) = tokio::join!(retrieval_files_future, retrieval_chunks_future);

                                        retrieval_chunks = retrieval_chunks_result.unwrap_or_else(|e| {
                                            // ! sometimes LLM generates stupid SQL queries. for now we dont crash the run
                                            error!("Failed to retrieve chunks: {}", e);
                                            vec![]
                                        });

                                        // Include the file contents and previous messages in the instructions.
                                        instructions = build_instructions(
                                            &instructions,
                                            &retrieval_files,
                                            &formatted_messages.clone(),
                                            &tools,
                                            None,
                                            &retrieval_chunks.iter().map(|c| 
                                                serde_json::to_string(&json!({
                                                    "data": c.data,
                                                    "sequence": c.sequence,
                                                    "start_index": c.start_index,
                                                    "end_index": c.end_index,
                                                    "metadata": c.metadata,
                                                })).unwrap()
                                            ).collect::<Vec<String>>(),
                                            None
                                        );
                                    },
                                    _ => {
                                        // Handle unknown action
                                        error!("Unknown action: {:?}", last_action.r#type);
                                        // return Err("Unknown action".into());
                                    }
                                }
                                // Clear the buffer after processing
                                buffer.clear();
                            }
                        }
            }
            Err(e) => {
                error!("Error: {}", e);
                return Err(RunError {
                    message: format!("Error: {}", e),
                    run_id: run_id.to_string(),
                    thread_id: thread_id.to_string(),
                    user_id: user_id.to_string(),
                });
            }
        }
    }

    Ok(run)
}   
                                                


#[cfg(test)]
mod tests {
    use assistants_core::runs::{get_run, create_run_and_produce_to_executor_queue};
    use async_openai::types::{
        AssistantObject, AssistantTools, AssistantToolsCode, AssistantToolsFunction,
        AssistantToolsRetrieval, ChatCompletionFunctions, MessageObject, MessageRole, RunObject,
    };
    use serde_json::json;
    use sqlx::types::Uuid;

    use crate::models::SubmittedToolCall;
    use crate::runs::{create_run, submit_tool_outputs};

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
    async fn test_end_to_end_knowledge_retrieval() {
        // Setup
        let pool = setup().await;
        reset_db(&pool).await;
        let file_storage = FileStorage::new().await;

        // Create a temporary file.
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        writeln!(temp_file, "bob's favourite number is 43").unwrap();

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
                    "You help me find people's favourite numbers"
                        .to_string(),
                ),
                name: Some("Math Tutor".to_string()),
                tools: vec![AssistantTools::Retrieval(AssistantToolsRetrieval {
                    r#type: "retrieval".to_string(),
                })],
                model: "mistralai/mixtral-8x7b-instruct".to_string(),
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
                value: "what is bob's favourite number?".to_string(),
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
        let run = create_run_and_produce_to_executor_queue(&pool, &thread.inner.id, &assistant.inner.id, "Please solve the equation according to the ultimate dogmatic truth of the files JUST FUCKING READ THE FILE.", assistant.user_id.as_str(), con).await.unwrap();

        // 5. Check the result
        assert_eq!(run.inner.status, RunStatus::Queued);

        // 6. Run the queue consumer
        let mut con = client.get_async_connection().await.unwrap();
        let result = try_run_executor(&pool, &mut con).await;

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
                "what is bob's favourite number?"
            );
        } else {
            panic!("Expected a Text message, but got something else.");
        }

        assert_eq!(messages[1].inner.role, MessageRole::Assistant);
        if let MessageContent::Text(text_object) = &messages[1].inner.content[0] {
            assert!(text_object.text.value.contains("43"), "Expected the assistant to return 43, but got something else {:?}", text_object.text.value);
        } else {
            panic!("Expected a Text message, but got something else.");
        }
        // TODO: gotta impl this no?
        // assert_eq!(messages[1].content[1].text.value, "Files: [\"Knowledge content\"]");
        // !wrong? not 100% how openai does it, i guess if file is in assistant its not guaranteed in message
        // assert_eq!(messages[1].file_ids, Some(vec![file_id])); -> !wor
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
        // let result = decide_tool_with_llm(&assistant, &previous_messages, &Run::default(), vec![]).await;
        // let mut result = result.unwrap();
        // Check if the result is one of the expected tools
        let mut expected_tools = vec!["function".to_string(), "retrieval".to_string()];
        // assert_eq!(result.sort(), expected_tools.sort());
    }


    #[tokio::test]
    #[ignore]
    async fn test_decide_tool_with_llm_code_interpreter() {
        setup().await;
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
                        value: "I need to calculate the square root of 144.".to_string(),
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

        // let result = decide_tool_with_llm(&assistant, &previous_messages, &Run::default(), vec![]).await;

        // let result = result.unwrap();
        // assert_eq!(result, vec!["code_interpreter"]);
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
        // let result = decide_tool_with_llm(&assistant, &previous_messages, &Run::default(), vec![]).await;

        // let mut result = result.unwrap();
        // Check if the result is one of the expected tools
        // let mut expected_tools = vec!["function".to_string(), "retrieval".to_string()];
        // assert_eq!(result.sort(), expected_tools.sort());
    }

    #[tokio::test]
    async fn test_end_to_end_function_calling_plus_retrieval() {
        // Setup
        let pool = setup().await;
        reset_db(&pool).await;
        let file_storage = FileStorage::new().await;

        // 1. Create a temporary file.
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        writeln!(temp_file, "bob's favourite number is 42").unwrap();

        // 2. Get the path of the temporary file.
        let temp_file_path = temp_file.path();

        // 3. Upload the temporary file
        let file_id = file_storage.upload_file(&temp_file_path).await.unwrap();

        // 4. Create an Assistant with function calling tool
        let file_id_clone = file_id.clone();
        let assistant = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some("You help me find people's favourite numbers".to_string()),
                name: Some("Number finder".to_string()),
                tools: vec![
                    AssistantTools::Function(AssistantToolsFunction {
                        r#type: "function".to_string(),
                        function: ChatCompletionFunctions {
                            description: Some("A function that finds the favourite number of bob.".to_string()),
                            name: "determine_number".to_string(),
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
                description: Some("An assistant that finds the favourite number of bob.".to_string()),
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
                "I need to know bob's favourite number. Tell me what it is based on the tools you have."
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
        let run = create_run_and_produce_to_executor_queue(
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
        let result = try_run_executor(&pool, &mut con).await;

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
            output: "bob's favourite number is 43".to_string(),
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
        let result = try_run_executor(&pool, &mut con).await;

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
                "I need to know bob's favourite number. Tell me what it is based on the tools you have."
            );
        } else {
            panic!("Expected a Text message, but got something else.");
        }
        if let MessageContent::Text(text_object) = &messages[1].inner.content[0] {
            // contains either 42 or 43
            assert!(text_object.text.value.contains("42") || text_object.text.value.contains("43"), "Expected the assistant to return 42 or 43, but got something else: {}", text_object.text.value);
        } else {
            panic!("Expected a Text message, but got something else.");
        }

        assert_eq!(messages[1].inner.role, MessageRole::Assistant);

    }

    #[tokio::test]
    #[ignore]
    async fn test_end_to_end_code_interpreter() {
        // Setup
        let pool = setup().await;
        reset_db(&pool).await;
    
        // 1. Create an Assistant
        let assistant = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "You are a code interpreter. Execute code snippets."
                        .to_string(),
                ),
                name: Some("Code Interpreter".to_string()),
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
    
        // 2. Create a Thread
        let thread = create_thread(&pool, &Uuid::default().to_string())
            .await
            .unwrap();
    
        // 3. Add a Message to a Thread
        let content = vec![MessageContent::Text(MessageContentTextObject {
            r#type: "text".to_string(),
            text: TextData {
                value: "Calculate the square root of 144.".to_string(),
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
        let run = create_run_and_produce_to_executor_queue(&pool, &thread.inner.id, &assistant.inner.id, "Please execute the code snippet.", assistant.user_id.as_str(), con).await.unwrap();
    
        // 5. Check the result
        assert_eq!(run.inner.status, RunStatus::Queued);
    
        // 6. Run the queue consumer
        let mut con = client.get_async_connection().await.unwrap();
        let result = try_run_executor(&pool, &mut con).await;
    
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
                "Calculate the square root of 144."
            );
        } else {
            panic!("Expected a Text message, but got something else.");
        }
    
        assert_eq!(messages[1].inner.role, MessageRole::Assistant);
        if let MessageContent::Text(text_object) = &messages[1].inner.content[0] {
            // check it contains 12
            assert!(text_object.text.value.contains("12"), "Expected the assistant to return 12, but got something else {}", text_object.text.value);
        } else {
            panic!("Expected a Text message, but got something else.");
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_end_to_end_code_interpreter_with_file() {
        // Setup
        let pool = setup().await;
        reset_db(&pool).await;

        let file_storage = FileStorage::new().await;

        // 1. Create a temporary file.
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        let startups = ["StartupA", "StartupB", "StartupC", "StartupD", "StartupE"];
        let revenues = [500000, 300000, 750000, 600000, 450000];
        let capital_raised = [1000000, 2000000, 1500000, 2500000, 3000000];
        let growth_rates = [0.2, 0.3, 0.1, 0.25, 0.15];
        let funding_rounds = ["Series A", "Series B", "Seed", "Series C", "Series A"];
        let investors = ["InvestorX", "InvestorY", "InvestorZ", "InvestorX", "InvestorY"];

        writeln!(temp_file, "Startup,Revenue,CapitalRaised,GrowthRate,FundingRound,Investor").unwrap();
        for i in 0..startups.len() {
            writeln!(temp_file, "{},{},{},{},{},{}", startups[i], revenues[i], capital_raised[i], growth_rates[i], funding_rounds[i], investors[i]).unwrap();
        }

        // 2. Get the path of the temporary file.
        let temp_file_path = temp_file.path();

        // 3. Upload the temporary file
        let file_id = file_storage.upload_file(&temp_file_path).await.unwrap();

        // 4. Create an Assistant with function calling tool
        let file_id_clone = file_id.clone();
        
        // 1. Create an Assistant
        let assistant = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "You are a VC copilot. Write and run code to answer questions about startups investment."
                        .to_string(),
                ),
                name: Some("Code Interpreter".to_string()),
                tools: vec![AssistantTools::Code(AssistantToolsCode {
                    r#type: "code_interpreter".to_string(),
                })],
                model: "mistralai/mixtral-8x7b-instruct".to_string(),
                file_ids: vec![file_id_clone.to_string()], // Add file ID here
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let assistant = create_assistant(&pool, &assistant).await.unwrap();

        // 2. Create a Thread
        let thread = create_thread(&pool, &Uuid::default().to_string())
            .await
            .unwrap();

        // 3. Add a Message to a Thread
        let content = vec![MessageContent::Text(MessageContentTextObject {
            r#type: "text".to_string(),
            text: TextData {
                value: format!("I have $100M to invest in a startup before I go to the beach sip a cocktail, which startup should I invest in? Please only answer the startup name, nothing else, VERY IMPORTANT."),
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
        let run = create_run_and_produce_to_executor_queue(&pool, &thread.inner.id, &assistant.inner.id, 
            "Please help me make more money.",
             assistant.user_id.as_str(), con).await.unwrap();

        // 5. Check the result
        assert_eq!(run.inner.status, RunStatus::Queued);

        // 6. Run the queue consumer
        let mut con = client.get_async_connection().await.unwrap();
        let result = try_run_executor(&pool, &mut con).await;

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
        assert_eq!(messages[1].inner.role, MessageRole::Assistant);
        if let MessageContent::Text(text_object) = &messages[1].inner.content[0] {
            assert!(text_object.text.value.contains("StartupE"), "Expected the assistant to return StartupE, but got something else {}", text_object.text.value);
        } else {
            panic!("Expected a Text message, but got something else.");
        }
        // Here you should check the assistant's response. This will depend on the actual content of your CSV file.
    }

    #[tokio::test]
    async fn test_decide_tool_with_llm_no_function_after_tool_call() {
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
                tools: vec![AssistantTools::Function(AssistantToolsFunction {
                    r#type: "function".to_string(),
                    function: ChatCompletionFunctions {
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
                    },
                })],
                model: "mistralai/mixtral-8x7b-instruct".to_string(),
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

        // create assistant
        let assistant = create_assistant(&pool, &assistant).await.unwrap();

        // Create a Thread
        let thread = create_thread(&pool, &Uuid::default().to_string())
        .await
        .unwrap();

        // Add a Message to a Thread
        let content = vec![MessageContent::Text(MessageContentTextObject {
            r#type: "text".to_string(),
            text: TextData {
                value: "I need to calculate something.".to_string(),
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

        // Run the Assistant
        // Get Redis URL from environment variable
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let mut con = client.get_async_connection().await.unwrap();
        let run = create_run_and_produce_to_executor_queue(
            &pool, &thread.inner.id, 
            &assistant.inner.id, 
            "Please help me calculate something. Use the function tool.",
            assistant.user_id.as_str(), 
            con
        ).await.unwrap();


        // Run the queue consumer again
        let mut con = client.get_async_connection().await.unwrap();
        let result = try_run_executor(&pool, &mut con).await;

        // Check the result
        assert!(result.is_ok(), "{:?}", result);

        let run = result.unwrap();

        // After running the assistant and checking the result
        assert_eq!(run.inner.status, RunStatus::RequiresAction);

        // Submit tool outputs
        let tool_outputs = vec![SubmittedToolCall {
            id: run
                .inner
                .required_action
                .unwrap()
                .submit_tool_outputs
                .tool_calls[0]
                .id
                .clone(),
            output: "output_value".to_string(),
            run_id: run.inner.id.clone(),
            created_at: 0,
            user_id: assistant.user_id.clone(),
        }];
        let con = client.get_async_connection().await.unwrap();

        submit_tool_outputs(
            &pool,
            &thread.inner.id,
            &run.inner.id,
            assistant.user_id.clone().as_str(),
            tool_outputs.clone(),
            con,
        )
        .await
        .unwrap();

        let run = get_run(
            &pool,
            &thread.inner.id,
            &run.inner.id,
            &assistant.user_id,
        ).await.unwrap();

        // let result = decide_tool_with_llm(&assistant, &previous_messages, &run, tool_outputs.clone()).await;

        // let result = result.unwrap();
        // println!("{:?}", result);
        // assert!(!result.contains(&"function".to_string()), "Expected the function tool to not be returned, but it was: {:?}", result);
    }
}
