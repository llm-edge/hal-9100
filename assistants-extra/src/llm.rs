use assistants_extra::anthropic::call_anthropic_api;
use assistants_extra::openai::{
    call_open_source_openai_api_with_messages, call_openai_api_with_messages, Message,
};
use log::{error, info};
use std::collections::HashMap;
use std::error::Error;

pub async fn llm(
    model_name: &str,
    model_url: Option<String>,
    system_prompt: &str,
    user_prompt: &str,
    temperature: Option<f32>,
    max_tokens_to_sample: i32,
    stop_sequences: Option<Vec<String>>,
    top_p: Option<f32>,
    top_k: Option<i32>,
    metadata: Option<HashMap<String, String>>,
) -> Result<String, Box<dyn Error>> {
    let messages = vec![
        Message {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        },
        Message {
            role: "user".to_string(),
            content: user_prompt.to_string(),
        },
    ];
    if model_name.contains("claude") {
        let instructions = format!(
            "<system>\n{}\n</system>\n<user>\n{}\n</user>",
            system_prompt, user_prompt
        );
        info!("Calling Claude API with instructions: {}", instructions);

        call_anthropic_api(
            instructions,
            max_tokens_to_sample,
            Some(model_name.to_string()),
            temperature,
            stop_sequences,
            top_p,
            top_k,
            metadata,
        )
        .await
        .map(|res| res.completion)
        .map_err(|e| {
            error!("Error calling Claude API: {}", e);
            Box::new(e) as Box<dyn Error>
        })
    } else if model_name.contains("gpt") {
        info!("Calling OpenAI API with messages: {:?}", messages);
        call_openai_api_with_messages(
            messages,
            max_tokens_to_sample,
            Some(model_name.to_string()),
            temperature,
            stop_sequences,
            top_p,
        )
        .await
        .map(|res| res.choices[0].message.content.clone())
        .map_err(|e| {
            error!("Error calling OpenAI API: {}", e);
            Box::new(e) as Box<dyn Error>
        })
    } else if model_name.contains("/") {
        // ! super hacky
        let model_name = model_name.split('/').last().unwrap_or_default();
        let url = model_url.unwrap_or_else(|| {
            std::env::var("MODEL_URL")
                .unwrap_or_else(|_| String::from("http://localhost:8000/v1/chat/completions"))
        });
        info!(
            "Calling Open Source LLM through OpenAI API with messages: {:?}",
            messages
        );
        call_open_source_openai_api_with_messages(
            messages,
            max_tokens_to_sample,
            model_name.to_string(),
            temperature,
            stop_sequences,
            top_p,
            url,
        )
        .await
        .map(|res| res.choices[0].message.content.clone())
        .map_err(|e| {
            error!("Error calling Open Source LLM through OpenAI API: {}", e);
            Box::new(e) as Box<dyn Error>
        })
    } else {
        Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Unknown model",
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dotenv;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_llm_openai() {
        dotenv::dotenv().ok();

        let result = llm(
            "gpt-3.5-turbo",
            None,
            "You help the user discover deep truths about themselves and the world.",
            "According to the Hitchhiker guide to the galaxy, what is the meaning of life, the universe, and everything?",
            Some(0.5),
            60,
            None,
            Some(1.0),
            None,
            None,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_llm_anthropic() {
        dotenv::dotenv().ok();

        let result = llm(
            "claude-2.1",
            None,
            "You help the user discover deep truths about themselves and the world.",
            "According to the Hitchhiker guide to the galaxy, what is the meaning of life, the universe, and everything?",
            Some(0.5),
            60,
            None,
            Some(1.0),
            None,
            None,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_llm_open_source() {
        dotenv::dotenv().ok();

        let result = llm(
            "open-source/mistral-7b-instruct",
            Some("https://api.perplexity.ai/chat/completions".to_string()),
            "You help the user discover deep truths about themselves and the world.",
            "According to the Hitchhiker guide to the galaxy, what is the meaning of life, the universe, and everything?",
            Some(0.5),
            60,
            None,
            Some(1.0),
            None,
            None,
        )
        .await;
        assert!(result.is_ok(), "{:?}", result);
        let result = result.unwrap();
        // assert!(result.contains("42"));
        // println!("{:?}", result);
    }
}
