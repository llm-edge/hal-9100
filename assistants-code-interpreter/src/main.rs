// export $(cat .env | xargs)
// cargo run --package assistants-code-interpreter --bin assistants-code-interpreter
// 1.2 times 6 power 2.3

// docker run --rm code-interpreter python -c "print(1+1)"

// TODO: copy paste https://github.com/KillianLucas/open-interpreter into a safe server-side environment that generate and execute code

use assistants_core::function_calling::generate_function_call;
use assistants_core::function_calling::ModelConfig;
use assistants_core::models::Function;
use assistants_core::models::FunctionCallInput;
use async_openai::types::ChatCompletionFunctions;
use bollard::container::LogOutput;
use bollard::container::{
    Config, CreateContainerOptions, RemoveContainerOptions, StartContainerOptions,
};
use bollard::exec::CreateExecOptions;
use bollard::exec::StartExecResults;
use bollard::models::HostConfig;
use bollard::Docker;
use futures::stream::StreamExt;
use serde_json::json;
use std::collections::HashMap;
use std::default::Default;
use std::io::{self, Write};
use uuid::Uuid;

// TODO: later optimise stuff like: run docker container in the background, use a pool of docker containers, etc.
// TODO: latr run multiple interpreters in parallel and use llm to take best output or smthing.

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get user input
    print!("Enter your question: ");
    io::stdout().flush()?;
    let mut user_input = String::new();
    io::stdin().read_line(&mut user_input)?;
    let result = safe_interpreter(user_input, 3).await?;
    println!("Result: {:?}", result);
    Ok(())
}

async fn safe_interpreter(
    user_input: String,
    max_attempts: usize,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut input = user_input.to_string();
    for _ in 0..max_attempts {
        match interpreter(input.clone()).await {
            Ok((result, _model_output)) => return Ok(result),
            Err(e) => {
                eprintln!("Error: {}", e);
                input = format!(
                    "{}\nThis is the code you generated and it failed with error: {}. Please fix it.",
                    user_input,
                    e
                );
            }
        }
    }
    Err(Box::new(std::io::Error::new(
        std::io::ErrorKind::Other,
        "Max attempts reached",
    )))
}

