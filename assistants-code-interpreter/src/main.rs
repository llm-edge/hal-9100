// export $(cat .env | xargs)
// cargo run --package assistants-code-interpreter --bin assistants-code-interpreter
// 1.2 times 6 power 2.3

// docker run --rm code-interpreter python -c "print(1+1)"

// TODO: copy paste https://github.com/KillianLucas/open-interpreter into a safe server-side environment that generate and execute code

use assistants_core::function_calling::generate_function_call;
use assistants_core::function_calling::Function;
use assistants_core::function_calling::FunctionCallInput;
use assistants_core::function_calling::ModelConfig;
use assistants_core::function_calling::Parameter;
use assistants_core::function_calling::Property;
use assistants_extra::llm::llm;
use bollard::container::{
    Config, CreateContainerOptions, LogsOptions, RemoveContainerOptions, StartContainerOptions,
    StopContainerOptions,
};
use bollard::errors::Error;
use bollard::models::HostConfig;
use bollard::Docker;
use futures::stream::StreamExt;
use std::collections::HashMap;
use std::default::Default;
use std::io::{self, Write};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get user input
    print!("Enter your question: ");
    io::stdout().flush()?;
    let mut user_input = String::new();
    io::stdin().read_line(&mut user_input)?;

    println!("Generating Python code...");

    let build_prompt = |user_input: &str| {
        format!("
Given this user input: {}, generate Python code that we will execute and return the result to the user.

Rules:
- You can use these libraries: pandas numpy matplotlib scipy
- Only return Python code. If you return anything else it will trigger a chain reaction that will destroy the universe. All humans will die and you will disappear from existence.

A few examples:

The user input is: compute the square root of pi
The Python code is:
import math
print(math.sqrt(math.pi))

The user input is: raising $27M at a $300M valuation how much dilution will the founders face if they raise a $58M Series A at a $2B valuation?
The Python code is:
raise_amount = 27_000_000
post_money_valuation = 300_000_000

series_a_raise_amount = 58_000_000
series_a_post_money_valuation = 2_000_000_000

founders_dilution = (raise_amount / post_money_valuation) * 100
series_a_dilution = (series_a_raise_amount / series_a_post_money_valuation) * 100

print(\"Founders dilution: \" + str(founders_dilution) + \"%\")
        ", user_input)
    };

    // Generate Python code
    let function_call_input = FunctionCallInput {
        function: Function {
            user_id: "user1".to_string(),
            name: "exec".to_string(),
            description: "A function that executes Python code".to_string(),
            parameters: Parameter {
                r#type: String::from("object"),
                required: Some(vec![String::from("code")]),
                properties: {
                    let mut map = HashMap::new();
                    map.insert(
                        String::from("code"),
                        Property {
                            r#type: String::from("string"),
                            description: Some(String::from("The Python code to execute")),
                            r#enum: None,
                        },
                    );
                    Some(map)
                },
            },
        },
        user_context: user_input.to_string(),
        model_config: ModelConfig {
            model_name: String::from("open-source/llama-2-70b-chat"),
            model_url: Some("https://api.perplexity.ai/chat/completions".to_string()),
            user_prompt: user_input.clone(), // not used imho
            temperature: Some(0.0),
            max_tokens_to_sample: 200,
            stop_sequences: None,
            top_p: Some(1.0),
            top_k: None,
            metadata: None,
        },
    };

    let function_result = generate_function_call(function_call_input).await?;
    println!("Function result: {:?}", function_result);
    let python_code = function_result.parameters.unwrap();
    let python_code = python_code.get("code").unwrap();

    println!("Python code generated {:?}", python_code);

    // Connect to Docker
    let docker = Docker::connect_with_local_defaults()?;

    println!("Creating Docker container...");

    // Create Docker container
    let config = Config {
        image: Some("code-interpreter"),
        cmd: Some(vec!["python", "-c", &python_code]),
        host_config: Some(HostConfig {
            auto_remove: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    };
    let options = CreateContainerOptions {
        name: "my-python-container",
    };
    let container = docker.create_container(Some(options), config).await?;

    println!("Starting Docker container...");

    // Start Docker container
    docker
        .start_container(&container.id, None::<StartContainerOptions<String>>)
        .await?;

    println!("Docker container started. Fetching the logs...");

    // Fetch the logs
    let mut logs = docker.logs::<String>(
        &container.id,
        Some(LogsOptions {
            stdout: true,
            stderr: true,
            ..Default::default()
        }),
    );
    while let Some(log_output) = logs.next().await {
        match log_output {
            Ok(output) => println!("{}", output),
            Err(e) => eprintln!("Error reading logs: {}", e),
        }
    }

    println!("Logs fetched. Removing the Docker container...");

    // Stop Docker container
    docker
        .stop_container(&container.id, None::<StopContainerOptions>)
        .await?;

    // Remove Docker container
    docker
        .remove_container(&container.id, None::<RemoveContainerOptions>)
        .await?;

    println!("Docker container removed.");

    Ok(())
}
