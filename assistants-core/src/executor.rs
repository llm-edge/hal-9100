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
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use tiktoken_rs::p50k_base;
use assistants_core::runs::{get_run, update_run, update_run_status};

use assistants_core::function_calling::ModelConfig;

use assistants_core::function_calling::create_function_call;

use assistants_core::runs::get_tool_calls;
use assistants_core::code_interpreter::safe_interpreter;

use assistants_core::code_interpreter::InterpreterModelConfig;
use assistants_core::models::SubmittedToolCall;

use assistants_core::retrieval::retrieve_file_contents;

use crate::models::Chunk;
use crate::retrieval::generate_queries_and_fetch_chunks;


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

/// Builds the instructions for the assistant.
///
/// This function takes several arguments, constructs parts of the instructions separately, and then
/// combines them into the final instructions string based on their priority and the context size limit.
///
/// # Arguments
///
/// * `original_instructions` - The original instructions string.
/// * `file_contents` - A vector of strings representing the file contents.
/// * `previous_messages` - A string representing the previous messages.
/// * `tools` - A string representing the tools.
/// * `code_output` - An optional string representing the code output.
/// * `context_size` - The context size limit for the language model.
/// * `retrieval_chunks` - A vector of strings representing the retrieval chunks.
///
/// # Returns
///
/// * A string representing the final instructions for the assistant.
///
/// # Note
///
/// The function uses the `tiktoken_rs` library to count the tokens in the instructions.
/// The parts of the instructions are added to the final instructions string based on their priority.
/// The order of priority (from highest to lowest) is: original instructions, tools, code output,
/// previous messages, file contents, and retrieval chunks.
/// If a part doesn't fit within the context size limit, it is not added to the final instructions.
fn build_instructions(
    original_instructions: &str,
    file_contents: &Vec<String>,
    previous_messages: &str,
    tools: &str,
    code_output: Option<&str>,
    retrieval_chunks: &Vec<String>,
    context_size: Option<usize>,
) -> String {
    let bpe = p50k_base().unwrap();

    // if context_size is None, use env var or default to x
    let context_size = context_size.unwrap_or_else(|| {
        std::env::var("MODEL_CONTEXT_SIZE")
            .unwrap_or_else(|_| "4096".to_string())
            .parse::<usize>()
            .unwrap_or(4096)
    });

    // Build each part of the instructions
    let instructions_part = format!("<instructions>\n{}\n</instructions>\n", original_instructions);
    let file_contents_part = format!("<file>\n{:?}\n</file>\n", file_contents);
    let retrieval_chunks_part = format!("<chunk>\n{:?}\n</chunk>\n", retrieval_chunks);
    let tools_part = format!("<tools>\n{}\n</tools>\n", tools);
    let code_output_part = match code_output {
        Some(output) => format!("<math_solution>\n{}\n</math_solution>\n", output),
        None => String::new(),
    };
    let previous_messages_part = format!("<previous_messages>\n{}\n</previous_messages>", previous_messages);

    // Initialize the final instructions with the highest priority part
    let mut final_instructions = instructions_part;

    // List of other parts ordered by priority
    let mut other_parts = [tools_part, previous_messages_part, code_output_part, file_contents_part.clone(), retrieval_chunks_part.clone()];
    // TODO: probably this could be made customisable if someone has a usecase where code is very important for example

    // Add other parts to the final instructions if they fit in the context limit
    for part in &other_parts {
        let part_tokens = bpe.encode_with_special_tokens(part).len();
        let final_instructions_tokens = bpe.encode_with_special_tokens(&final_instructions).len();

        if final_instructions_tokens + part_tokens <= context_size {
            // If file_contents_part is already in final_instructions, do not add retrieval_chunks_part
            if part == &retrieval_chunks_part && final_instructions.contains(&retrieval_chunks_part) {
                continue;
            }
            final_instructions += part;
        } else {
            break;
        }
    }

    final_instructions
}