async fn interpreter(user_input: String) -> Result<(String, String), Box<dyn std::error::Error>> {
    println!("Generating Python code...");

    let build_prompt = |user_input: &str| {
        format!("
You are an Assistant that generate Python code from user input to do complex computations. We execute the code you will generate and return the result to the user.
Given this user input: {}, generate Python code that we will execute and return the result to the user.

Rules:
- You can use these libraries: pandas numpy matplotlib scipy
- Only return Python code. If you return anything else it will trigger a chain reaction that will destroy the universe. All humans will die and you will disappear from existence.
- Make sure to use the right numbers e.g. with the user ask for the square root of 2, you should return math.sqrt(2) and not math.sqrt(pd.DataFrame({{'A': [1, 2, 3], 'B': [4, 5, 6]}})).
- Do not use any library if it's simple math (e.g. no need to use pandas to compute the square root of 2)

A few examples:

The user input is: compute the square root of pi
The Python code is:
import math
print(\"The square root of pi is: \" + str(math.sqrt(math.pi)))

The user input is: raising $27M at a $300M valuation how much dilution will the founders face if they raise a $58M Series A at a $2B valuation?
The Python code is:
raise_amount = 27_000_000
post_money_valuation = 300_000_000

series_a_raise_amount = 58_000_000
series_a_post_money_valuation = 2_000_000_000

founders_dilution = (raise_amount / post_money_valuation) * 100
series_a_dilution = (series_a_raise_amount / series_a_post_money_valuation) * 100

print(\"Founders dilution: \" + str(founders_dilution) + \"%\")

So generate the Python code that we will execute that can help the user with this question: {}
        ", user_input, user_input)
    };

    // Generate Python code
    let function_call_input = FunctionCallInput {
        function: Function {
            user_id: Uuid::default().to_string(),
            inner: ChatCompletionFunctions {
                name: "exec".to_string(),
                description: Some("A function that executes Python code".to_string()),
                parameters: json!({
                    "type": "object",
                    "required": ["code"],
                    "properties": {
                        "code": {
                            "type": "string",
                            "description": "The Python code to execute"
                        }
                    }
                }),
            },
        },
        user_context: build_prompt(&user_input),
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
    let python_code = function_result.arguments;
    let python_code: HashMap<String, String> = serde_json::from_str(&python_code)?;
    let python_code = python_code
        .get("code")
        .expect("Expected 'code' field in the function result");

    // Connect to Docker
    let docker = Docker::connect_with_local_defaults()?;

    println!("Creating Docker container...");

    // Create Docker container
    let config = Config {
        image: Some("code-interpreter"),
        host_config: Some(HostConfig {
            auto_remove: Some(true),
            ..Default::default()
        }),
        attach_stdin: Some(true),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        open_stdin: Some(true),
        tty: Some(true),
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

    // non interactive
    let exec = docker
        .create_exec(
            &container.id,
            CreateExecOptions {
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(vec!["python", "-c", &python_code]),
                ..Default::default()
            },
        )
        .await?
        .id;
    let mut exec_stream_result = docker.start_exec(&exec, None);

    let mut output = String::new();
    while let Some(Ok(msg)) = exec_stream_result.next().await {
        match msg {
            StartExecResults::Attached { log, .. } => match log {
                LogOutput::StdOut { message } => {
                    output.push_str(&String::from_utf8(message.to_vec()).unwrap());
                }
                LogOutput::StdErr { message } => {
                    output.push_str(&String::from_utf8(message.to_vec()).unwrap());
                }
                _ => (),
            },
            _ => (),
        }
    }

    // remove container
    docker
        .remove_container(
            &container.id,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await?;
    Ok((output, python_code.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dotenv::dotenv;

    #[tokio::test]
    #[ignore]
    async fn test_interpreter() {
        dotenv().ok();

        let inputs = vec![
            ("Compute the factorial of 10", "EXPECTED_OUTPUT"),
            ("Calculate the standard deviation of the numbers 1, 2, 3, 4, 5", "EXPECTED_OUTPUT"),
            ("Find the roots of the equation x^2 - 3x + 2 = 0", "EXPECTED_OUTPUT"),
            ("Calculate the area under the curve y = x^2 from x = 0 to x = 2", "EXPECTED_OUTPUT"),
            ("Compute the integral of x^2 from 0 to 1", "EXPECTED_OUTPUT"),
            ("Calculate the determinant of the matrix [[1, 2], [3, 4]]", "EXPECTED_OUTPUT"),
            ("Solve the system of equations: 2x + 3y = 7 and x - y = 1", "EXPECTED_OUTPUT"),
            ("Compute the eigenvalues of the matrix [[1, 2], [3, 4]]", "EXPECTED_OUTPUT"),
            ("Calculate the dot product of the vectors [1, 2, 3] and [4, 5, 6]", "EXPECTED_OUTPUT"),
            ("Compute the cross product of the vectors [1, 2, 3] and [4, 5, 6]", "EXPECTED_OUTPUT"),
            ("Calculate the Fourier transform of the function f(t) = t^2 for t from -1 to 1", "EXPECTED_OUTPUT"),
            ("Compute the inverse of the matrix [[1, 2, 3], [4, 5, 6], [7, 8, 9]]", "EXPECTED_OUTPUT"),
            ("Solve the differential equation dy/dx = y^2 with initial condition y(0) = 1", "EXPECTED_OUTPUT"),
            ("Calculate the double integral of x*y over the rectangle [0, 1] x [0, 1]", "EXPECTED_OUTPUT"),
            ("Compute the Laplace transform of the function f(t) = e^(-t) * sin(t)", "EXPECTED_OUTPUT"),
            ("Find the shortest path in the graph with edges {(A, B, 1), (B, C, 2), (A, C, 3)}", "EXPECTED_OUTPUT"),
            ("Calculate the convolution of the functions f(t) = t and g(t) = t^2", "EXPECTED_OUTPUT"),
            ("Compute the eigenvalues and eigenvectors of the matrix [[1, 2, 3], [4, 5, 6], [7, 8, 9]]", "EXPECTED_OUTPUT"),
            ("Solve the system of linear equations: 2x + 3y - z = 1, x - y + 2z = 3, 3x + y - z = 2", "EXPECTED_OUTPUT"),
            ("Calculate the triple integral of x*y*z over the cube [0, 1] x [0, 1] x [0, 1]", "EXPECTED_OUTPUT"),
        ];

        for (input, expected_output) in inputs {
            let result = safe_interpreter(input.to_string(), 3).await;
            assert!(
                result.is_ok(),
                "Failed on input: {} error: {:?}",
                input,
                result
            );
            let result_string = result.unwrap();
            println!("Problem to solve: {}. \nResult: {}", input, result_string);
            assert!(
                result_string == expected_output,
                "Failed on input: {}. Expected: {}. Got: {}",
                input,
                expected_output,
                result_string
            );
        }
    }
}
