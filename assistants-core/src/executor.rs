use async_openai::Client;
use async_openai::config::OpenAIConfig;
use async_openai::error::OpenAIError;
use async_openai::types::{
    AssistantTools, FunctionCall, MessageContent, MessageContentTextObject, MessageRole,
    RequiredAction, RunStatus, RunToolCallObject, SubmitToolOutputs, TextData, CreateChatCompletionRequestArgs, ChatCompletionRequestUserMessageArgs, CreateChatCompletionResponse, CreateChatCompletionStreamResponse,
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
use std::pin::Pin;
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

use assistants_core::function_calling::string_to_function_call;
use assistants_core::retrieval::fetch_chunks;

use pin_project::pin_project;

use crate::assistants::Tools;
use crate::prompts::TagStream;


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
            println!("Run error: {}", run_error);
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

    // TODO: lock the thread around here? https://github.com/stellar-amenities/assistants/issues/26

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

    let mut tool_calls = String::new();
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
        let tool_calls_db = get_tool_calls(
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
        tool_calls = required_action
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

        info!("tool_calls: {}", tool_calls);
    }

    info!("Assistant tools: {:?}", assistant.inner.tools);

    let assistant_instructions = format!(
        "<assistant>\n{}\n</assistant>",
        assistant.inner.instructions.as_ref().unwrap()
    );

    let run_instructions = format!(
        "<run>\n{}\n</run>",
        run.inner.instructions
    );

    // TODO: let's reduce this prompt in the future using robust benchmarking
    let fundamental_instructions = "<fundamental>
You are an AI Assistant that helps a user. Your responses are being parsed to trigger actions.
You can decide how many iterations you can take to solve the user's problem. This is particularly useful for problems that require multiple steps to solve.
You might be given a set of tools that you can use by using it like: <tool_name> in your answer. Make sure to close the tag and sometimes use content inside the tag.

Your fundamental, unbreakable rules are:
- Your message is always structured in a sort of xml but don't abuse nested tags. If you want to comment your solutions, use <comment>your comment</comment>.
- But in general don't be too verbose, stick to the point, except if the user ask for it.
- Only use the tools you are given.
- To use the tools, make sure to follow the usage given in the <tools> tag precisely.
- Do not try to escape characters.
- You will solve problems in multiple steps, fundamentally you are within a solution-loop.
- Do not reuse multiple times the same tool within a solution-loop.
- To use the tool you want, say the tool is named \"cut_wood\" and you want to use it, you can use it like this: <cut_wood>some parameters</cut_wood>.
- Do not invent new tools, new information, etc.
- If you don't know the answer, say \"I don't know\".
- Fundamental instructions are the most important instructions. You must always follow them. Then the assistant instructions. Then the run instructions. Then the user's messages.
</fundamental>


Examples of your answers:

User: \"I want to gather wood for winter, there are 2 forests, one is 10km away and contains a lot of Pine trees while the other is 20km away and contains a lot of Oak trees. 
I gave you a manual that explains the best wood to use for winter.
I have 12.3L of gas in my car. Which forest should I go to?\"

Answer:

<steps>
2
</steps>

<comment>
I will first need to get some more information from this manual.
</comment>

<retrieval>
pine | oak
</retrieval>


...
Later the loop will call you again with the new information acquired from this tool.
... 

Answer:

<steps>
1
</steps>

<comment>
I will go to the forest that contains the most pine trees.
</comment>";

    let final_instructions = format!("{}\n{}\n{}\n", fundamental_instructions, assistant_instructions, run_instructions);
    
    
    let tool_map = serde_json::json!({
        "steps": "<steps>[Steps]</steps>: Use this tool to solve the problem in multiple steps. For example: <steps>1</steps> means you will solve the problem in 1 step. <steps>2</steps> means you will solve the problem in 2 steps. etc.",
        "function_calling": "<function_calling>[Function Calling]</function_calling>: Use this tool to call a function. Make sure to write correct JSON or it will fail. 

Please provide the name of the function you want to use and the arguments in the following format: { 'name': 'function_name', 'arguments': { 'arg_name1': 'parameter_value', 'arg_name2': 'arg_value' ... } }.

Rules:
- Do not break lines.
- Make sure to surround the JSON by <function_calling>{...}</function_calling>
- Do not escape characters.
- Do not say anything but the <function_calling> tag with the JSON inside.

Examples:

1. Fetching a user's profile

Prompt:
{\"function\": {\"description\": \"Fetch a user's profile\",\"name\": \"get_user_profile\",\"parameters\": {\"username\": {\"properties\": {},\"required\": [\"username\"],\"type\": \"string\"}}},\"user_context\": \"I want to see the profile of user 'john_doe'.\"}
Answer:
<function_calling>{ \"name\": \"get_user_profile\", \"arguments\": { \"username\": \"john_doe\" } }</function_calling>

2. Sending a message

Prompt:
{\"function\": {\"description\": \"Send a message to a user\",\"name\": \"send_message\",\"parameters\": {\"recipient\": {\"properties\": {},\"required\": [\"recipient\"],\"type\": \"string\"}, \"message\": {\"properties\": {},\"required\": [\"message\"],\"type\": \"string\"}}},\"user_context\": \"I want to send 'Hello, how are you?' to 'jane_doe'.\"}
Answer:
<function_calling>{ \"name\": \"send_message\", \"arguments\": { \"recipient\": \"jane_doe\", \"message\": \"Hello, how are you?\" } }</function_calling>

Negative examples:

Prompt:
{\"function\": {\"description\": \"Get the weather for a city\",\"name\": \"weather\",\"parameters\": {\"city\": {\"properties\": {},\"required\": [\"city\"],\"type\": \"string\"}}},\"user_context\": \"Give me a weather report for Toronto, Canada.\"}
Incorrect Answer:
<function_calling>{ \"name\": \"weather\", \"arguments\": { \"city\": \"Toronto, Canada\" } }</function_calling>",
        "code_interpreter": "<code_interpreter>[Code Interpreter]</code_interpreter>: Use this tool to generate code. This is useful to do complex data analysis. For example: <code_interpreter></code_interpreter>. You do not need to pass any parameters to this tool.",
        "retrieval": "<retrieval>[Retrieval]</retrieval>: Use this tool to retrieve information from a knowledge base. 
        
You must return return the best full-text search query for the given context to solve the user's problem.

Your output will be used in the following code:

// Convert the query to tsquery and execute it on the database
let rows = sqlx::query!(
    r#\"
    SELECT sequence, data, file_name, metadata FROM chunks 
    WHERE to_tsvector(data) @@ to_tsquery($1)
    \"#,
    query,
)
.fetch_all(pool)
.await?;

Where \"query\" is your output.

Examples:

1. Healthcare: the output could be \"heart & disease | stroke\".

2. Finance: the output could be \"stocks | bonds\".

3. Education: the output could be \"mathematics | physics\".

4. Automotive: the output could be \"sedan | SUV\".

5. Agriculture: the output could be \"organic | conventional & farming\".

Do not add spaces in your output, e.g. \"disease of the heart\" is wrong, \"disease & heart\" is correct.

Make sure to surround your query with the tag <retrieval>."
    });

    // based on assistants tools

    let mut tools = Vec::new();
    tools.push(tool_map["steps"].to_string());
    let functions = assistant.inner.tools.iter().filter_map(|tool| {
        if let AssistantTools::Function(f) = tool {
            Some(f)
        } else {
            None
        }
    }).collect::<Vec<_>>();

    if !functions.is_empty() {
        let function_tool_strings: Vec<String> = functions.iter().map(|tool| {
            serde_json::to_string(&tool.function).unwrap()
        }).collect();
        tools.push(format!(
            "{}\n{}",
            tool_map["function_calling"].to_string(),
            function_tool_strings.join("\n")
        ));
    }
    if assistant.inner.tools.iter().any(|tool| matches!(tool, AssistantTools::Retrieval(_))) {
        tools.push(tool_map["retrieval"].to_string());
    }
    if assistant.inner.tools.iter().any(|tool| matches!(tool, AssistantTools::Code(_))) {
        tools.push(tool_map["code_interpreter"].to_string());
    }
    
    let instructions = build_instructions(
        &final_instructions,
        &vec![],
        &formatted_messages,
        &tools,
        &tool_calls,
        None,
        &vec![],
        None
    );

    // current hack: if the model name contains "gpt-4" or "gpt-3.5" we assume it's OpenAI
    // otherwise it's open source LLM
    // we don't support Anthropic - Fuck them
    let is_openai_model = assistant.inner.model.contains("gpt-4") || assistant.inner.model.contains("gpt-3.5");
   
    let api_key = if is_openai_model {
        std::env::var("OPENAI_API_KEY").unwrap()
    } else {
        std::env::var("MODEL_API_KEY").unwrap_or_else(|_| String::from("EMPTY"))
    };
    let api_base = if is_openai_model {
        "https://api.openai.com/v1".to_string()
    } else {
        std::env::var("MODEL_URL").unwrap()
    };
    let llm_api_config = OpenAIConfig::new().with_api_base(api_base).with_api_key(api_key);
    let client = Client::with_config(llm_api_config);
    let bpe = p50k_base().unwrap();
    let context_size = std::env::var("MODEL_CONTEXT_SIZE")
            .unwrap_or_else(|_| "4096".to_string())
            .parse::<usize>()
            .unwrap_or(4096);

    let mut all_file_ids = Vec::new();

    // If the run has associated file IDs, add them to the list
    all_file_ids.extend(run.inner.file_ids.iter().cloned());

    // If the assistant has associated file IDs, add them to the list
    all_file_ids.extend(assistant.inner.file_ids.iter().cloned());


    let steps = 1;
    let instructions_clone = instructions.clone();
    let mut tool_step: ToolStep = ToolStep::new(run, instructions, "".to_string());
    let mut code_output = String::new();
    let mut retrieval_files = vec![];
    let mut retrieval_chunks: Vec<Chunk> = vec![];
    // The LLM decides how many steps it wants to take to solve the problem
    for _ in 0..steps {
        let run_inner_required_action = tool_step.run.inner.required_action.clone();

        // At every inference, we compute the input size in terms of token
        let tokens =
            bpe.encode_with_special_tokens(&serde_json::to_string(&tool_step.instructions).unwrap());
        let max_tokens = (context_size - tokens.len()) as u16;
        println!("instructions: {}", tool_step.instructions);
        let request = CreateChatCompletionRequestArgs::default()
            .model(assistant.inner.model.clone())
            // .max_tokens(max_tokens)
            // .max_tokens(u16::MAX) // TODO: why not?
            .max_tokens(max_tokens)
            .messages([ChatCompletionRequestUserMessageArgs::default()
                .content(tool_step.instructions)
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

        let stream = client.chat().create_stream(request).await.map_err(|e| RunError {
            message: format!("Failed to create stream: {}", e),
            run_id: run_id.to_string(),
            thread_id: thread_id.to_string(),
            user_id: user_id.to_string(),
        })?;
        println!("calling use_tools");
        tool_step = use_tools(
            stream,
            pool,
            steps,
            run_id,
            thread_id,
            user_id,
            &formatted_messages,
            &tools,
            &tool_calls,
            run_inner_required_action,
            &file_storage,
            &all_file_ids,
            &assistant.inner.model,
            &instructions_clone,
            &mut code_output,
            &mut retrieval_chunks,
            &mut retrieval_files,
        )
        .await?;
        // Update instructions with the new tool usage
        tool_step.instructions = build_instructions(
            &instructions_clone,
            &retrieval_files,
            &formatted_messages,
            &tools,
            &tool_calls,
            Some(&code_output),
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
        // TODO: impl steps update
    }

    // if the last run step requires an action, return now, it's a function calling
    if tool_step.run.inner.status == RunStatus::RequiresAction {
        return Ok(tool_step.run);
    }

    let content = vec![MessageContent::Text(MessageContentTextObject {
        r#type: "text".to_string(),
        text: TextData {
            value: tool_step.llm_output,
            annotations: vec![],
        },
    })];
    add_message_to_thread(
        pool,
        &thread.inner.id,
        MessageRole::Assistant,
        content,
        &tool_step.run.user_id.to_string(),
        None,
    )
    .await.map_err(|e| RunError {
        message: format!("Failed to add message to thread: {}", e),
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        user_id: user_id.to_string(),
    })?;

    // set to completed
    update_run_status(
        pool,
        thread_id,
        &tool_step.run.inner.id,
        RunStatus::Completed,
        &tool_step.run.user_id,
        None,
        None,
    )
    .await.map_err(|e| RunError {
        message: format!("Failed to update run status: {}", e),
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        user_id: user_id.to_string(),
    })?;
    

    Ok(tool_step.run)
}   

// Helper functions for handling different LLM actions
async fn handle_steps_action(
    last_action: &LLMAction,
    steps: &mut usize,
) -> Result<(), RunError> {
    let content = last_action.content.replace("<steps>", "").replace("</steps>", "");
    *steps = content.parse::<usize>().unwrap_or(*steps);
    Ok(())
}
async fn handle_function_calling_action(
    pool: &PgPool,
    thread_id: &str,
    run_id: &str,
    user_id: &str,
    last_action: &LLMAction,
    run_inner_required_action: Option<RequiredAction>,
) -> Result<(), RunError> {
    // TODO: any run status update here?
    if run_inner_required_action.is_none() {
        // TODO: multiple function calls?
        let function_call = string_to_function_call(&last_action.content).map_err(|e| RunError {
            message: format!("Failed to parse function call: {}", e),
            run_id: run_id.to_string(),
            thread_id: thread_id.to_string(),
            user_id: user_id.to_string(),
        })?;
        let function_call_id = uuid::Uuid::new_v4().to_string();

        let required_action = RequiredAction {
            r#type: "submit_tool_outputs".to_string(),
            submit_tool_outputs: SubmitToolOutputs {
                tool_calls: vec![RunToolCallObject {
                    id: function_call_id,
                    r#type: "function".to_string(),
                    function: function_call,
                }],
            },
        };

        update_run_status(
            pool,
            thread_id,
            run_id,
            RunStatus::RequiresAction,
            user_id,
            Some(required_action),
            None,
        )
        .await
        .map_err(|e| RunError {
            message: format!("Failed to update run status: {}", e),
            run_id: run_id.to_string(),
            thread_id: thread_id.to_string(),
            user_id: user_id.to_string(),
        })?;
    }

    Ok(())
}

async fn handle_code_interpreter_action(
    thread_id: &str,
    run_id: &str,
    user_id: &str,
    formatted_messages: &str,
    model_name: &str,
    code_output: &mut String,
) -> Result<(), RunError> {
    // TODO: atm this function still uses a separate prompt - not sure we want to unify into single prompt
    *code_output = safe_interpreter(
        formatted_messages.to_string(),
        0,
        3,
        InterpreterModelConfig {
            model_name: model_name.to_string(),
            model_url: None,
            max_tokens_to_sample: -1,
            stop_sequences: None,
            top_p: Some(1.0),
            top_k: None,
            metadata: None,
        },
    )
    .await
    .map_err(|e| RunError {
        message: format!("Failed to run code: {}", e),
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        user_id: user_id.to_string(),
    })?;

    Ok(())
}

async fn handle_retrieval_action(
    pool: &PgPool,
    run_id: &str,
    thread_id: &str,
    file_ids: &Vec<String>,
    user_id: &str,
    action: &LLMAction,
    retrieval_chunks: &mut Vec<Chunk>,
    retrieval_files: &mut Vec<String>,
    file_storage: &FileStorage,
) -> Result<(), RunError> {
    *retrieval_files = retrieve_file_contents(&file_ids, file_storage).await;
    *retrieval_chunks = fetch_chunks(pool, action.content.to_string()).await.map_err(|e| RunError {
        message: format!("Failed to fetch chunks: {}", e),
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        user_id: user_id.to_string(),
    })?;

    Ok(())
}

pub struct ToolStep {
    pub run: Run,
    pub instructions: String,
    pub llm_output: String,
}
impl ToolStep {
    pub fn new(run: Run, instructions: String, llm_output: String) -> Self {
        Self {
            run,
            instructions,
            llm_output,
        }
    }
}

async fn use_tools(
    stream: Pin<Box<dyn Stream<Item = Result<CreateChatCompletionStreamResponse, OpenAIError>> + Send>>,
    pool: &PgPool,
    mut steps: usize,
    run_id: &str,
    thread_id: &str,
    user_id: &str,
    formatted_messages: &str,
    tools: &Vec<String>,
    tool_calls: &str,
    run_inner_required_action: Option<RequiredAction>,
    file_storage: &FileStorage,
    file_ids: &Vec<String>,
    model_name: &str,
    instructions: &str,
    code_output: &mut String,
    retrieval_chunks: &mut Vec<Chunk>,
    retrieval_files: &mut Vec<String>,
) -> Result<ToolStep, RunError> {
    let mut tag_stream = TagStream::new(Box::pin(stream));
    let mut final_output = String::new();
    while let Ok(Some((current_tag, current_tag_content, full_output))) = tag_stream.next_tag().await {
        println!("current_tag: {}, current_tag_content: {}", current_tag, current_tag_content);
        final_output = full_output.clone();
        // TODO: more robust :D e.g. might extract type above ...
        if current_tag.contains("steps") || current_tag.contains("retrieval") || current_tag.contains("function_calling") || current_tag.contains("code_interpreter") {
            let action = match current_tag.as_ref() {
                "steps" => {
                    LLMAction {
                            r#type: LLMActionType::Steps,
                            content: current_tag_content.to_string(),
                    }
                },
                "function_calling" => {
                    LLMAction {
                            r#type: LLMActionType::FunctionCalling,
                            content: current_tag_content.to_string(),
                    }
                },
                "code_interpreter" => {
                    LLMAction {
                            r#type: LLMActionType::CodeInterpreter,
                            content: current_tag_content.to_string(),
                    }
                },
                "retrieval" => {
                    LLMAction {
                            r#type: LLMActionType::Retrieval,
                            content: current_tag_content.to_string(),
                    }
                },
                _ => LLMAction {
                    r#type: LLMActionType::Unknown,
                    content: current_tag_content.to_string(),
                }
            };
            match action.r#type {
                LLMActionType::Steps => {
                    handle_steps_action(&action, &mut steps).await?;
                },
                LLMActionType::FunctionCalling => {
                    handle_function_calling_action(
                        pool,
                        thread_id,
                        run_id,
                        user_id,
                        &action,
                        run_inner_required_action.clone(),
                    )
                    .await?;
                    return Ok(ToolStep::new(get_run(pool, thread_id, run_id, user_id).await.map_err(|e| RunError {
                        message: format!("Failed to get run: {}", e),
                        run_id: run_id.to_string(),
                        thread_id: thread_id.to_string(),
                        user_id: user_id.to_string(),
                    })?, instructions.to_string(), full_output));
                },
                LLMActionType::CodeInterpreter => {
                    handle_code_interpreter_action(
                        thread_id,
                        run_id,
                        user_id,
                        formatted_messages,
                        model_name,
                        code_output,
                    )
                    .await?;
                },
                LLMActionType::Retrieval => {
                    handle_retrieval_action(
                        pool,
                        run_id,
                        thread_id,
                        file_ids,
                        user_id,
                        &action,
                        retrieval_chunks,
                        retrieval_files,
                        file_storage,
                    )
                    .await?;
                },
                _ => {
                    error!("Unknown action: {:?}", action.r#type);
                }
            }
        }
    }

    // Handle stream error
    if let Err(e) = tag_stream.next_tag().await {
        error!("Stream Error: {}", e);
        println!("Stream Error: {}", e);
        return Err(RunError {
            message: format!("Stream Error: {}", e),
            run_id: run_id.to_string(),
            thread_id: thread_id.to_string(),
            user_id: user_id.to_string(),
        });
    }

    Ok(ToolStep::new(get_run(pool, thread_id, run_id, user_id).await.map_err(|e| RunError {
        message: format!("Failed to get run: {}", e),
        run_id: run_id.to_string(),
        thread_id: thread_id.to_string(),
        user_id: user_id.to_string(),
    })?, instructions.to_string(), final_output))
}                               


#[cfg(test)]
mod tests {
    use assistants_core::runs::{get_run, create_run_and_produce_to_executor_queue};
    use async_openai::types::{
        AssistantObject, AssistantTools, AssistantToolsCode, AssistantToolsFunction,
        AssistantToolsRetrieval, ChatCompletionFunctions, MessageObject, MessageRole, RunObject, FunctionObject,
    };
    use serde_json::json;
    use sqlx::types::Uuid;

    use crate::models::{SubmittedToolCall, Function};
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
                model: "mixtral-8x7b-instruct".to_string(),
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
                model: "mixtral-8x7b-instruct".to_string(),
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
                model: "mixtral-8x7b-instruct".to_string(),
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
                model: "mixtral-8x7b-instruct".to_string(),
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
}
