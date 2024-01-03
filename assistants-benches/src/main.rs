use assistants_extra::llm::llm;
use async_openai::types::RunObject;
use async_openai::types::RunStatus;
use jsonpath_rust::JsonPathFinder;
use reqwest::Client;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Write};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

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

Score:";

    let mut scored_test_cases: HashMap<String, Vec<ScoredStep>> = HashMap::new();

    for mut test_case in test_cases {
        println!("Running test case: {}", test_case.test_case);
        for model in &test_case.models {
            println!("Running model: {}", model);
            let mut variables: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            let mut scored_steps: Vec<ScoredStep> = Vec::new();
            for mut step in &mut test_case.steps {
                // if endpoint finish by /assistants, replace the model property by the current model
                if step.endpoint.ends_with("/assistants") {
                    step.request["model"] = json!(model);
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
                let llm_score = llm(
                    "claude-2.1",
                    None,
                    p,
                    &user_prompt,
                    Some(0.5),
                    -1,
                    None,
                    Some(1.0),
                    None,
                    None,
                    Some(16_000),
                )
                .await?;
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
                    let response_field_path = variable_to_save["name"].as_str().unwrap();

                    // Create a JsonPathFinder with the actual response and the desired path
                    let finder =
                        JsonPathFinder::from_str(&actual_response.to_string(), response_field_path)
                            .unwrap();

                    // Use the finder to get the desired value
                    let variable_value = finder.find();

                    // Convert to string, assuming only need string atm - and remove "[" and "]" and "\""
                    let variable_value = variable_value
                        .to_string()
                        .replace("[", "")
                        .replace("]", "")
                        .replace("\"", "");

                    // Store the variable in a HashMap for later use.
                    variables.insert(variable_name.to_string(), variable_value.to_string());
                }
                // parse llm_score string into a number between 0 and 5 or None using regex - use string contain (llm tends to add some bullshit)
                let regex = regex::Regex::new(r"(\d+)\s*$").unwrap();
                let llm_score = regex
                    .captures_iter(llm_score.as_str())
                    .last()
                    .and_then(|cap| {
                        cap.get(1)
                            .map(|m| m.as_str().parse::<f64>().ok().unwrap_or_default())
                    });
                scored_steps.push(ScoredStep {
                    endpoint: step.endpoint.clone(),
                    method: step.method.clone(),
                    request: step.request.clone(),
                    expected_response: step.expected_response.clone(),
                    score: llm_score,
                    start_time,
                    end_time,
                    duration,
                });

                let thread_id = variables
                    .get("thread_id")
                    .unwrap_or(&"".to_string())
                    .replace("\"", "");
                let run_id = variables
                    .get("run_id")
                    .unwrap_or(&"".to_string())
                    .replace("\"", "");

                // After making the request, poll the run status until it's "completed" or "requires_action"
                let mut poll_count = 0;
                loop {
                    // If run_id and thread_id are both not present in the response, skip the rest of the steps in this test case.
                    if thread_id.len() == 0
                        || run_id.len() == 0
                        || thread_id == "null"
                        || run_id == "null"
                    {
                        eprintln!("Run ID or thread ID is null, no need to poll for run status");
                        break;
                    }
                    if poll_count >= 10 {
                        eprintln!(
                            "Exceeded maximum polling attempts, skipping this use case/model run"
                        );
                        break;
                    }

                    println!(
                        "Polling for run status with thread_id: {}, run_id: {}",
                        thread_id, run_id
                    );
                    let run_status_response = client
                        .get(&format!(
                            "http://localhost:3000/threads/{}/runs/{}",
                            thread_id, run_id
                        ))
                        .send()
                        .await?
                        .json::<RunObject>()
                        .await?;
                    println!("Run status response: {:?}", run_status_response);

                    let status = run_status_response.status;
                    match status {
                        RunStatus::Completed | RunStatus::RequiresAction => break,
                        RunStatus::Failed => {
                            // TODO: should handle better this use case
                            eprintln!("Run failed, skipping this use case/model run");
                            break;
                        }
                        _ => {
                            // If the status is anything else (e.g. "running"), wait a bit and then continue the loop
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                            poll_count += 1;
                            continue;
                        }
                    }
                }
            }
            scored_test_cases
                .entry(model.to_string())
                .or_insert_with(Vec::new)
                .extend(scored_steps);
        }
        // Save the scored test cases to a new file
        let start = SystemTime::now();
        let since_the_epoch = start.duration_since(UNIX_EPOCH).unwrap();
        let timestamp = since_the_epoch.as_secs();
        let path = std::env::current_dir().unwrap();
        let mut path_parent = path.display().to_string();
        // hack: add assistants-benches if not present (debug and run have different paths somehow)
        if !path_parent.contains("assistants-benches") {
            path_parent = format!("{}/assistants-benches", path_parent);
        }
        let dir = format!("{}/v0_bench_results", path_parent);

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

    Ok(())
}

// docker-compose -f docker/docker-compose.yml --profile api up
// best because non indempotent: make reboot && make all
// cargo run --package assistants-benches
// TODOs: function calling in weird state atm - basically function name is unique in db so cannot create multiple functions with same name

#[tokio::main]
async fn main() {
    let _ = dotenv::dotenv();
    let path = std::env::current_dir().unwrap();
    let path_parent = path.display().to_string();
    // hack: remove "assistants-benches" if present (debug and run have different paths somehow)
    let path_parent = path_parent.replace("assistants-benches", "");
    println!("The current directory is {}", path_parent);
    let cases_dir = format!("{}/assistants-benches/src/cases", path_parent);

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
