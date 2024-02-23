use hal_9100_extra::anthropic::call_anthropic_api;
use hal_9100_extra::openai::{
    call_open_source_openai_api_with_messages, call_openai_api_with_messages, Message,
};
use log::{error, info};
use std::collections::HashMap;
use std::error::Error;
use tiktoken_rs::p50k_base;
// TODO async backoff
// TODO unsure if worthwhile to use async openai here due to nonopenai llms
pub async fn llm(
    model_name: &str,
    model_url: Option<String>,
    system_prompt: &str,
    user_prompt: &str,
    temperature: Option<f32>,
    mut max_tokens_to_sample: i32,
    stop_sequences: Option<Vec<String>>,
    top_p: Option<f32>,
    top_k: Option<i32>,
    metadata: Option<HashMap<String, String>>,
    context_size: Option<i32>,
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
        // if max_tokens_to_sample == -1 we just use maximum length based on current prompt
        if max_tokens_to_sample == -1 {
            let bpe = p50k_base().unwrap();
            let tokens = bpe.encode_with_special_tokens(&instructions);
            max_tokens_to_sample = context_size.unwrap_or(4096) - tokens.len() as i32;
            info!(
                "Automatically computed max_tokens_to_sample: {}",
                max_tokens_to_sample
            );
        }

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
        if max_tokens_to_sample == -1 {
            let bpe = p50k_base().unwrap();
            let tokens = bpe.encode_with_special_tokens(&serde_json::to_string(&messages).unwrap());
            max_tokens_to_sample = context_size.unwrap_or(4096) - tokens.len() as i32;
            info!(
                "Automatically computed max_tokens_to_sample: {}",
                max_tokens_to_sample
            );
        }
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
    } else {
        let url = model_url.unwrap_or_else(|| {
            std::env::var("MODEL_URL")
                .unwrap_or_else(|_| String::from("http://localhost:8000/v1/chat/completions"))
        });
        info!(
            "Calling Open Source LLM {:?} through OpenAI API on URL {:?} with messages: {:?}",
            model_name, url, messages
        );
        if max_tokens_to_sample == -1 {
            let bpe = p50k_base().unwrap();
            let tokens = bpe.encode_with_special_tokens(&serde_json::to_string(&messages).unwrap());
            max_tokens_to_sample = context_size.unwrap_or(4096) - tokens.len() as i32;
            info!(
                "Automatically computed max_tokens_to_sample: {}",
                max_tokens_to_sample
            );
        }
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dotenv;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_llm() {
        dotenv::dotenv().ok();
        let model_name = std::env::var("TEST_MODEL_NAME")
            .unwrap_or_else(|_| "mistralai/mixtral-8x7b-instruct".to_string());

        let system_prompt = "System prompt";
        let user_prompt = "User prompt";
        let temperature = Some(0.5);
        let max_tokens_to_sample = 50;
        // let stop_sequences = Some(vec!["\n".to_string()]);
        let top_p = Some(0.9);
        let top_k = Some(50);
        let metadata = Some(HashMap::new());
        let context_size = Some(4096);
        let res = llm(
            &model_name,
            None,
            system_prompt,
            user_prompt,
            temperature,
            max_tokens_to_sample,
            None,
            top_p,
            top_k,
            metadata,
            context_size,
        )
        .await;
        assert!(res.is_ok(), "Error: {:?}", res.err());
        info!("Result: {}", res.unwrap());
    }
}
