use std::pin::Pin;

use assistants_core::models::Message;
use assistants_extra::llm::generate_chat_responses;
use async_openai::{
    error::OpenAIError,
    types::{CreateChatCompletionRequest, CreateChatCompletionStreamResponse, FinishReason},
};
use log::error;
use pin_project::pin_project;

use crate::{
    function_calling::ModelConfig,
    models::{LLMAction, LLMActionType},
};
use async_stream::stream;
use async_trait::async_trait;
use futures::{pin_mut, Stream, StreamExt};
use roxmltree::{Document, Node};
use tiktoken_rs::p50k_base;

// This function formats the messages into a string
pub fn format_messages(messages: &Vec<Message>) -> String {
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
///
/// Check how OpenAI does the Window management: https://platform.openai.com/docs/assistants/how-it-works/context-window-management
pub fn build_instructions(
    original_instructions: &str,
    file_contents: &Vec<String>,
    previous_messages: &str,
    tools: &Vec<String>,
    tool_calls: &str,
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
    let instructions_part = format!(
        "<instructions>\n{}\n</instructions>\n",
        original_instructions
    );
    let file_contents_part = format!("<file>\n{:?}\n</file>\n", file_contents);
    let retrieval_chunks_part = format!("<chunk>\n{:?}\n</chunk>\n", retrieval_chunks);
    let tools_part = format!("<tools>\n{:?}\n</tools>\n", tools);
    let tool_calls_part = format!("<tool_calls>\n{}\n</tool_calls>\n", tool_calls);
    let code_output_part = match code_output { // TODO: maybe different tag
        Some(output) => format!("<math_solution>\n{}\n</math_solution>\n", output),
        None => String::new(),
    };
    let previous_messages_part = format!(
        "<previous_messages>\n{}\n</previous_messages>",
        previous_messages
    );

    // Initialize the final instructions with the highest priority part
    let mut final_instructions = instructions_part;

    // List of other parts ordered by priority
    let mut other_parts = [
        tool_calls_part,
        tools_part,
        previous_messages_part,
        code_output_part,
        file_contents_part.clone(),
        retrieval_chunks_part.clone(),
    ];
    // TODO: probably this could be made customisable if someone has a usecase where code is very important for example

    // Add other parts to the final instructions if they fit in the context limit
    for part in &other_parts {
        let part_tokens = bpe.encode_with_special_tokens(part).len();
        let final_instructions_tokens = bpe.encode_with_special_tokens(&final_instructions).len();

        if final_instructions_tokens + part_tokens <= context_size {
            // If file_contents_part is already in final_instructions, do not add retrieval_chunks_part
            if part == &retrieval_chunks_part && final_instructions.contains(&retrieval_chunks_part)
            {
                continue;
            }
            final_instructions += part;
        } else {
            break;
        }
    }

    final_instructions
}

#[pin_project]
pub struct TagStream {
    #[pin]
    stream:
        Pin<Box<dyn Stream<Item = Result<CreateChatCompletionStreamResponse, OpenAIError>> + Send>>,
    buffer: String,
    full_output: String,
}

impl TagStream {
    pub fn new(
        stream: Pin<
            Box<dyn Stream<Item = Result<CreateChatCompletionStreamResponse, OpenAIError>> + Send>,
        >,
    ) -> Self {
        TagStream {
            stream,
            buffer: String::new(),
            full_output: String::new(),
        }
    }

    // Process the stream and look for complete tags
    pub async fn next_tag(&mut self) -> Result<Option<(String, String, String)>, OpenAIError> {
        while let Some(result) = self.stream.next().await {
            match result {
                Ok(response) => {
                    let first_choice = response.choices.first().unwrap();

                    if let Some(content) = &first_choice.delta.content {
                        self.buffer.push_str(&content);
                    }
                    if first_choice.finish_reason == Some(FinishReason::Stop) {
                        // If there's a stop reason, return the content in the buffer
                        let content = self.buffer.clone();
                        self.buffer.clear();
                        return Ok(Some((
                            "end_of_message".to_string(),
                            content,
                            self.full_output.clone(),
                        )));
                    }

                    if first_choice.finish_reason == Some(FinishReason::Length) {
                        // If there's a stop reason, return the content in the buffer
                        let content = self.buffer.clone();
                        self.buffer.clear();
                        return Ok(Some((
                            "end_of_message".to_string(),
                            content,
                            self.full_output.clone(),
                        )));
                    }

                    if let Some((tag, content)) = self.extract_complete_tag() {
                        // append buffer to full_output
                        self.full_output = format!("{}{}", self.full_output, self.buffer);
                        self.buffer.clear();
                        return Ok(Some((tag, content, self.full_output.clone())));
                    }
                }
                Err(e) => {
                    println!("Error: {}", e);
                    error!("Error while streaming LLM: {}", e);
                    // Return the error to the caller
                    return Err(e);
                }
            }
        }
        // If the stream ends without errors, return Ok(None)
        Ok(None)
    }

    // Extract a complete tag from the buffer
    fn extract_complete_tag(&mut self) -> Option<(String, String)> {
        if let Ok(doc) = Document::parse(&self.buffer) {
            if let Some(first_element) = doc.root().first_element_child() {
                let tag_name = first_element.tag_name().name().to_string();

                // Serialize the subtree of the element to include nested tags
                let tag_content = first_element
                    .descendants()
                    .filter_map(|n| {
                        if n.is_text() && n.parent().unwrap().eq(&first_element) {
                            Some(n.text().unwrap_or_default())
                        } else if n.is_element() && n != first_element {
                            // Serialize the element and its descendants
                            let range = n.range();
                            Some(&self.buffer[range])
                        } else {
                            None
                        }
                    })
                    .collect::<String>();

                // Calculate the length of the matched element to remove it from the buffer
                let matched_element_length = first_element.range().end;

                // Remove the processed tag from the buffer
                self.buffer.drain(..matched_element_length);

                return Some((tag_name, tag_content));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::{env, pin::Pin};

    use crate::{models::LLMActionType, prompts::TagStream};
    use assistants_core::prompts::build_instructions;
    use assistants_extra::llm::generate_chat_responses;
    use async_openai::{
        config::OpenAIConfig,
        error::OpenAIError,
        types::{
            ChatCompletionRequestUserMessageArgs, ChatCompletionResponseStreamMessage,
            ChatCompletionStreamResponseDelta, CreateChatCompletionRequestArgs,
            CreateChatCompletionStreamResponse, FinishReason,
        },
        Client,
    };
    use async_stream::stream;
    use futures::{Stream, StreamExt};
    use tiktoken_rs::p50k_base;

    #[test]
    fn test_build_instructions_context_limit() {
        let original_instructions = "Solve the quadratic equation x^2 + 5x + 6 = 0.";
        let file_contents = vec![
            "# Python script to solve quadratic equations\nimport cmath\ndef solve_quadratic(a, b, c):\n    # calculate the discriminant\n    d = (b**2) - (4*a*c)\n    # find two solutions\n    sol1 = (-b-cmath.sqrt(d))/(2*a)\n    sol2 = (-b+cmath.sqrt(d))/(2*a)\n    return sol1, sol2\n".to_string(),
            "# Another Python script\nprint('Hello, world!')\n".to_string(),
        ];
        let previous_messages = "<message>\n{\"role\": \"user\", \"content\": \"Can you solve a quadratic equation for me?\"}\n</message>\n<message>\n{\"role\": \"assistant\", \"content\": \"Sure, I can help with that. What's the equation?\"}\n</message>\n";
        let tools = vec![
            "Python".to_string(),
            "Wolfram Alpha".to_string(),
            "Mathematica".to_string(),
        ];
        let tool_calls = "<tool_call>\n{\"tool\": \"code_interpreter\", \"action\": \"run\", \"args\": {\"code\": \"x^2 + 5x + 6 = 0\", \"language\": \"python\"}}\n</tool_call>\n";
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
            &tools,
            tool_calls,
            code_output,
            &retrieval_chunks,
            Some(context_size),
        );

        // Use tiktoken to count tokens
        let bpe = p50k_base().unwrap();
        let tokens = bpe.encode_with_special_tokens(&instructions);

        // Check that the instructions do not exceed the context limit
        assert!(
            tokens.len() <= context_size,
            "The instructions exceed the context limit"
        );

        // Check that the instructions contain the most important parts
        assert!(
            instructions.contains(original_instructions),
            "The instructions do not contain the original instructions"
        );
        let t_s = format!("{:?}", tools);
        assert!(
            instructions.contains(&t_s),
            "The instructions do not contain the tools"
        );
        assert!(
            instructions.contains(previous_messages),
            "The instructions do not contain the previous messages"
        );

        // Check that the instructions do not contain the less important parts
        assert!(
            !instructions.contains(&file_contents[0]),
            "The instructions contain the file contents"
        );
        assert!(
            !instructions.contains(&retrieval_chunks[0]),
            "The instructions contain the retrieval chunks"
        );
    }

    // Helper function to create a mock stream from a vector of results
    fn mock_stream(
        responses: Vec<Result<CreateChatCompletionStreamResponse, OpenAIError>>,
    ) -> Pin<Box<dyn Stream<Item = Result<CreateChatCompletionStreamResponse, OpenAIError>> + Send>>
    {
        Box::pin(stream! {
            for response in responses {
                yield response;
            }
        })
    }

    #[tokio::test]
    async fn test_tag_stream() {
        let responses = vec![
            Ok(CreateChatCompletionStreamResponse {
                id: "1".to_string(),
                created: 1234567890,
                model: "model_name".to_string(),
                system_fingerprint: Some("fingerprint".to_string()),
                object: "object_type".to_string(),
                choices: vec![ChatCompletionResponseStreamMessage {
                    index: 0,
                    finish_reason: None,
                    delta: ChatCompletionStreamResponseDelta {
                        content: Some("<tag>content</tag>".to_string()),
                        function_call: None,
                        role: None,
                        tool_calls: None,
                    },
                }],
            }),
            Ok(CreateChatCompletionStreamResponse {
                id: "1".to_string(),
                created: 1234567890,
                model: "model_name".to_string(),
                system_fingerprint: Some("fingerprint".to_string()),
                object: "object_type".to_string(),
                choices: vec![ChatCompletionResponseStreamMessage {
                    index: 0,
                    finish_reason: None,
                    delta: ChatCompletionStreamResponseDelta {
                        content: Some("<partial>".to_string()),
                        function_call: None,
                        role: None,
                        tool_calls: None,
                    },
                }],
            }),
            Ok(CreateChatCompletionStreamResponse {
                id: "1".to_string(),
                created: 1234567890,
                model: "model_name".to_string(),
                system_fingerprint: Some("fingerprint".to_string()),
                object: "object_type".to_string(),
                choices: vec![ChatCompletionResponseStreamMessage {
                    index: 0,
                    finish_reason: None,
                    delta: ChatCompletionStreamResponseDelta {
                        content: Some("incomplete tag</partial>".to_string()),
                        function_call: None,
                        role: None,
                        tool_calls: None,
                    },
                }],
            }),
            Ok(CreateChatCompletionStreamResponse {
                id: "1".to_string(),
                created: 1234567890,
                model: "model_name".to_string(),
                system_fingerprint: Some("fingerprint".to_string()),
                object: "object_type".to_string(),
                choices: vec![ChatCompletionResponseStreamMessage {
                    index: 0,
                    finish_reason: None,
                    delta: ChatCompletionStreamResponseDelta {
                        content: Some("<nested><tag>inner content</tag></nested>".to_string()),
                        function_call: None,
                        role: None,
                        tool_calls: None,
                    },
                }],
            }),
            Ok(CreateChatCompletionStreamResponse {
                id: "1".to_string(),
                created: 1234567890,
                model: "model_name".to_string(),
                system_fingerprint: Some("fingerprint".to_string()),
                object: "object_type".to_string(),
                choices: vec![ChatCompletionResponseStreamMessage {
                    index: 0,
                    finish_reason: None,
                    delta: ChatCompletionStreamResponseDelta {
                        content: Some("hello my name is\n<name>\nbob\n</name>\ni will help you now\n<help>\nthis is".to_string()),
                        function_call: None,
                        role: None,
                        tool_calls: None,
                    },
                }],
            }),
            // This response simulates the end of the stream without a closing tag
            Ok(CreateChatCompletionStreamResponse {
                id: "1".to_string(),
                created: 1234567890,
                model: "model_name".to_string(),
                system_fingerprint: Some("fingerprint".to_string()),
                object: "object_type".to_string(),
                choices: vec![ChatCompletionResponseStreamMessage {
                    index: 0,
                    finish_reason: Some(FinishReason::Length),
                    delta: ChatCompletionStreamResponseDelta {
                        content: None, // No further content, indicating the end of the stream
                        function_call: None,
                        role: None,
                        tool_calls: None,
                    },
                }],
            }),
            // Add more responses to simulate different scenarios
        ];

        let stream = mock_stream(responses);
        let mut tag_stream = TagStream::new(stream);

        // Test for a complete tag
        if let Ok(Some((tag, content, _))) = tag_stream.next_tag().await {
            assert_eq!(tag, "tag");
            assert_eq!(content, "content");
        } else {
            panic!("Failed to extract complete tag");
        }

        // Test for a partial tag
        if let Ok(Some((tag, content, _))) = tag_stream.next_tag().await {
            assert_eq!(tag, "partial");
            assert_eq!(content, "incomplete tag");
        } else {
            panic!("Failed to extract partial tag");
        }

        // Test for nested tags
        if let Ok(Some((tag, content, _))) = tag_stream.next_tag().await {
            assert_eq!(tag, "nested");
            assert!(content.contains("<tag>inner content</tag>"));
        } else {
            panic!("Failed to extract nested tags");
        }

        // Test for a complete tag before the stop reason
        if let Ok(Some((tag, content, _))) = tag_stream.next_tag().await {
            assert_eq!(tag, "end_of_message");
            assert_eq!(
                content.trim(),
                "hello my name is\n<name>\nbob\n</name>\ni will help you now\n<help>\nthis is"
            );
        } else {
            panic!("Failed to extract complete tag before stop reason");
        }
        // Add more assertions for different scenarios
    }

    #[tokio::test]
    async fn test_tag_stream_with_real_llm_stream() {
        dotenv::dotenv().ok();
        // Set up OpenAI client
        let api_base = env::var("MODEL_URL").unwrap();
        let api_key = env::var("MODEL_API_KEY").unwrap();
        let llm_api_config = OpenAIConfig::new()
            .with_api_base(api_base)
            .with_api_key(api_key);
        let client = Client::with_config(llm_api_config);

        // Create a request
        let request = CreateChatCompletionRequestArgs::default()
            .model("mixtral-8x7b-instruct") // Use appropriate model
            .messages([ChatCompletionRequestUserMessageArgs::default()
                .content("What is Rocco's basilisk? Your messages is always structured in xml, your comments are in <comment> blocks and there are other tags i'll tell you later.")
                .build()
                .unwrap()
                .into()])
            .build()
            .unwrap();

        // Create a stream from the client
        let stream = client.chat().create_stream(request).await.unwrap();

        // Use TagStream to process the LLM stream
        let mut tag_stream = TagStream::new(Box::pin(stream));

        // Process the stream and check for tags
        while let Some((tag, content, _)) = tag_stream.next_tag().await.unwrap() {
            if tag == "end_of_message" {
                break;
            }

            // Perform assertions based on expected tags and content
            assert!(!tag.is_empty(), "Tag should not be empty: {}", tag);
            assert!(
                !content.is_empty(),
                "Content should not be empty: {}",
                content
            );
            // Add more specific assertions as needed
        }
    }
}
