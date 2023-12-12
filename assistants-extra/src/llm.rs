use assistants_extra::anthropic::call_anthropic_api;
use assistants_extra::openai::{
    call_open_source_openai_api_with_messages, call_openai_api_with_messages, Message,
};
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
        .map_err(|e| Box::new(e) as Box<dyn Error>)
    } else if model_name.contains("gpt") {
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
        .map_err(|e| Box::new(e) as Box<dyn Error>)
    } else if model_name.contains("/") {
        // ! super hacky
        let model_name = model_name.split('/').last().unwrap_or_default();
        let url = model_url.unwrap_or_else(|| {
            std::env::var("MODEL_URL")
                .unwrap_or_else(|_| String::from("http://localhost:8000/v1/chat/completions"))
        });
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
        .map_err(|e| Box::new(e) as Box<dyn Error>)
    } else {
        Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Unknown model",
        )))
    }
}

use futures::future::join_all;

pub async fn llm_parallel(
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
    k: usize,
) -> Result<Vec<String>, Box<dyn Error>> {
    let mut futures = Vec::new();

    for _ in 0..k {
        let future = llm(
            model_name,
            model_url.clone(),
            system_prompt,
            user_prompt,
            temperature,
            max_tokens_to_sample,
            stop_sequences.clone(),
            top_p,
            top_k,
            metadata.clone(),
        );
        futures.push(future);
    }

    let results = join_all(futures).await;
    let mut responses = Vec::new();

    for result in results {
        match result {
            Ok(response) => responses.push(response),
            Err(e) => return Err(e),
        }
    }

    Ok(responses)
}

pub struct LLM {
    model_name: String,
    model_url: Option<String>,
    system_prompt: String,
    user_prompt: String,
    temperature: Option<f32>,
    max_tokens_to_sample: i32,
    stop_sequences: Option<Vec<String>>,
    top_p: Option<f32>,
    top_k: Option<i32>,
    metadata: Option<HashMap<String, String>>,
}

impl LLM {
    pub fn new(model_name: &str) -> Self {
        Self {
            model_name: model_name.to_string(),
            model_url: None,
            system_prompt: "".to_string(),
            user_prompt: "".to_string(),
            temperature: None,
            max_tokens_to_sample: 0,
            stop_sequences: None,
            top_p: None,
            top_k: None,
            metadata: None,
        }
    }

    pub fn model_url(mut self, model_url: String) -> Self {
        self.model_url = Some(model_url);
        self
    }

    pub fn system_prompt(mut self, system_prompt: String) -> Self {
        self.system_prompt = system_prompt;
        self
    }

    pub fn user_prompt(mut self, user_prompt: String) -> Self {
        self.user_prompt = user_prompt;
        self
    }

    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn max_tokens_to_sample(mut self, max_tokens_to_sample: i32) -> Self {
        self.max_tokens_to_sample = max_tokens_to_sample;
        self
    }

    pub fn stop_sequences(mut self, stop_sequences: Vec<String>) -> Self {
        self.stop_sequences = Some(stop_sequences);
        self
    }

    pub fn top_p(mut self, top_p: f32) -> Self {
        self.top_p = Some(top_p);
        self
    }

    pub fn top_k(mut self, top_k: i32) -> Self {
        self.top_k = Some(top_k);
        self
    }

    pub fn metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub async fn run(self) -> Result<String, Box<dyn Error>> {
        llm(
            &self.model_name,
            self.model_url,
            &self.system_prompt,
            &self.user_prompt,
            self.temperature,
            self.max_tokens_to_sample,
            self.stop_sequences,
            self.top_p,
            self.top_k,
            self.metadata,
        )
        .await
    }

    pub async fn run_parallel(self, k: usize) -> Result<Vec<String>, Box<dyn Error>> {
        let mut futures = Vec::new();

        for _ in 0..k {
            let future = llm(
                &self.model_name,
                self.model_url.clone(),
                &self.system_prompt,
                &self.user_prompt,
                self.temperature,
                self.max_tokens_to_sample,
                self.stop_sequences.clone(),
                self.top_p,
                self.top_k,
                self.metadata.clone(),
            );
            futures.push(future);
        }

        let results = join_all(futures).await;
        let mut responses = Vec::new();

        for result in results {
            match result {
                Ok(response) => responses.push(response),
                Err(e) => return Err(e),
            }
        }

        Ok(responses)
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

    #[tokio::test]
    async fn test_llm_parallel() {
        dotenv::dotenv().ok();
        let results = LLM::new("open-source/mistral-7b-instruct")
            .model_url("https://api.perplexity.ai/chat/completions".to_string())
            .system_prompt("You help the user discover deep truths about themselves and the world.".to_string())
            .user_prompt("According to the Hitchhiker guide to the galaxy, what is the meaning of life, the universe, and everything?".to_string())
            .temperature(0.5)
            .top_p(1.0)
            .run_parallel(5)
            .await;

        assert!(results.is_ok(), "{:?}", results);
        let results = results.unwrap();
        assert_eq!(results.len(), 5);
        for result in results {
            assert!(!result.is_empty());
        }
    }
}
