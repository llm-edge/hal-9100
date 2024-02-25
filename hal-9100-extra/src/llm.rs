use hal_9100_extra::anthropic::call_anthropic_api;
use hal_9100_extra::openai::{
    call_open_source_openai_api_with_messages, call_openai_api_with_messages, Message,
};
use log::{error, info};
use std::collections::HashMap;
use std::error::Error;
use std::ops::Deref;
use tiktoken_rs::cl100k_base;

#[derive(Clone, Debug)]
pub struct HalLLMRequestArgs {
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens_to_sample: Option<i32>,
    pub stop_sequences: Option<Vec<String>>,
    pub top_p: Option<f32>,
    pub top_k: Option<i32>,
    pub metadata: Option<HashMap<String, String>>,
    pub context_size: Option<i32>,
}

impl Default for HalLLMRequestArgs {
    fn default() -> Self {
        Self {
            messages: vec![],
            temperature: Some(0.7), // Default temperature (0.7 is a good value for most cases
            max_tokens_to_sample: Some(-1), // Default max tokens to sample
            stop_sequences: None,
            top_p: None,
            top_k: None,
            metadata: None,
            context_size: None,
        }
    }
}

impl HalLLMRequestArgs {
    pub fn messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    // Get the system prompt from the messages
    pub fn get_system_prompt(&self) -> Option<String> {
        self.messages.iter().find_map(|message| {
            if message.role == "system" {
                Some(message.content.clone())
            } else {
                None
            }
        })
    }

    // Get the user prompt from the messages
    pub fn get_user_prompt(&self) -> Option<String> {
        self.messages.iter().find_map(|message| {
            if message.role == "user" {
                Some(message.content.clone())
            } else {
                None
            }
        })
    }

    // Replace the system prompt with a new one
    pub fn set_system_prompt(&mut self, new_prompt: String) -> &mut Self {
        let mut found = false;
        for message in &mut self.messages {
            if message.role == "system" {
                message.content = new_prompt.clone();
                found = true;
                break;
            }
        }
        if !found {
            self.messages.push(Message {
                role: "system".to_string(),
                content: new_prompt,
            });
        }
        self
    }

    // Replace the LAST user prompt with a new one
    pub fn set_last_user_prompt(&mut self, new_prompt: String) -> &mut Self {
        let mut found = false;
        if let Some(message) = self
            .messages
            .iter_mut()
            .rev()
            .find(|message| message.role == "user")
        {
            message.content = new_prompt.clone();
            found = true;
        }
        if !found {
            self.messages.push(Message {
                role: "user".to_string(),
                content: new_prompt,
            });
        }
        self
    }

    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn max_tokens_to_sample(mut self, max_tokens_to_sample: i32) -> Self {
        self.max_tokens_to_sample = Some(max_tokens_to_sample);
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

    pub fn context_size(mut self, context_size: i32) -> Self {
        self.context_size = Some(context_size);
        self
    }

    pub fn build(self) -> Result<Self, Box<dyn std::error::Error>> {
        // Here you can add validation logic and return Err if something is not right
        // For simplicity, we'll assume everything is fine
        Ok(self)
    }
}

// Define the Client struct
#[derive(Debug, Clone)]
pub struct HalLLMClient {
    pub model_name: String,
    pub model_url: String,
    pub api_key: String, // Assuming an API key is needed
}

impl HalLLMClient {
    // Constructor for Client
    pub fn new(model_name: String, model_url: String, api_key: String) -> Self {
        Self {
            model_name,
            model_url,
            api_key,
        }
    }

    pub fn set_model_name(&mut self, model_name: String) {
        self.model_name = model_name;
    }
    pub fn set_model_url(&mut self, model_url: String) {
        self.model_url = model_url;
    }
    pub fn set_api_key(&mut self, api_key: String) {
        self.api_key = api_key;
    }

