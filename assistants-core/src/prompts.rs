use assistants_core::models::Message;

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
pub fn build_instructions(
    original_instructions: &str,
    retrieval_files: &Vec<String>,
    previous_messages: &str,
    function_calls: &str,
    code_output: Option<&str>,
    retrieval_chunks: &Vec<String>,
    context_size: Option<usize>,
    action_calls: &str,
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
    let retrieval_files_part = format!("<file>\n{:?}\n</file>\n", retrieval_files);
    let retrieval_chunks_part = format!("<chunk>\n{:?}\n</chunk>\n", retrieval_chunks);
    let function_calls_part = format!("<function_calls>\n{}\n</function_calls>\n", function_calls);
    let code_output_part = match code_output {
        Some(output) => format!("<math_solution>\n{}\n</math_solution>\n", output),
        None => String::new(),
    };
    let previous_messages_part = format!(
        "<previous_messages>\n{}\n</previous_messages>\n",
        previous_messages
    );
    let action_calls_part = format!("<action_calls>\n{}\n</action_calls>\n", action_calls);

    // Initialize the final instructions with the highest priority part
    let mut final_instructions = instructions_part;

    // List of other parts ordered by priority
    let mut other_parts = [
        function_calls_part,
        action_calls_part,
        previous_messages_part,
        code_output_part,
        retrieval_files_part.clone(),
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

#[cfg(test)]
mod tests {
    use assistants_core::prompts::build_instructions;
    use tiktoken_rs::p50k_base;

    #[test]
    fn test_build_instructions_context_limit() {
        let original_instructions = "Solve the quadratic equation x^2 + 5x + 6 = 0.";
        let file_contents = vec![
            "# Python script to solve quadratic equations\nimport cmath\ndef solve_quadratic(a, b, c):\n    # calculate the discriminant\n    d = (b**2) - (4*a*c)\n    # find two solutions\n    sol1 = (-b-cmath.sqrt(d))/(2*a)\n    sol2 = (-b+cmath.sqrt(d))/(2*a)\n    return sol1, sol2\n".to_string(),
            "# Another Python script\nprint('Hello, world!')\n".to_string(),
        ];
        let previous_messages = "<message>\n{\"role\": \"user\", \"content\": \"Can you solve a quadratic equation for me?\"}\n</message>\n<message>\n{\"role\": \"assistant\", \"content\": \"Sure, I can help with that. What's the equation?\"}\n</message>\n";
        let function_calls = "code_interpreter";
        let code_output = Some("The solutions are (-2+0j) and (-3+0j)");
        let context_size = 200; // Set a realistic context size
        let retrieval_chunks = vec![
            "Here's a chunk of text retrieved from a large document...".to_string(),
            "And here's another chunk of text...".to_string(),
        ];
        let action_calls = "somehting";

        let instructions = build_instructions(
            original_instructions,
            &file_contents,
            previous_messages,
            function_calls,
            code_output,
            &retrieval_chunks,
            Some(context_size),
            action_calls,
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
        assert!(
            instructions.contains(function_calls),
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
}