pub async fn decide_tool_with_llm(
    assistant: &Assistant,
    previous_messages: &[Message],
    run: &Run,
    tool_calls_db: Vec<SubmittedToolCall>
) -> Result<Vec<String>, Box<dyn Error>> {
    // Build the system prompt
    let system_prompt = "You are an assistant that decides which tool to use based on a list of tools to solve the user problem.

Rules:
- You only return one of the tools like \"<retrieval>\" or \"<function>\" or \"<code_interpreter>\" or multiple of them
- Do not return \"tools\"
- Feel free to use MORE tools rather than LESS
- Tools use snake_case, not camelCase
- The tool names must be one of the tools available, nothing else OR A HUMAN WILL DIE
- Your answer must be very concise and make sure to surround the tool by <>, do not say anything but the tool name with the <> around it.
- If you do not obey a human will die

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

Another example:
<user>
<tools>{\"description\":\"useful to call functions in the user's product, which would provide you later some additional context about the user's problem\",\"function\":{\"arguments\":{\"type\":\"object\"},\"description\":\"A function that compute the purpose of life according to the fundamental laws of the universe.\",\"name\":\"compute_purpose_of_life\"},\"name\":\"function\"}
---
{\"description\":\"useful to retrieve information from files\",\"name\":\"retrieval\"}
---
{\"description\":\"useful for performing complex math problems which LLMs are bad at by default\",\"name\":\"code_interpreter\"}</tools>

<previous_messages>User: [Text(MessageContentTextObject { type: \"text\", text: TextData { value: \"I need to calculate the square root of 144.\", annotations: [] } })]
</previous_messages>

<instructions>You help me by using the tools you have.</instructions>

</user>

In this example, the assistant should return \"<code_interpreter>\".

Your answer will be used to use the tool so it must be very concise and make sure to surround the tool by <>, do not say anything but the tool name with the <> around it.";

    let tools = assistant.inner.tools.clone();
    println!("tools: {:?}", tools);
    // Build the user prompt
    let tools_as_string = tools
        .iter()
        .map(|t| {
            serde_json::to_string(&match t {
                AssistantTools::Code(_) => json!({"name": "code_interpreter", "description": "useful for performing complex math problems which LLMs are bad at by default. Do not use code_interpreter if it's simple math that you believe a LLM can do (e.g. 1 + 1, 9 * 7, etc.) - Make sure to use code interpreter for more complex math problems"}),
                AssistantTools::Retrieval(_) => json!({"name": "retrieval", "description": "useful to retrieve information from files"}),
                AssistantTools::Function(e) => 
                    json!({
                        "name": "function",
                        "description": "Useful to call functions in the user's product, which would provide you later some additional context about the user's problem. You can also use this to perform actions in the user's product.",
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
        -1,        // max_tokens_to_sample
        None,      // stop_sequences
        Some(1.0), // top_p
        None,      // top_k
        None,      // metadata
        None,      // metadata
    )
    .await?;

    info!("decide_tool_with_llm raw result: {}", result);

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

    // remove non alphanumeric chars and keep underscores
    results = results
        .iter()
        .map(|r| {
            r.chars()
                .filter(|c| c.is_alphanumeric() || *c == '_')
                .collect::<String>()
        })
        .collect::<Vec<String>>();

    // Check if the length of tool_calls_db is equal to the length of required_action output
    if let Some(required_action) = &run.inner.required_action {
        // Compare all ids from required action outputs and tool calls ids
        let required_ids: HashSet<_> = required_action.submit_tool_outputs.tool_calls.iter().map(|t| t.clone().id)
        .collect();
        let tool_calls_ids: HashSet<_> = tool_calls_db.iter().map(|t| t.clone().id)
        .collect();
        if required_ids.is_subset(&tool_calls_ids) {
            // If all tool calls have been done, remove function from tools_decision
            results.retain(|tool| tool != "function");
        } 
    }

    info!("decide_tool_with_llm result: {:?}", results);

    Ok(results
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>())
}

pub struct RunError {
    pub message: String,
    pub run_id: String,
    pub thread_id: String,
    pub user_id: String,
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::fmt::Debug for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl std::error::Error for RunError {}

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
            let _ = update_run_status(
                &pool,
                &run_error.thread_id,
                &run_error.run_id,
                RunStatus::Failed,
                &run_error.user_id,
                None,
            )
            .await;
            Err(run_error)
        }
    }
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

    info!("Retrieving run");
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
    info!("Asking LLM to decide which tool to use");

    // Decide which tool to use
    let mut tools_decision = decide_tool_with_llm(&assistant, &messages, &run, tool_calls_db).await.map_err(|e| RunError {
        message: format!("Failed to decide tool: {}", e),
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        user_id: user_id.to_string(),
    })?;

    info!("Tools decision: {:?}", tools_decision);

    let mut instructions = build_instructions(
        &run.inner.instructions,
        &vec![],
        &formatted_messages,
        &tools,
        None,
        &vec![],
        None
    );

    let model = assistant.inner.model.clone();
    let url = std::env::var("MODEL_URL")
        .unwrap_or_else(|_| String::from("http://localhost:8000/v1/chat/completions"));
    // Call create_function_call here
    let model_config = ModelConfig {
        model_name: model.clone(),
        model_url: url.clone().into(),
        user_prompt: formatted_messages.clone(), // TODO: assuming this is the user prompt. Should it be just last message? Or more custom?
        temperature: Some(0.0),
        max_tokens_to_sample: 200,
        stop_sequences: None,
        top_p: Some(1.0),
        top_k: None,
        metadata: None,
    };

    // Sort the tools_decision so that "function" comes first if present
    tools_decision.sort_by(|a, b| {
        if a == "function" {
            Ordering::Less
        } else if b == "function" {
            Ordering::Greater
        } else {
            a.cmp(b)
        }
    });

    let mut retrieval_files: Vec<String> = vec![];
    let mut retrieval_chunks: Vec<Chunk> = vec![];

    // Iterate over the sorted tools_decision
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
                .await.map_err(|e| RunError {
                    message: format!("Failed to update run status: {}", e),
                    run_id: run_id.to_string(),
                    thread_id: thread_id.to_string(),
                    user_id: user_id.to_string(),
                })?;

                info!("Generating function to call");

                let function_results =
                    create_function_call(&pool, user_id, model_config.clone()).await.map_err(|e| RunError {
                        message: format!("Failed to create function call: {}", e),
                        run_id: run_id.to_string(),
                        thread_id: thread_id.to_string(),
                        user_id: user_id.to_string(),
                    })?;

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
                    .await.map_err(|e| RunError {
                        message: format!("Failed to update run status: {}", e),
                        run_id: run_id.to_string(),
                        thread_id: thread_id.to_string(),
                        user_id: user_id.to_string(),
                    })?;
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
                
            }
            "code_interpreter" => {
                // Call the safe_interpreter function // TODO: not sure if we should pass formatted_messages or just last user message
                let code_output = match safe_interpreter(formatted_messages.clone(), 0, 3, InterpreterModelConfig {
                    model_name: model.clone(),
                    model_url: url.clone().into(),
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

                // Build instructions with the code output
                instructions = build_instructions(
                    &instructions,
                    &retrieval_files,
                    &formatted_messages,
                    &tools,
                    code_output.as_deref(),
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
            }
            _ => {
                // Handle unknown tool
                error!("Unknown tool: {}", tool_decision);
                return Err(RunError {
                    message: format!("Unknown tool: {}", tool_decision),
                    run_id: run_id.to_string(),
                    thread_id: thread_id.to_string(),
                    user_id: user_id.to_string(),
                });
            }
        }
    }

    info!("Calling LLM API with instructions: {}", instructions);

    // Less prompt is more - just making sure the LLM does not talk too much about his context but rather directly answer the user TODO: (should be configurable)
    let system_prompt = format!("You are an assistant that help a user based on tools and context you have.

Rules:
- Do not hallucinate
- Obey strictly to the user request e.g. in <message> tags - EXTREMELY IMPORTANT
- Answer directly the user e.g. 'What is the solution to the equation \"x + 2 = 4\"?' You should answer \"x = 2\" even though receiving bunch of context before.
- Do not add tags in your answer such as <function_calls> etc. nor continue the user sentence. Just answer the user.

These are additional instructions from the user that you must obey absolutely:

{}

", assistant.inner.instructions.as_ref().unwrap_or(&"".to_string()));

    let result = llm(
        &assistant.inner.model,
        url.clone().into(),
        &system_prompt,
        &instructions,
        None, // temperature
        -1,
        None,      // stop_sequences
        None, // top_p
        None,      // top_k
        None,      // metadata
        None,      // metadata
    ).await;

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
            .await.map_err(|e| RunError {
                message: format!("Failed to add message to thread: {}", e),
                run_id: run_id.to_string(),
                thread_id: thread_id.to_string(),
                user_id: user_id.to_string(),
            })?;
            // Update run status to "completed"
            run = update_run_status(
                pool,
                &thread.inner.id,
                &run.inner.id,
                RunStatus::Completed,
                user_id,
                None,
            )
            .await.map_err(|e| RunError {
                message: format!("Failed to update run status: {}", e),
                run_id: run_id.to_string(),
                thread_id: thread_id.to_string(),
                user_id: user_id.to_string(),
            })?;
            Ok(run)
        }
        Err(e) => {
            error!("Assistant model error: {}", e);
            Err(RunError {
                message: format!("Assistant model error: {}", e),
                run_id: run_id.to_string(),
                thread_id: thread_id.to_string(),
                user_id: user_id.to_string(),
            })
        }
    }
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



    #[test]
    fn test_build_instructions_context_limit() {
        let original_instructions = "Solve the quadratic equation x^2 + 5x + 6 = 0.";
        let file_contents = vec![
            "# Python script to solve quadratic equations\nimport cmath\ndef solve_quadratic(a, b, c):\n    # calculate the discriminant\n    d = (b**2) - (4*a*c)\n    # find two solutions\n    sol1 = (-b-cmath.sqrt(d))/(2*a)\n    sol2 = (-b+cmath.sqrt(d))/(2*a)\n    return sol1, sol2\n".to_string(),
            "# Another Python script\nprint('Hello, world!')\n".to_string(),
        ];
        let previous_messages = "<message>\n{\"role\": \"user\", \"content\": \"Can you solve a quadratic equation for me?\"}\n</message>\n<message>\n{\"role\": \"assistant\", \"content\": \"Sure, I can help with that. What's the equation?\"}\n</message>\n";
        let tools = "code_interpreter";
        let code_output = Some("The solutions are (-2+0j) and (-3+0j)");
        let context_size = 200; // Set a realistic context size
        let retrieval_chunks = vec![
            "Here's a chunk of text retrieved from a large document...".to_string(),
            "And here's another chunk of text...".to_string(),
        ];
    
        let instructions = build_instructions(
            original_instructions,
            &file_contents,
            previous_messages,
            tools,
            code_output,
            &retrieval_chunks,
            Some(context_size),
        );
    
        // Use tiktoken to count tokens
        let bpe = p50k_base().unwrap();
        let tokens = bpe.encode_with_special_tokens(&instructions);
    
        // Check that the instructions do not exceed the context limit
        assert!(tokens.len() <= context_size, "The instructions exceed the context limit");
    
        // Check that the instructions contain the most important parts
        assert!(instructions.contains(original_instructions), "The instructions do not contain the original instructions");
        assert!(instructions.contains(tools), "The instructions do not contain the tools");
        assert!(instructions.contains(previous_messages), "The instructions do not contain the previous messages");

        // Check that the instructions do not contain the less important parts
        assert!(!instructions.contains(&file_contents[0]), "The instructions contain the file contents");
        assert!(!instructions.contains(&retrieval_chunks[0]), "The instructions contain the retrieval chunks");
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
        let result = decide_tool_with_llm(&assistant, &previous_messages, &Run::default(), vec![]).await;
        let mut result = result.unwrap();
        // Check if the result is one of the expected tools
        let mut expected_tools = vec!["function".to_string(), "retrieval".to_string()];
        assert_eq!(result.sort(), expected_tools.sort());
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

        let result = decide_tool_with_llm(&assistant, &previous_messages, &Run::default(), vec![]).await;

        let result = result.unwrap();
        assert_eq!(result, vec!["code_interpreter"]);
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
        let result = decide_tool_with_llm(&assistant, &previous_messages, &Run::default(), vec![]).await;

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

        let result = decide_tool_with_llm(&assistant, &previous_messages, &run, tool_outputs.clone()).await;

        let result = result.unwrap();
        println!("{:?}", result);
        assert!(!result.contains(&"function".to_string()), "Expected the function tool to not be returned, but it was: {:?}", result);
    }
}