    // TODO async backoff
    pub async fn create_chat_completion(
        &self,
        request: HalLLMRequestArgs,
    ) -> Result<String, Box<dyn Error>> {
        let mut max_tokens_to_sample = request.max_tokens_to_sample.unwrap_or(-1);

        if self.model_name.contains("claude") {
            // ! disgusting but who care about anthropic? raise your hand
            let json_messages = serde_json::to_string(&request.messages).unwrap();
            info!("Calling Claude API with messages: {:?}", json_messages);
            // if max_tokens_to_sample == -1 we just use maximum length based on current prompt
            if max_tokens_to_sample == -1 {
                let bpe = cl100k_base().unwrap();
                let tokens = bpe.encode_with_special_tokens(&json_messages);
                max_tokens_to_sample = request.context_size.unwrap_or(4096) - tokens.len() as i32;
                info!(
                    "Automatically computed max_tokens_to_sample: {:?}",
                    max_tokens_to_sample
                );
            }

            call_anthropic_api(self, request.max_tokens_to_sample(max_tokens_to_sample))
                .await
                .map(|res| res.completion)
                .map_err(|e| {
                    error!("Error calling Claude API: {}", e);
                    Box::new(e) as Box<dyn Error>
                })
        } else if self.model_name.contains("gpt") {
            info!("Calling OpenAI API with messages: {:?}", request.messages);
            if max_tokens_to_sample == -1 {
                let bpe = cl100k_base().unwrap();
                let tokens = bpe
                    .encode_with_special_tokens(&serde_json::to_string(&request.messages).unwrap());
                max_tokens_to_sample = request.context_size.unwrap_or(4096) - tokens.len() as i32;
                info!(
                    "Automatically computed max_tokens_to_sample: {}",
                    max_tokens_to_sample
                );
            }
            call_openai_api_with_messages(
                request.messages,
                max_tokens_to_sample,
                Some(self.model_name.clone()),
                request.temperature,
                request.stop_sequences,
                request.top_p,
            )
            .await
            .map(|res| res.choices[0].message.content.clone())
            .map_err(|e| {
                error!("Error calling OpenAI API: {}", e);
                Box::new(e) as Box<dyn Error>
            })
        } else {
            info!(
                "Calling Open Source LLM {:?} through OpenAI API on URL {:?} with messages: {:?}",
                self.model_name, self.model_url, request.messages
            );
            if max_tokens_to_sample == -1 {
                let bpe = cl100k_base().unwrap();
                let tokens = bpe
                    .encode_with_special_tokens(&serde_json::to_string(&request.messages).unwrap());
                max_tokens_to_sample = request.context_size.unwrap_or(4096) - tokens.len() as i32;
                info!(
                    "Automatically computed max_tokens_to_sample: {}",
                    max_tokens_to_sample
                );
            }
            call_open_source_openai_api_with_messages(
                request.messages,
                max_tokens_to_sample,
                self.model_name.clone(),
                request.temperature,
                request.stop_sequences,
                request.top_p,
                self.model_url.clone(),
            )
            .await
            .map(|res| res.choices[0].message.content.clone())
            .map_err(|e| {
                error!(
                    "Error calling Open Source {:?} LLM through OpenAI API on URL {:?}: {}",
                    self.model_name, self.model_url, e
                );
                Box::new(e) as Box<dyn Error>
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dotenv;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_llm_new() {
        let client = HalLLMClient::new(
            std::env::var("TEST_MODEL_NAME")
                .unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string()),
            std::env::var("MODEL_URL").unwrap_or_else(|_| "".to_string()),
            std::env::var("MODEL_API_KEY").unwrap_or_else(|_| "".to_string()),
        );

        let request = HalLLMRequestArgs::default()
            .messages(vec![Message {
                role: "user".to_string(),
                content: "1+1=?".to_string(),
            }])
            .temperature(0.7)
            .max_tokens_to_sample(50)
            // Add other method calls to set fields as needed
            .build()
            .unwrap();

        let response = client.create_chat_completion(request).await.unwrap();
        println!("Response: {}", response);
    }
}
