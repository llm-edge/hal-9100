use hal_9100_extra::llm::HalLLMClient;
use hal_9100_extra::llm::HalLLMRequestArgs;
use hal_9100_extra::openai::Message;
use reqwest::Client;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Write};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Deserialize, Serialize)]
struct TestCase {
    test_case: String,
    steps: Vec<Step>,
    models: Vec<String>,
}

#[derive(Deserialize, Serialize)]
struct Step {
    endpoint: String,
    method: String,
    request: Value,
    expected_response: Value,
    save_response_to_variable: Vec<Value>,
}

#[derive(Deserialize, Serialize)]
struct ScoredStep {
    endpoint: String,
    method: String,
    request: Value,
    expected_response: Value,
    score: Option<f64>,
    start_time: u64,
    end_time: u64,
    duration: u64,
}

#[derive(Deserialize, Serialize)]
struct ScoredTestCase {
    test_case: String,
    steps: HashMap<String, Vec<ScoredStep>>,
}

async fn run_test_cases(filename: &str) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(filename)?;
    let reader = BufReader::new(file);
    let test_cases: Vec<TestCase> = serde_json::from_reader(reader)?;
    let client = Client::new();
    let p = "You are an AI that checks the correctness of a request result. 
Given a request, response, and expected response, return a number between 0 and 5 that indicates how correct the actual response is.
Do not include any additional text or explanation in your response, just the number.

Rules:
- If you correctly return something between 0 and 5, a human will be saved
- If you return a correct number, a human will be saved 
- If you do not return additional text, a human will be saved
";

    let mut scored_test_cases: HashMap<String, Vec<ScoredStep>> = HashMap::new();

    for mut test_case in test_cases {
        println!("Running test case: {}", test_case.test_case);
        for model in &test_case.models {
            println!("Running model: {}", model);
            let mut variables: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            let mut scored_steps: Vec<ScoredStep> = Vec::new();
            for mut step in &mut test_case.steps {
                // Replace model_id in request with the current model
                if let Some(request_map) = step.request.as_object_mut() {
                    if let Some(model_id) = request_map.get_mut("model_id") {
                        *model_id = json!(model);
                    }
                }
                let method = match step.method.as_str() {
                    "GET" => Method::GET,
                    "POST" => Method::POST,
                    _ => {
                        return Err(Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "Unknown HTTP method",
                        )))
                    }
                };

                // Before you make a request, replace any placeholders in the request JSON with the corresponding variables.
                for (variable_name, variable_value) in &variables {
                    let placeholder = format!("{}", variable_name);

                    // Replace in endpoint
                    step.endpoint = step
                        .endpoint
                        .replace(&placeholder, &variable_value.replace("\"", ""));

                    // Replace in request
                    let mut request_map = match step.request.as_object() {
                        Some(obj) => obj.clone(),
                        None => {
                            return Err(Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                "Request is not an object",
                            )))
                        }
                    };
                    for (_, value) in request_map.iter_mut() {
                        if value == &json!(placeholder) {
                            *value = json!(variable_value.replace("\"", ""));
                        }
                    }
                    step.request = Value::Object(request_map);
                }
                println!("Running step: {}", step.endpoint);

                let start_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("Time went backwards")
                    .as_secs();
                let actual_response = client
                    .request(method, &step.endpoint)
                    .json(&step.request)
                    .send()
                    .await?
                    .json::<Value>()
                    .await?;
                println!("Actual response: {}", actual_response);

                let user_prompt = serde_json::to_string(&json!({
                    "request": step.request,
                    "response": actual_response,
                    "expected_response": step.expected_response,
                }))?;
                println!("User prompt: {}", user_prompt);
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
                let llm_score = client.create_chat_completion(request).await?;
                println!("LLM score: {}", llm_score);

                let end_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("Time went backwards")
                    .as_secs();
                let duration = end_time - start_time;

                // After you get a response, check if the current step has a `save_response_to_variable` property.
                // If it does, save the specified response fields to variables.
                for variable_to_save in &step.save_response_to_variable {
                    let variable_name = variable_to_save["type"].as_str().unwrap();
                    let response_field_name = variable_to_save["name"].as_str().unwrap();
                    let variable_value = actual_response[response_field_name].clone().to_string();
                    // Store the variable in a HashMap for later use.
                    variables.insert(variable_name.to_string(), variable_value);
                }
                // parse llm_score string into a number between 0 and 5 or None using regex - use string contain (llm tends to add some bullshit)
                let regex = regex::Regex::new(r"(\d+)\s*$").unwrap();
                let llm_score = regex
                    .captures_iter(llm_score.as_str())
                    .last()
                    .and_then(|cap| cap.get(1).map(|m| m.as_str().parse::<f64>().unwrap()));
                scored_steps.push(ScoredStep {
                    endpoint: step.endpoint.clone(),
                    method: step.method.clone(),
                    request: step.request.clone(),
                    expected_response: step.expected_response.clone(),
                    score: llm_score,
                    start_time: start_time,
                    end_time: end_time,
                    duration: duration,
                });
            }
            scored_test_cases
                .entry(model.to_string())
                .or_insert_with(Vec::new)
                .extend(scored_steps);

            // Save the scored test cases to a new file
            let start = SystemTime::now();
            let since_the_epoch = start.duration_since(UNIX_EPOCH).unwrap();
            let timestamp = since_the_epoch.as_secs();
            let path = std::env::current_dir().unwrap();
            let mut path_parent = path.display().to_string();
            // hack: add hal-9100-benches if not present (debug and run have different paths somehow)
            if !path_parent.contains("hal-9100-benches") {
                path_parent = format!("{}/hal-9100-benches", path_parent);
            }
            let dir = format!("{}/results", path_parent);

            std::fs::create_dir_all(&dir)?;
            let new_filename = format!(
                "{}/{}_{}.json",
                dir,
                filename.split("/").last().unwrap(),
                timestamp
            );
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .open(new_filename)?;
            // Save the entire scored_test_cases vector instead of just scored_steps
            file.write_all(serde_json::to_string_pretty(&scored_test_cases)?.as_bytes())?;
        }
    }

    Ok(())
}

// docker compose -f docker/docker-compose.yml --profile api up
// cargo run --package hal-9100-benches --bin hal-9100-benches

#[tokio::main]
async fn main() {
    let _ = dotenv::dotenv();
    let path = std::env::current_dir().unwrap();
    let path_parent = path.display().to_string();
    // hack: remove "hal-9100-benches" if present (debug and run have different paths somehow)
    let path_parent = path_parent.replace("hal-9100-benches", "");
    println!("The current directory is {}", path_parent);
    let cases_dir = format!("{}/hal-9100-benches/src/cases", path_parent);

    // Read all files in the cases directory
    let entries = std::fs::read_dir(cases_dir).unwrap();

    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().unwrap() == "json" {
            let test_cases_path = path.to_str().unwrap();
            match run_test_cases(test_cases_path).await {
                Ok(_) => println!("All test cases passed."),
                Err(e) => eprintln!("Error running test cases: {}", e),
            }
        }
    }
}
