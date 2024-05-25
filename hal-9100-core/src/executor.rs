use async_openai::types::{
    AssistantTools, FunctionCall, MessageContent, MessageContentTextObject, MessageRole,
    RequiredAction, RunStatus, RunToolCallObject, SubmitToolOutputs, TextData, RunStepType, StepDetails, RunStepDetailsMessageCreationObject, MessageCreation, RunStepDetailsToolCallsObject, RunStepDetailsToolCalls, RunStepDetailsToolCallsCodeObject, CodeInterpreter, CodeInterpreterOutput, RunStepDetailsToolCallsCodeOutputLogsObject, RunStepDetailsToolCallsRetrievalObject, RunStepDetailsToolCallsFunctionObject, RunStepFunctionObject,
};
use futures::future::try_join_all;
use hal_9100_extra::llm::{HalLLMClient, HalLLMRequestArgs};
use log::{error, info};
use redis::AsyncCommands;
use serde_json::{self, json};
use sqlx::PgPool;

use hal_9100_core::assistants::{get_assistant};
use hal_9100_core::file_storage::minio_storage::MinioStorage;
use hal_9100_core::messages::{add_message_to_thread, list_messages};
use hal_9100_core::models::{Assistant, Message, Run};
use hal_9100_core::threads::{get_thread};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use hal_9100_core::runs::{get_run, update_run_status};


use hal_9100_core::function_calling::create_function_call;

use hal_9100_core::runs::get_tool_calls;
use hal_9100_core::code_interpreter::safe_interpreter;

use hal_9100_core::models::SubmittedToolCall;

use hal_9100_core::retrieval::retrieve_file_contents;

use hal_9100_core::models::Chunk;
use hal_9100_core::retrieval::generate_queries_and_fetch_chunks;

use crate::file_storage::file_storage::FileStorage;
use crate::function_calling::execute_request;
use crate::models::{RunStep};
use crate::openapi::ActionRequest;
use crate::prompts::{format_messages, build_instructions};
use crate::run_steps::{create_step, update_step, list_steps, set_all_steps_status};

pub fn extract_step_id_and_function_output(steps: Vec<RunStep>, tool_calls: Vec<SubmittedToolCall>) -> Vec<(String, String, RunStepFunctionObject)> {
    let mut result = Vec::new();

    for step in steps {
        if let StepDetails::ToolCalls(step_details) = &step.inner.step_details {
            // result.push((step.inner.id))
            for step_tool_call in &step_details.tool_calls {
                if let RunStepDetailsToolCalls::Function(step_function) = step_tool_call {
                    for tool_call in tool_calls.iter() {
                        if step_function.id == tool_call.id {
                            result.push((step.inner.id.clone(), tool_call.id.clone(), RunStepFunctionObject {
                                name: step_function.function.name.clone(),
                                arguments: step_function.function.arguments.clone(),
                                output: Some(tool_call.output.clone()),
                            }));
                        }
                    }
                }
            }
        }
    }

    result
}

#[derive(Debug)]
pub enum DecideToolError {
    // JsonError(serde_json::Error),
    // SqlxError(sqlx::Error),
    Other(String),
}

impl fmt::Display for DecideToolError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            // FunctionCallError::JsonError(e) => write!(f, "JSON error: {}", e),
            // FunctionCallError::SqlxError(e) => write!(f, "SQLx error: {}", e),
            DecideToolError::Other(e) => write!(f, "Other error: {}", e),
        }
    }
}

impl std::error::Error for DecideToolError {}


