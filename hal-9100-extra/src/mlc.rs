use mlc_llm::chat_module::{ChatMessage, ChatModule};
use tokio::sync::mpsc;
use futures::stream::Stream;
use std::error::Error;

pub async fn call_mlc_llm_with_messages(
    messages: Vec<ChatCompletionRequestMessage>,
    max_tokens_to_sample: i32,
    model: Option<String>,
    temperature: Option<f32>,
    stop_sequences: Option<Vec<String>>,
    top_p: Option<f32>,
) -> Result<String, Box<dyn Error>> {
    let model_path = model.unwrap_or_else(|| "/path/to/Llama2-13B-q8f16_1".to_string());
    let device = "mps".to_string(); // Adjust as needed

    let cm = ChatModule::new(&model_path, &device, None)?;

    let chat_messages: Vec<ChatMessage> = messages
        .into_iter()
        .map(|msg| ChatMessage::new(&msg.role, &msg.content))
        .collect();

    let output = cm.generate(chat_messages, None)?;
    Ok(output)
}

pub async fn call_mlc_llm_with_messages_stream(
    messages: Vec<ChatCompletionRequestMessage>,
    max_tokens_to_sample: i32,
    model: String,
    temperature: Option<f32>,
    stop_sequences: Option<Vec<String>>,
    top_p: Option<f32>,
) -> Result<impl Stream<Item = Result<String, Box<dyn Error>>>, Box<dyn Error>> {
    let model_path = model;
    let device = "mps".to_string(); // Adjust as needed

    let cm = ChatModule::new(&model_path, &device, None)?;

    let chat_messages: Vec<ChatMessage> = messages
        .into_iter()
        .map(|msg| ChatMessage::new(&msg.role, &msg.content))
        .collect();

    let (tx, rx) = mpsc::channel(1024);

    tokio::spawn(async move {
        let output = cm.generate(chat_messages, None);
        match output {
            Ok(result) => {
                let _ = tx.send(Ok(result)).await;
            }
            Err(e) => {
                let _ = tx.send(Err(Box::new(e) as Box<dyn Error>)).await;
            }
        }
    });

    Ok(rx)
}