pub async fn decide_tool_with_llm(
    assistant: &Assistant,
    previous_messages: &[Message],
    run: &Run,
    tool_calls_db: Vec<SubmittedToolCall>,
    mut client: HalLLMClient,
    mut request: HalLLMRequestArgs,
) -> Result<Vec<String>, DecideToolError> {

    // if there are no tools, return empty
    if assistant.inner.tools.is_empty() {
        return Ok(vec![]);
    }

    // Build the system prompt
    let system_prompt = "You are an assistant that decides which tool to use based on a list of tools to solve the user problem.

Rules:
- You only return one of the tools like \"<retrieval>\" or \"<function>\" or \"<code_interpreter>\" or \"<action>\" or multiple of them
- Do not return \"tools\"
- If you do not have any tools to use, return nothing
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

Other example:
<user>
<tools>{\"description\":\"useful to call functions in the user's product, which would provide you later some additional context about the user's problem\",\"function\":{\"arguments\":{\"type\":\"object\"},\"description\":\"A function that compute the purpose of life according to the fundamental laws of the universe.\",\"name\":\"compute_purpose_of_life\"},\"name\":\"function\"}
---
{\"description\":\"useful to retrieve information from files\",\"name\":\"retrieval\"}
---
{\"description\":\"useful to make HTTP requests to the user's APIs, which would provide you later some additional context about the user's problem. You can also use this to perform actions to help the user.\",\"data\":{\"info\":{\"description\":\"MediaWiki API\",\"title\":\"MediaWiki API\"},\"paths\":{\"/api.php\":{\"get\":{\"summary\":\"Fetch random facts from Wikipedia using the MediaWiki API\"}}}},\"name\":\"action\"}</tools>

<previous_messages>User: [Text(MessageContentTextObject { type: \"text\", text: TextData { value: \"Can you tell me a random fact?\", annotations: [] } })]
</previous_messages>

<instructions>You help me by using the tools you have.</instructions>

</user>

In this example, the assistant should return \"<action>\".

Your answer will be used to use the tool so it must be very concise and make sure to surround the tool by \"<\" and \">\", do not say anything but the tool name with the <> around it.";

    let tools = assistant.inner.tools.clone();
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
                    }),
                    AssistantTools::Extra(e) => {
                        let data = e.data.as_ref().unwrap();
                        json!({
                            "name": "action",
                            "description": "Useful to make HTTP requests to the user's APIs, which would provide you later some additional context about the user's problem. You can also use this to perform actions to help the user.",
                            "data": {
                                "info": {
                                    "description": data.get("info").unwrap_or(&json!({})).get("description").unwrap_or(&json!("")).to_string().replace("\"", ""),
                                    "title": data.get("info").unwrap_or(&json!({})).get("title").unwrap_or(&json!("")).to_string().replace("\"", ""),
                                },
                                "paths": data.get("paths"),
                            }
                        })
                },
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
        "<instructions>{}</instructions>\nSelected tool(s):",
        assistant.inner.instructions.as_ref().unwrap()
    ));    

    client.set_model_name(assistant.inner.model.clone());
    request.set_system_prompt(system_prompt.to_string());
    request.set_last_user_prompt(user_prompt);
    
    let result = 
        client.create_chat_completion(request.temperature(0.0))
        .await.map_err(|e| {
            error!(
                "Error calling Open Source {:?} LLM through OpenAI API on URL {:?}: {}",
                client.model_name, client.model_url, e
            );
            DecideToolError::Other(e.to_string())
        })?;

    info!("decide_tool_with_llm raw result: {}", result);

    // Just in case regex what's in <tool> sometimes LLM do this (e.g. extract the "tool" using a regex)
    let regex = regex::Regex::new(r"<(.*?)>").unwrap();
    let mut results = Vec::new();
    for captures in regex.captures_iter(&result) {
        results.push(captures[1].to_string());
    }
    // Also get the "<tool" e.g. LLM forget to close the > sometimes (retarded)
    let regex = regex::Regex::new(r"<(\w+)").unwrap();
    for captures in regex.captures_iter(&result) {
        results.push(captures[1].to_string().replace("\"", ""));
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

    // filter out what is not in the tools
    results.retain(|tool| 
        // function, retrieval, code_interpreter, action
        tool == "function" || tool == "retrieval" || tool == "code_interpreter" || tool == "action"
    );

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
    client: HalLLMClient, // Not using a reference here because we want to be able to tweak the client at runtime
    file_storage: &dyn FileStorage,
) {
    loop {
        match try_run_executor(&pool, con, client.clone(), file_storage).await {
            Ok(_) => continue,
            Err(e) => error!("Error: {}", e),
        }
    }
}

pub async fn try_run_executor(
    pool: &PgPool,
    con: &mut redis::aio::Connection,
    client: HalLLMClient,
    file_storage: &dyn FileStorage,
) -> Result<Run, RunError> {
    match run_executor(&pool, con, client, file_storage).await {
        Ok(run) => { 
            info!("Execution done: {:?}", run);
            set_all_steps_status(&pool, &run.inner.id, &run.user_id, RunStatus::Completed).await.map_err(|e| RunError {
                message: format!("Failed to set all steps status: {}", e),
                run_id: run.inner.id.clone(),
                thread_id: run.inner.thread_id.clone(),
                user_id: run.user_id.clone(),
            })?;
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
            // TODO: add data error in step
            set_all_steps_status(&pool, &run_error.run_id, &run_error.user_id, RunStatus::Failed).await.map_err(|e| RunError {
                message: format!("Failed to set all steps status: {}", e),
                run_id: run_error.run_id.clone(),
                thread_id: run_error.thread_id.clone(),
                user_id: run_error.user_id.clone(),
            })?;
            Err(run_error)
        }
    }
}


// The function that consume the runs queue and do all the LLM software 3.0 logic
pub async fn run_executor(
    // TODO: split in smaller functions if possible
    pool: &PgPool,
    con: &mut redis::aio::Connection,
    mut client: HalLLMClient,
    file_storage: &dyn FileStorage,
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
    let assistant_id = assistant.inner.id.clone();

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

    // LLM Context updated by tools
    let mut function_calls = String::new();
    let mut action_calls = String::new();
    let mut retrieval_files: Vec<String> = vec![];
    let mut retrieval_chunks: Vec<Chunk> = vec![];
    let mut code_output: Option<String> = None;
    let mut code: Option<String> = None;
    let mut tool_calls_db: Vec<SubmittedToolCall> = vec![];
    let mut request = HalLLMRequestArgs::default();

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

        // for each function call sent by the user, update the run step in database

        // first fetch the steps for this run 

        let steps = list_steps(
            pool,
            thread_id,
            &run.inner.id,
            &run.user_id,
        ).await.map_err(|e| RunError {
            message: format!("Failed to list steps: {}", e),
            run_id: run_id.to_string(),
            thread_id: thread_id.to_string(),
            user_id: user_id.to_string(),
        })?;

        let details = extract_step_id_and_function_output(steps, tool_calls_db.clone());

        for (step_id, tool_call_id, function_data) in details {
            
            update_step(
                pool,
                &step_id,
                RunStatus::Completed,
                StepDetails::ToolCalls(RunStepDetailsToolCallsObject {
                    r#type: "function".to_string(),
                    tool_calls: vec![RunStepDetailsToolCalls::Function(RunStepDetailsToolCallsFunctionObject{
                        id: tool_call_id,
                        r#type: "function".to_string(),
                        function: function_data,
                    })],
                }),
                &run.user_id,
            ).await.map_err(|e| RunError {
                message: format!("Failed to update step: {}", e),
                run_id: run_id.to_string(),
                thread_id: thread_id.to_string(),
                user_id: user_id.to_string(),
            })?;
        }

        // Use the tool call data to build the prompt like Input "functions" Output ""..."" DUMB MODE
        function_calls = required_action
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

        info!("function_calls: {}", function_calls);
    }

    info!("Assistant tools: {:?}", assistant.inner.tools);
    info!("Asking LLM to decide which tool to use");

    // Decide which tool to use
    let mut tools_decision = decide_tool_with_llm(&assistant, &messages, &run, tool_calls_db, 
        client.clone(),
        request.clone()
    ).await.map_err(|e| RunError {
        message: format!("Failed to decide tool: {}", e),
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        user_id: user_id.to_string(),
    })?;

    info!("Tools decision: {:?}", tools_decision);

    let mut instructions = build_instructions(
        &run.inner.instructions,
        &retrieval_files,
        &formatted_messages,
        &function_calls,
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
        None,
        &action_calls
    );

    let model = assistant.inner.model.clone();
    client.set_model_name(model.clone());
    request.set_last_user_prompt(formatted_messages.clone());
    request.set_system_prompt(instructions.clone());

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

                info!("Generating function to call");

                let function_results =
                    create_function_call(&pool, 
                        &assistant.inner.id,
                        user_id, 
                        client.clone(),
                        request.clone().temperature(0.0),
                    ).await.map_err(|e| RunError {
                        message: format!("Failed to create function call: {}", e),
                        run_id: run_id.to_string(),
                        thread_id: thread_id.to_string(),
                        user_id: user_id.to_string(),
                    })?;

                info!("Function results: {:?}", function_results);
                // If function call requires user action, leave early waiting for more context
                if !function_results.is_empty() {
                    let mut tool_call_ids = Vec::new();
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
                                    .map(|f| {
                                        let id = uuid::Uuid::new_v4().to_string();
                                        tool_call_ids.push(id.clone());
                                        RunToolCallObject {
                                            id,
                                            r#type: "function".to_string(), // TODO hardcoded
                                            function: FunctionCall {
                                                name: f.clone().name,
                                                arguments: f.clone().arguments,
                                        }
                            }})
                                    .collect::<Vec<RunToolCallObject>>(),
                            },
                        }),
                        None
                    )
                    .await.map_err(|e| RunError {
                        message: format!("Failed to update run status: {}", e),
                        run_id: run_id.to_string(),
                        thread_id: thread_id.to_string(),
                        user_id: user_id.to_string(),
                    })?;

                    // create a step with output None for each function call
                    // Convert the loop into a vector of futures
                    let futures: Vec<_> = function_results.iter().enumerate().map(|(i, function)| {
                        let pool = pool.clone();
                        let run_inner_id = run.inner.id.clone();
                        let assistant_id = assistant_id.clone();
                        let run_inner_thread_id = run.inner.thread_id.clone();
                        let tool_call_id = tool_call_ids[i].clone();
                        let user_id = run.user_id.clone();
                        let function_name = function.name.clone();
                        let function_arguments = function.arguments.clone();
                        async move {
                            create_step(
                                &pool,
                                &run_inner_id,
                                &assistant_id,
                                &run_inner_thread_id,
                                RunStepType::ToolCalls,
                                RunStatus::InProgress,
                                StepDetails::ToolCalls(RunStepDetailsToolCallsObject {
                                    r#type: "function".to_string(),
                                    tool_calls: vec![RunStepDetailsToolCalls::Function(RunStepDetailsToolCallsFunctionObject{
                                        id: tool_call_id,
                                        r#type: "function".to_string(),
                                        function: RunStepFunctionObject {
                                            name: function_name,
                                            arguments: function_arguments,
                                            output: None,
                                        }
                                    })],
                                }),
                                &user_id,
                            ).await.map_err(|e| RunError {
                                message: format!("Failed to create step: {}", e),
                                run_id: run_inner_id,
                                thread_id: run_inner_thread_id,
                                user_id: user_id,
                            })
                        }
                    }).collect();

                    // Use try_join_all to wait for all futures to complete
                    try_join_all(futures).await.map_err(|e| {
                        // Handle the error from any of the futures if they fail
                        RunError {
                            message: format!("Failed to create steps in parallel: {}", e),
                            run_id: run.inner.id.clone(),
                            thread_id: run.inner.thread_id.clone(),
                            user_id: run.user_id.clone(),
                        }
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
                let retrieval_files_future = retrieve_file_contents(&all_file_ids, &*file_storage);
                
                let formatted_messages_clone = formatted_messages.clone();
                let retrieval_chunks_future = generate_queries_and_fetch_chunks(
                    &pool,
                    client.clone(),
                    request.set_last_user_prompt(formatted_messages_clone).clone().temperature(0.0),
                );
                
                let results = tokio::join!(retrieval_files_future, retrieval_chunks_future);
                retrieval_files = results.0;
                retrieval_chunks = results.1.unwrap_or_else(|e| {
                    // ! sometimes LLM generates stupid SQL queries. for now we dont crash the run
                    error!("Failed to retrieve chunks: {}", e);
                    vec![]
                });

                create_step(
                    pool,
                    &run.inner.id,
                    &assistant_id,
                    &run.inner.thread_id,
                    RunStepType::ToolCalls,
                    RunStatus::InProgress,
                    StepDetails::ToolCalls(RunStepDetailsToolCallsObject {
                        r#type: "retrieval".to_string(),
                        tool_calls: vec![RunStepDetailsToolCalls::Retrieval(RunStepDetailsToolCallsRetrievalObject{
                            id: uuid::Uuid::new_v4().to_string(),
                            r#type: "retrieval".to_string(),
                            retrieval: HashMap::new(), // TODO
                        })],
                    }),
                    &run.user_id,
                ).await.map_err(|e| RunError {
                    message: format!("Failed to create step: {}", e),
                    run_id: run_id.to_string(),
                    thread_id: thread_id.to_string(),
                    user_id: user_id.to_string(),
                })?;

                // Include the file contents and previous messages in the instructions.
                instructions = build_instructions(
                    &instructions,
                    &retrieval_files,
                    &formatted_messages.clone(),
                    &function_calls,
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
                    None,
        &action_calls
                );
                
            }
            "code_interpreter" => {
                // Call the safe_interpreter function // TODO: not sure if we should pass formatted_messages or just last user message
                let interpreter_results = match safe_interpreter(formatted_messages.clone(), 0, 3, 
                client.clone(),
                request.clone().temperature(0.0)
            ).await {
                    Ok((code_output, code)) => {
                        // Handle the successful execution of the code
                        // You might want to store the result or send it back to the user
                        (code_output, code)
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
                info!("Code interpreter results: {:?}", interpreter_results.clone());

                code_output = Some(interpreter_results.0);
                code = Some(interpreter_results.1);


                if code_output.is_none() {
                    return Err(RunError {
                        message: format!("Failed to run code: no output"),
                        run_id: run_id.to_string(),
                        thread_id: thread_id.to_string(),
                        user_id: user_id.to_string(),
                    });
                }

                create_step(
                    pool,
                    &run.inner.id,
                    &assistant_id,
                    &run.inner.thread_id,
                    RunStepType::ToolCalls,
                    RunStatus::InProgress,
                    StepDetails::ToolCalls(RunStepDetailsToolCallsObject {
                        r#type: "code_interpreter".to_string(),
                        tool_calls: vec![RunStepDetailsToolCalls::Code(RunStepDetailsToolCallsCodeObject{
                            id: uuid::Uuid::new_v4().to_string(),
                            r#type: "code_interpreter".to_string(),
                            code_interpreter: CodeInterpreter {
                                input: code.unwrap(),
                                outputs: vec![CodeInterpreterOutput::Log(RunStepDetailsToolCallsCodeOutputLogsObject{
                                    r#type: "log".to_string(),
                                    logs: code_output.clone().unwrap(),
                                })],
                            },
                        })],
                    }),
                    &run.user_id,
                ).await.map_err(|e| RunError {
                    message: format!("Failed to create step: {}", e),
                    run_id: run_id.to_string(),
                    thread_id: thread_id.to_string(),
                    user_id: user_id.to_string(),
                })?;

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
                    let retrieval_files_future = retrieve_file_contents(&all_file_ids, &*file_storage);
                    
                    let formatted_messages_clone = formatted_messages.clone();
                    let retrieval_chunks_future = generate_queries_and_fetch_chunks(
                        &pool,
                        client.clone(),
                        request.set_last_user_prompt(formatted_messages_clone).clone().temperature(0.0)
                    );
                    
                    let (r_f, retrieval_chunks_result) = tokio::join!(retrieval_files_future, retrieval_chunks_future);

                    retrieval_chunks = retrieval_chunks_result.unwrap_or_else(|e| {
                        // ! sometimes LLM generates stupid SQL queries. for now we dont crash the run
                        error!("Failed to retrieve chunks: {}", e);
                        vec![]
                    });

                    retrieval_files = r_f;
                }

                // Build instructions with the code output
                instructions = build_instructions(
                    &instructions,
                    &retrieval_files,
                    &formatted_messages,
                    &function_calls,
                    code_output.clone().as_deref(),
                    &retrieval_chunks.iter().map(|c| 
                        serde_json::to_string(&json!({
                            "data": c.data,
                            "sequence": c.sequence,
                            "start_index": c.start_index,
                            "end_index": c.end_index,
                            "metadata": c.metadata,
                        })).unwrap()
                    ).collect::<Vec<String>>(),
                    None,
        &action_calls
                );
            },
            "action" => {
                // 1. generate function call
                // 2. execute 

                info!("Generating function to call");

                let function_results =
                    create_function_call(&pool, 
                        &assistant.inner.id,
                        user_id, 
                        client.clone(),
                        request.clone().temperature(0.0),
                    )
                    .await.map_err(|e| RunError {
                        message: format!("Failed to create function call: {}", e),
                        run_id: run_id.to_string(),
                        thread_id: thread_id.to_string(),
                        user_id: user_id.to_string(),
                    })?;

                info!("Function results: {:?}", function_results);

                // Before the loop, convert the loop into a vector of futures
                let futures: Vec<_> = function_results.into_iter().map(|function| {
                    let pool = pool.clone();
                    let assistant_id = assistant_id.clone();
                    let run_inner_id = run.inner.id.clone();
                    let run_user_id = run.user_id.clone();
                    let tool_call_id = uuid::Uuid::new_v4().to_string();
                    async move {
                        let step = create_step(
                            &pool,
                            &run_inner_id,
                            &assistant_id,
                            &thread_id,
                            RunStepType::ToolCalls,
                            RunStatus::InProgress,
                            StepDetails::ToolCalls(RunStepDetailsToolCallsObject {
                                r#type: "function".to_string(), // TODO not sure it should be function or action
                                tool_calls: vec![RunStepDetailsToolCalls::Function(RunStepDetailsToolCallsFunctionObject{
                                    id: tool_call_id.clone(),
                                    r#type: "function".to_string(),
                                    function: RunStepFunctionObject {
                                        name: function.name.clone(),
                                        arguments: function.arguments.clone(),
                                        output: None,
                                    }
                                })],
                            }),
                            &run_user_id,
                        ).await.map_err(|e| RunError {
                            message: format!("Failed to create step: {}", e),
                            run_id: run_id.to_string(),
                            thread_id: thread_id.to_string(),
                            user_id: user_id.to_string(),
                        })?;
                        let metadata = function.metadata.unwrap();
                        let output = execute_request(ActionRequest{
                            domain: metadata["domain"].to_string().replace("\"", ""),
                            path: metadata["path"].to_string().replace("\"", ""),
                            method: metadata["method"].to_string().replace("\"", ""),
                            operation: metadata["operation"].to_string().replace("\"", ""),
                            operation_hash: None,
                            is_consequential: false,
                            content_type: metadata["content_type"].to_string().replace("\"", ""),
                            params: Some(serde_json::from_str(&function.arguments).unwrap()),
                            headers: metadata.get("headers").cloned(),
                        }).await.map_err(|e| RunError {
                            message: format!("Failed to execute request: {}", e),
                            run_id: run_id.to_string(),
                            thread_id: thread_id.to_string(),
                            user_id: user_id.to_string(),
                        })?;
                        let string_output = serde_json::to_string(&output).unwrap();
                        update_step(
                            &pool,
                            &step.inner.id,
                            RunStatus::Completed,
                            StepDetails::ToolCalls(RunStepDetailsToolCallsObject {
                                r#type: "function".to_string(),
                                tool_calls: vec![RunStepDetailsToolCalls::Function(RunStepDetailsToolCallsFunctionObject{
                                    id: tool_call_id,
                                    r#type: "function".to_string(),
                                    function: RunStepFunctionObject {
                                        name: function.name.clone(),
                                        arguments: function.arguments.clone(),
                                        output: Some(string_output.clone()),
                                    }
                                })],
                            }),
                            &run_user_id,
                        ).await.map_err(|e| RunError {
                            message: format!("Failed to update step: {}", e),
                            run_id: run_id.to_string(),
                            thread_id: thread_id.to_string(),
                            user_id: user_id.to_string(),
                        })?;

                        info!("Action results: {:?}", output);

                        let stringified_function = serde_json::to_string(&json!({
                            "name": function.name,
                            "arguments": function.arguments,
                        })).unwrap().replace("\\", "");

                        Ok::<_, RunError>(format!(
                            "<input>{:?}</input>\n\n<output>{:?}</output>",
                            stringified_function, 
                            string_output
                        ).replace("\\\\", "").replace("\\\"", ""))
                    }
                }).collect();

                // Then, use tokio::try_join! to execute them concurrently
                let results: Result<Vec<_>, _> = try_join_all(futures).await;

                // Handle the results
                match results {
                    Ok(outputs) => {
                        // Concatenate all outputs into action_calls
                        action_calls = outputs.join("\n");
                    },
                    Err(e) => {
                        // Handle the error
                        return Err(e);
                    }
                }

                instructions = build_instructions(
                    &run.inner.instructions,
                    &retrieval_files,
                    &formatted_messages,
                    &function_calls,
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
                    None,
                    &action_calls
                );
            },
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

    request
            .set_system_prompt(system_prompt)
            .set_last_user_prompt(instructions);

    let result = client.create_chat_completion(
        request.temperature(0.0),
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
            let message = add_message_to_thread(
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
            create_step(
                pool,
                &run.inner.id,
                &assistant.inner.id,
                &thread.inner.id,
                RunStepType::MessageCreation,
                RunStatus::Completed,
                StepDetails::MessageCreation(RunStepDetailsMessageCreationObject {
                    r#type: "message_creation".to_string(),
                    message_creation: MessageCreation {
                        message_id: message.inner.id,
                    }
                }),
                &run.user_id.to_string(),
            )
            .await
            .map_err(|e| RunError {
                message: format!("Failed to create step: {}", e),
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
                None
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
    use hal_9100_core::runs::{get_run, create_run_and_produce_to_executor_queue};
    use async_openai::types::{
        AssistantObject, AssistantTools, AssistantToolsCode, AssistantToolsFunction,
        AssistantToolsRetrieval, ChatCompletionFunctions, MessageObject, MessageRole, RunObject, FunctionObject, AssistantToolsExtra, RunStepObject, ThreadObject,
    };
    use hal_9100_core::models::{Assistant, Message, Run, Thread};
    use hal_9100_extra::config::Hal9100Config;
    use serde_json::json;
    use sqlx::types::Uuid;
    use sqlx::{Pool, Postgres};

    use crate::assistants::create_assistant;
    use crate::models::SubmittedToolCall;
    use crate::run_steps::list_steps;
    use crate::runs::{create_run, submit_tool_outputs};
    use crate::test_data::OPENAPI_SPEC;
    use crate::threads::create_thread;

    use super::*;
    use dotenv::dotenv;
    use sqlx::postgres::PgPoolOptions;
    use std::io::Write;


    async fn setup() -> (Pool<Postgres>, hal_9100_extra::config::Hal9100Config, Box<dyn FileStorage>) {
        dotenv().ok();
        let hal_9100_config = Hal9100Config::default();
        let database_url = hal_9100_config.database_url.clone();
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
            Err(_) => (),
        };
        return (
            pool,
            hal_9100_config.clone(),
            Box::new(MinioStorage::new(hal_9100_config).await),
        );
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
            "TRUNCATE assistants, threads, messages, runs, functions, tool_calls, run_steps RESTART IDENTITY"
        )
        .execute(pool)
        .await
        .unwrap();
        reset_redis().await.unwrap();
    }

    
    #[tokio::test]
    async fn test_end_to_end_knowledge_retrieval() {
        // Setup
        let (pool, hal_9100_config, file_storage) = setup().await;
        reset_db(&pool).await;

        // Create a temporary file.
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        writeln!(temp_file, "bob's favourite number is 43").unwrap();

        // Get the path of the temporary file.
        let temp_file_path = temp_file.path();

        // Upload the temporary file
        let file_id = file_storage.upload_file(&temp_file_path).await.unwrap();
        let model_name = std::env::var("TEST_MODEL_NAME").unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string());
        
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
                model: model_name,
                file_ids: vec![file_id_clone.id],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let assistant = create_assistant(&pool, &assistant).await.unwrap();

        // check assistant has file
        assert_eq!(assistant.inner.file_ids, vec![file_id.id]);

        // 2. Create a Thread
        let thread_object = Thread {
            inner: ThreadObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };

        let thread = create_thread(&pool, &thread_object)
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
        let llm_client = HalLLMClient::new(
            assistant.inner.model,
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY must be set"),
        );
        let mut con = client.get_async_connection().await.unwrap();
        let result = try_run_executor(&pool, &mut con, llm_client, &*file_storage).await;

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
    #[ignore]
    async fn test_decide_tool_with_llm_anthropic() {
        setup().await;
        let mut functions = FunctionObject {
            description: Some("A calculator function".to_string()),
            name: "calculator".to_string(),
            parameters: Some(json!({
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
            })),
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
        let llm_client = HalLLMClient::new(
            assistant.inner.model.clone(),
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("ANTHROPIC_API_KEY").expect("MODEL_API_KEY must be set"),
        );
        let request = HalLLMRequestArgs::default().temperature(0.0);
        let result = decide_tool_with_llm(&assistant, &previous_messages, &Run::default(), vec![], llm_client,
            request
    ).await;
        let mut result = result.unwrap();
        // Check if the result is one of the expected tools
        let mut expected_tools = vec!["function".to_string(), "retrieval".to_string()];
        assert_eq!(result.sort(), expected_tools.sort());
    }


    #[tokio::test]
    #[ignore]
    async fn test_decide_tool_with_llm_code_interpreter() {
        setup().await;
        let model_name = std::env::var("TEST_MODEL_NAME")
        .unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string());

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
                model: model_name,
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

        let llm_client = HalLLMClient::new(
            assistant.inner.model.clone(),
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY"),
        );
        let request = HalLLMRequestArgs::default().temperature(0.0);
        let result = decide_tool_with_llm(&assistant, &previous_messages, &Run::default(), vec![], llm_client, request).await;

        let result = result.unwrap();
        assert_eq!(result, vec!["code_interpreter"]);
    }

    #[tokio::test]
    async fn test_decide_tool_with_llm_open_source() {
        setup().await;
        let model_name = std::env::var("TEST_MODEL_NAME")
        .unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string());

        let mut functions = FunctionObject {
            description: Some("A calculator function".to_string()),
            name: "calculator".to_string(),
            parameters: Some(json!({
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
            })),
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
                model: model_name,
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

        let llm_client = HalLLMClient::new(
            assistant.inner.model.clone(),
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY"),
        );
        let request = HalLLMRequestArgs::default().temperature(0.0);
        let result = decide_tool_with_llm(&assistant, &previous_messages, &Run::default(), vec![], llm_client, request).await;

        let mut result = result.unwrap();
        // Check if the result is one of the expected tools
        let mut expected_tools = vec!["function".to_string(), "retrieval".to_string()];
        assert_eq!(result.sort(), expected_tools.sort());
    }

    #[tokio::test]
    async fn test_end_to_end_function_calling_plus_retrieval() {
        // Setup
        let (pool, hal_9100_config, file_storage) = setup().await;

        reset_db(&pool).await;

        // 1. Create a temporary file.
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        writeln!(temp_file, "bob's favourite number is 42. bob's favourite number is 42").unwrap();

        // 2. Get the path of the temporary file.
        let temp_file_path = temp_file.path();

        // 3. Upload the temporary file
        let file_id = file_storage.upload_file(&temp_file_path).await.unwrap();
        let model_name = std::env::var("TEST_MODEL_NAME").unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string());

        // 4. Create an Assistant with function calling tool
        let file_id_clone = file_id.clone();
        let assistant = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some("You help me find people's favourite numbers using retrieval and functions".to_string()),
                name: Some("Number finder".to_string()),
                tools: vec![
                    AssistantTools::Function(AssistantToolsFunction {
                        r#type: "function".to_string(),
                        function: FunctionObject {
                            description: Some("A function that finds the favourite number of bob.".to_string()),
                            name: "determine_number".to_string(),
                            parameters: Some(json!({
                                "type": "object",
                            })),
                        },
                    }),
                    AssistantTools::Retrieval(AssistantToolsRetrieval {
                        r#type: "retrieval".to_string(),
                    }),
                ],
                model: model_name,
                file_ids: vec![file_id_clone.id],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("An assistant that finds the favourite number of bob.".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string()
        };
        let assistant = create_assistant(&pool, &assistant).await.unwrap();

        // 5. Create a Thread
        let thread_object = Thread {
            inner: ThreadObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };

        let thread = create_thread(&pool, &thread_object)
            .await
            .unwrap();

        // 6. Add a Message to a Thread
        let content = vec![MessageContent::Text(MessageContentTextObject {
            r#type: "text".to_string(),
            text: TextData {
                value: 
                "I need to know bob's favourite number. Tell me what it is based on the tools you have (e.g. function calls etc.)."
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
        let llm_client = HalLLMClient::new(
            assistant.inner.model,
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY"),
        );
        let result = try_run_executor(&pool, &mut con, llm_client.clone(), &*file_storage).await;

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
            output: "My name is bob and my favourite number is 43".to_string(),
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

        let result = try_run_executor(&pool, &mut con, llm_client, &*file_storage).await;

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
                "I need to know bob's favourite number. Tell me what it is based on the tools you have (e.g. function calls etc.)."
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
        let (pool, hal_9100_config, file_storage) = setup().await;

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
        let thread_object = Thread {
            inner: ThreadObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };

        let thread = create_thread(&pool, &thread_object)
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
        let llm_client = HalLLMClient::new(
            assistant.inner.model,
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY"),
        );
        let result = try_run_executor(&pool, &mut con, llm_client, &*file_storage).await;
    
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
        let (pool, hal_9100_config, file_storage) = setup().await;
        reset_db(&pool).await;


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
        let model_name = std::env::var("TEST_MODEL_NAME").unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string());
        
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
                model: model_name,
                file_ids: vec![file_id_clone.id.to_string()], // Add file ID here
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let assistant = create_assistant(&pool, &assistant).await.unwrap();

        // 2. Create a Thread
        let thread_object = Thread {
            inner: ThreadObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };

        let thread = create_thread(&pool, &thread_object)
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
        let llm_client = HalLLMClient::new(
            assistant.inner.model,
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY"),
        );
        let result = try_run_executor(&pool, &mut con, llm_client, &*file_storage).await;

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
        let (pool, hal_9100_config, file_storage) = setup().await;

        reset_db(&pool).await;

        let model_name = std::env::var("TEST_MODEL_NAME").unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string());
        
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
                    function: FunctionObject {
                        description: Some("A calculator function".to_string()),
                        name: "calculator".to_string(),
                        parameters: Some(json!({
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
                        })),
                    },
                })],
                model: model_name,
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
        let thread_object = Thread {
            inner: ThreadObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };

        let thread = create_thread(&pool, &thread_object)
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
        let llm_client = HalLLMClient::new(
            assistant.inner.model.clone(),
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY"),
        );
        let result = try_run_executor(&pool, &mut con, llm_client.clone(), &*file_storage).await;

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


        let request = HalLLMRequestArgs::default().temperature(0.0);
        let result = decide_tool_with_llm(&assistant, &previous_messages, &run, tool_outputs.clone(), llm_client, request).await;

        let result = result.unwrap();
        println!("{:?}", result);
        assert!(!result.contains(&"function".to_string()), "Expected the function tool to not be returned, but it was: {:?}", result);
    }

    #[tokio::test]
    async fn test_decide_tool_with_llm_action() {
        // Setup
        let (pool, hal_9100_config, file_storage) = setup().await;

        // Get the model name from environment variable or use default
        let model_name = std::env::var("TEST_MODEL_NAME").unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string());

        // Create an assistant with "action" tool
        let assistant = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "You are a personal assistant. Use the MediaWiki API to fetch random facts."
                        .to_string(),
                ),
                name: Some("Fact Fetcher".to_string()),
                tools: vec![AssistantTools::Extra(AssistantToolsExtra {
                    r#type: "action".to_string(),
                    data: Some(serde_yaml::from_str(OPENAPI_SPEC).unwrap())})],
                model: model_name,
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
                        value: "Give me a random fact.".to_string(),
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
        let llm_client = HalLLMClient::new(
            assistant.inner.model.clone(),
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY"),
        );
        let request = HalLLMRequestArgs::default().temperature(0.0);
        let result = decide_tool_with_llm(&assistant, &previous_messages, &Run::default(), vec![], llm_client, request).await;
        let result = result.unwrap();

        // Check if the result is "action"
        assert_eq!(result[0], "action");
    }

    #[tokio::test]
    #[ignore] // TODO
    async fn test_end_to_end_action_tool() {
        // Setup
        let (pool, hal_9100_config, file_storage) = setup().await;

        let model_name = std::env::var("TEST_MODEL_NAME").unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string());

        // Create an assistant with "action" tool
        let assistant = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "You are a personal assistant. Use the MediaWiki API to fetch random facts."
                        .to_string(),
                ),
                name: Some("Fact Fetcher".to_string()),
                tools: vec![AssistantTools::Extra(AssistantToolsExtra {
                    r#type: "action".to_string(),
                    data: Some(serde_yaml::from_str(OPENAPI_SPEC).unwrap())})],
                model: model_name,
                file_ids: vec![],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let assistant = create_assistant(&pool, &assistant).await.unwrap();

        // Create a Thread
        let thread = create_thread(&pool, &Thread {
            inner: ThreadObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        })
            .await
            .unwrap();

        // Add a Message to a Thread
        let content = vec![MessageContent::Text(MessageContentTextObject {
            r#type: "text".to_string(),
            text: TextData {
                value: "Give me a random fact. Also provide the exact output from the API".to_string(),
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
            "Please help me find a random fact.",
             assistant.user_id.as_str(), 
             con
        ).await.unwrap();

        assert_eq!(run.inner.status, RunStatus::Queued);

        // Run the queue consumer again
        let mut con = client.get_async_connection().await.unwrap();
        let llm_client = HalLLMClient::new(
            std::env::var("TEST_MODEL_NAME")
                .unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string()),
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY must be set"),
        );
        let result = try_run_executor(&pool, &mut con, llm_client, &*file_storage).await;

        assert!(result.is_ok(), "{:?}", result);

        let run = result.unwrap();

        // Check the result
        assert_eq!(run.inner.status, RunStatus::Completed);

        // Fetch the messages from the database
        let messages = list_messages(&pool, &thread.inner.id, &assistant.user_id)
            .await
            .unwrap();

        // Check the messages
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].inner.role, MessageRole::User);
        if let MessageContent::Text(text_object) = &messages[0].inner.content[0] {
            assert_eq!(text_object.text.value, "Give me a random fact. Also provide the exact output from the API");
        } else {
            panic!("Expected a Text message, but got something else.");
        }

        assert_eq!(messages[1].inner.role, MessageRole::Assistant);
        if let MessageContent::Text(text_object) = &messages[1].inner.content[0] {
            assert!(
                text_object.text.value.contains("ID") 
                || text_object.text.value.contains("id") 
                || text_object.text.value.contains("batchcomplete") 
                || text_object.text.value.contains("talk"), 
                "Expected the assistant to return a text containing either 'ID', 'id', 'batchcomplete', or 'talk', but got something else: {}", 
                text_object.text.value
            );
        } else {
            panic!("Expected a Text message, but got something else.");
        }
    }

    #[tokio::test]
    async fn test_create_step_after_assistant_message() {
        let (pool, hal_9100_config, file_storage) = setup().await;

        reset_db(&pool).await;

        let model_name = std::env::var("TEST_MODEL_NAME").unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string());

        // Create an assistant
        let assistant = create_assistant(&pool, &Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "You are a personal math tutor. Write and run code to answer math questions."
                        .to_string(),
                ),
                name: Some("Math Tutor".to_string()),
                tools: vec![],
                model: model_name,
                file_ids: vec![],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        }).await.unwrap();

        // Create a thread
        let thread = create_thread(&pool, &Thread {
            inner: ThreadObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        })
            .await
            .unwrap();

        // Add a user message to the thread
        let user_message = add_message_to_thread(
            &pool,
            &thread.inner.id,
            MessageRole::User,
            vec![MessageContent::Text(MessageContentTextObject {
                r#type: "text".to_string(),
                text: TextData {
                    value: "User message".to_string(),
                    annotations: vec![],
                },
            })],
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
            "Please help me find a random fact.",
             assistant.user_id.as_str(), 
             con
        ).await.unwrap();

        assert_eq!(run.inner.status, RunStatus::Queued);

        // Run the queue consumer again
        let mut con = client.get_async_connection().await.unwrap();
        let llm_client = HalLLMClient::new(
            std::env::var("TEST_MODEL_NAME")
                .unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string()),
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY must be set"),
        );
        let result = try_run_executor(&pool, &mut con, llm_client, &*file_storage).await;

        assert!(result.is_ok(), "{:?}", result);

        let run = result.unwrap();

        // Check the result
        assert_eq!(run.inner.status, RunStatus::Completed);

        // Fetch the steps from the database
        let steps = list_steps(&pool, &thread.inner.id, &run.inner.id, &assistant.user_id)
            .await
            .unwrap();

        // Check the steps
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].inner.r#type, RunStepType::MessageCreation);
        info!("steps: {:?}", steps);
        // match steps[0].inner.step_details.clone() {
        //     StepDetails::MessageCreation(details) => {
        //         assert_eq!(details.message_creation.message_id, assistant_message.inner.id);
        //     },
        //     _ => panic!("Expected a MessageCreation step, but got something else."),
        // }
    }

    #[test]
    fn test_extract_step_id_and_function_output() {
        // Create a mock step
        let step = RunStep {
            inner: RunStepObject {
                id: "step-abcd".to_string(),
                object: "".to_string(),
                created_at: 0,
                assistant_id: Some("".to_string()),
                thread_id: "".to_string(),
                run_id: "".to_string(),
                r#type: RunStepType::ToolCalls,
                status: RunStatus::InProgress,
                last_error: None,
                expired_at: None,
                cancelled_at: None,
                failed_at: None,
                completed_at: None,
                metadata: None,
                step_details: StepDetails::ToolCalls(RunStepDetailsToolCallsObject {
                    r#type: "function".to_string(),
                    tool_calls: vec![RunStepDetailsToolCalls::Function(RunStepDetailsToolCallsFunctionObject {
                        id: "call-abcd".to_string(),
                        r#type: "function".to_string(),
                        function: RunStepFunctionObject {
                            name: "test_function".to_string(),
                            arguments: "test_arguments".to_string(),
                            output: None,
                        },
                    })],
                }),
            },
            user_id: "1".to_string(),
        };

        // Create a mock tool call
        let tool_call = SubmittedToolCall {
            id: "call-abcd".to_string(),
            output: "dog".to_string(),
            run_id: "1".to_string(),
            created_at: 0,
            user_id: "1".to_string(),
        };

        // Call the function with the mock data
        let result = extract_step_id_and_function_output(vec![step], vec![tool_call]);

        // Check the result
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "step-abcd");
        assert_eq!(result[0].1, "call-abcd");
        assert_eq!(result[0].2.name, "test_function");
        assert_eq!(result[0].2.output.clone().unwrap(), "dog");
    }

    #[tokio::test]
    async fn test_extract_step_id_and_function_output_integration() {
        // Setup
        let (pool, hal_9100_config, file_storage) = setup().await;
        reset_db(&pool).await;
        let model_name = std::env::var("TEST_MODEL_NAME").unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string());

        // Create an assistant
        let assistant = create_assistant(&pool, &Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "Help me using functions."
                        .to_string(),
                ),
                name: Some("Math Tutor".to_string()),
                tools: vec![AssistantTools::Function(AssistantToolsFunction {
                    r#type: "function".to_string(),
                    function: FunctionObject {
                        description: Some("A calculator function".to_string()),
                        name: "calculator".to_string(),
                        parameters: Some(json!({
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
                        })),
                    },
                })],
                model: model_name,
                file_ids: vec![],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        }).await.unwrap();

        // Create a thread
        let thread = create_thread(&pool, &Thread {
            inner: ThreadObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        })
            .await
            .unwrap();

        // Add a user message to the thread
        let user_message = add_message_to_thread(
            &pool,
            &thread.inner.id,
            MessageRole::User,
            vec![MessageContent::Text(MessageContentTextObject {
                r#type: "text".to_string(),
                text: TextData {
                    value: "Please calculate 2 + 2.".to_string(),
                    annotations: vec![],
                },
            })],
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
            "Please help me find by using the function tool.",
            assistant.user_id.as_str(), 
            con
        ).await.unwrap();

        assert_eq!(run.inner.status, RunStatus::Queued);

        // Run the queue consumer again
        let mut con = client.get_async_connection().await.unwrap();
        let llm_client = HalLLMClient::new(
            std::env::var("TEST_MODEL_NAME")
                .unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string()),
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY must be set"),
        );
        let result = try_run_executor(&pool, &mut con, llm_client, &*file_storage).await;

        assert!(result.is_ok(), "{:?}", result);

        let run = result.unwrap();

        // Check the result
        assert_eq!(run.inner.status, RunStatus::RequiresAction);

        let tool_call_id = run
            .inner
            .required_action
            .unwrap()
            .submit_tool_outputs
            .tool_calls[0]
            .id
            .clone();
        // Submit tool outputs
        let tool_outputs = vec![SubmittedToolCall {
            id: tool_call_id.clone(),
            output: "4".to_string(),
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

        let steps = list_steps(&pool, &thread.inner.id, &run.inner.id, &assistant.user_id)
            .await
            .unwrap();

        let step_id = steps[0].inner.id.clone();
        let result = extract_step_id_and_function_output(steps, tool_outputs);

        // Check the result
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, step_id);
        assert_eq!(result[0].1, tool_call_id);
        assert_eq!(result[0].2.name, "calculator");
        assert_eq!(result[0].2.output.clone().unwrap(), "4");
    }

    #[tokio::test]
    async fn test_step_update_with_function_output() {
        // Setup
        let (pool, hal_9100_config, file_storage) = setup().await;
        reset_db(&pool).await;
        let model_name = std::env::var("TEST_MODEL_NAME").unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string());

        // Create an assistant
        let assistant = create_assistant(&pool, &Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "Help me using functions."
                        .to_string(),
                ),
                name: Some("Math Tutor".to_string()),
                tools: vec![AssistantTools::Function(AssistantToolsFunction {
                    r#type: "function".to_string(),
                    function: FunctionObject {
                        description: Some("A calculator function".to_string()),
                        name: "calculator".to_string(),
                        parameters: Some(json!({
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
                        })),
                    },
                })],
                model: model_name,
                file_ids: vec![],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        }).await.unwrap();

        // Create a thread
        let thread = create_thread(&pool, &Thread {
            inner: ThreadObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        })
            .await
            .unwrap();

        // Add a user message to the thread
        let user_message = add_message_to_thread(
            &pool,
            &thread.inner.id,
            MessageRole::User,
            vec![MessageContent::Text(MessageContentTextObject {
                r#type: "text".to_string(),
                text: TextData {
                    value: "Please calculate 2 + 2.".to_string(),
                    annotations: vec![],
                },
            })],
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
            "Please help me find by using the function tool.",
            assistant.user_id.as_str(), 
            con
        ).await.unwrap();

        assert_eq!(run.inner.status, RunStatus::Queued);

        // Run the queue consumer again
        let mut con = client.get_async_connection().await.unwrap();
        let llm_client = HalLLMClient::new(
            std::env::var("TEST_MODEL_NAME")
                .unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string()),
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY must be set"),
        );
        let result = try_run_executor(&pool, &mut con, llm_client, &*file_storage).await;

        assert!(result.is_ok(), "{:?}", result);

        let run = result.unwrap();

        // Check the result
        assert_eq!(run.inner.status, RunStatus::RequiresAction);

        let tool_call_id = run
            .inner
            .required_action
            .unwrap()
            .submit_tool_outputs
            .tool_calls[0]
            .id
            .clone();

        // Fetch the steps from the database
        let steps = list_steps(&pool, &thread.inner.id, &run.inner.id, &assistant.user_id)
            .await
            .unwrap();

        // Check the step before tool output submission
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].inner.r#type, RunStepType::ToolCalls);
        if let StepDetails::ToolCalls(details) = &steps[0].inner.step_details {
            if let RunStepDetailsToolCalls::Function(function) = &details.tool_calls[0] {
                assert_eq!(function.id, tool_call_id);
                assert_eq!(function.function.name, "calculator");
                assert!(function.function.output.is_none());
            } else {
                panic!("Expected a Function tool call, but got something else.");
            }
        } else {
            panic!("Expected a ToolCalls step, but got something else.");
        }

        // Submit tool outputs
        let tool_outputs = vec![SubmittedToolCall {
            id: tool_call_id.clone(),
            output: "4".to_string(),
            run_id: run.inner.id.clone(),
            created_at: 0,
            user_id: assistant.user_id.clone(),
        }];
        let con = client.get_async_connection().await.unwrap();

        submit_tool_outputs(
            &pool,
            &thread.inner.id,
            &run.inner.id,
            &assistant.user_id,
            tool_outputs,
            con,
        )
        .await
        .unwrap();


        // Run the queue consumer again
        let mut con = client.get_async_connection().await.unwrap();
        let llm_client = HalLLMClient::new(
            std::env::var("TEST_MODEL_NAME")
                .unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string()),
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY must be set"),
        );
        let result = try_run_executor(&pool, &mut con, llm_client, &*file_storage).await;

        assert!(result.is_ok(), "{:?}", result);

        let run = result.unwrap();

        // Check the result
        assert_eq!(run.inner.status, RunStatus::Completed);


        // Fetch the steps from the database again
        let steps = list_steps(&pool, &thread.inner.id, &run.inner.id, &assistant.user_id)
            .await
            .unwrap();

        // Check the step after tool output submission
        assert_eq!(steps.len(), 2);

        // Find the tool call step
        let tool_call_step = steps.iter().find(|step| step.inner.r#type == RunStepType::ToolCalls).unwrap();

        if let StepDetails::ToolCalls(details) = &tool_call_step.inner.step_details {
            if let RunStepDetailsToolCalls::Function(function) = &details.tool_calls[0] {
                assert_eq!(function.id, tool_call_id);
                assert_eq!(function.function.name, "calculator");
                assert_eq!(function.function.output.as_ref().unwrap(), "4");
            } else {
                panic!("Expected a Function tool call, but got something else.");
            }
        } else {
            panic!("Expected a ToolCalls step, but got something else.");
        }
    }

    #[tokio::test]
    async fn test_step_update_with_multiple_function_output() {
        // Setup
        let (pool, hal_9100_config, file_storage) = setup().await;
        reset_db(&pool).await;
        let model_name = std::env::var("TEST_MODEL_NAME").unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string());

        // Create an assistant with two functions: 'get_weather' and 'celsius_to_kelvin'
        let assistant = create_assistant(&pool, &Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "Help me using functions."
                        .to_string(),
                ),
                name: Some("Weather Assistant".to_string()),
                tools: vec![
                    AssistantTools::Function(AssistantToolsFunction {
                        r#type: "function".to_string(),
                        function: FunctionObject {
                            description: Some("A function to get weather".to_string()),
                            name: "get_weather".to_string(),
                            parameters: Some(json!({
                                "type": "object",
                                "properties": {
                                    "location": {
                                        "type": "string",
                                        "description": "The location to get weather for."
                                    }
                                }
                            })),
                        },
                    }),
                    AssistantTools::Function(AssistantToolsFunction {
                        r#type: "function".to_string(),
                        function: FunctionObject {
                            description: Some("A function to get my name".to_string()),
                            name: "get_name".to_string(),
                            parameters: Some(json!({
                                "type": "object",
                                "properties": {}
                            })),
                        },
                    })
                ],
                model: model_name,
                file_ids: vec![],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        }).await.unwrap();

        // Create a thread
        let thread = create_thread(&pool, &Thread {
            inner: ThreadObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        })
            .await
            .unwrap();

        // Add a user message to the thread
        let user_message = add_message_to_thread(
            &pool,
            &thread.inner.id,
            MessageRole::User,
            vec![MessageContent::Text(MessageContentTextObject {
                r#type: "text".to_string(),
                text: TextData {
                    value: "Please tell me the weather and say my name by using functions.".to_string(),
                    annotations: vec![],
                },
            })],
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
            "Please help me find the weather and say my name by using functions.",
            assistant.user_id.as_str(), 
            con
        ).await.unwrap();

        assert_eq!(run.inner.status, RunStatus::Queued);

        // Run the queue consumer again
        let mut con = client.get_async_connection().await.unwrap();
        let llm_client = HalLLMClient::new(
            std::env::var("TEST_MODEL_NAME")
                .unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string()),
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY must be set"),
        );
        let result = try_run_executor(&pool, &mut con, llm_client, &*file_storage).await;

        assert!(result.is_ok(), "{:?}", result);

        let run = result.unwrap();

        // Check the result
        assert_eq!(run.inner.status, RunStatus::RequiresAction);
        
        let r_a = run
            .inner
            .required_action;

        // get the id of the weather tool call
        let weather_call_id = r_a.clone()
            .unwrap()
            .submit_tool_outputs
            .tool_calls
            .iter()
            .find(|tool_call| tool_call.function.name == "get_weather")
            .unwrap()
            .id
            .clone();
    
        let name_call_id = r_a.clone()
            .unwrap()
            .submit_tool_outputs
            .tool_calls
            .iter()
            .find(|tool_call| tool_call.function.name == "get_name")
            .unwrap()
            .id
            .clone();
        
        // Submit tool outputs
        let tool_outputs = vec![
            SubmittedToolCall {
                id: weather_call_id.clone(),
                output: "20".to_string(), // Let's say the weather in New York is 20 Celsius
                run_id: run.inner.id.clone(),
                created_at: 0,
                user_id: assistant.user_id.clone(),
            },
            SubmittedToolCall {
                id: name_call_id.clone(),
                output: "Bob is my name".to_string(),
                run_id: run.inner.id.clone(),
                created_at: 0,
                user_id: assistant.user_id.clone(),
            },
        ];
        let con = client.get_async_connection().await.unwrap();
        
        submit_tool_outputs(
            &pool,
            &thread.inner.id,
            &run.inner.id,
            &assistant.user_id,
            tool_outputs,
            con,
        )
        .await
        .unwrap();

        // Run the queue consumer again
        let mut con = client.get_async_connection().await.unwrap();
        let llm_client = HalLLMClient::new(
            std::env::var("TEST_MODEL_NAME")
                .unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string()),
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY must be set"),
        );
        let result = try_run_executor(&pool, &mut con, llm_client, &*file_storage).await;

        assert!(result.is_ok(), "{:?}", result);

        let run = result.unwrap();

        // Check the result
        assert_eq!(run.inner.status, RunStatus::Completed);

        // Check the final message
        let messages = list_messages(&pool, &thread.inner.id, &assistant.user_id)
            .await
            .unwrap();

        assert_eq!(messages.len(), 2); 

        let assistant_message = &messages[1];
        assert_eq!(assistant_message.inner.role, MessageRole::Assistant);
        if let MessageContent::Text(text_object) = &assistant_message.inner.content[0] {
            assert_eq!(text_object.text.value.contains("20"), true);
            assert_eq!(text_object.text.value.contains("Bob"), true);
        } else {
            panic!("Expected a Text message, but got something else.");
        }

        // check there are 3 steps 

        let steps = list_steps(&pool, &thread.inner.id, &run.inner.id, &assistant.user_id)
            .await
            .unwrap();

        assert_eq!(steps.len(), 3);
        // there should be 2 tool call steps
        let tool_call_steps = steps.iter().filter(|step| step.inner.r#type == RunStepType::ToolCalls).collect::<Vec<&RunStep>>();
        assert_eq!(tool_call_steps.len(), 2);

        // check the id of the tool call steps match the tool call ids

        let tool_call_step_ids = serde_json::to_string(&steps).unwrap();
        assert_eq!(tool_call_step_ids.contains(&weather_call_id), true);
        assert_eq!(tool_call_step_ids.contains(&name_call_id), true);
    }
}
