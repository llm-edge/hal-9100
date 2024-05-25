// cargo run --package hal-9100-code-interpreter --bin hal-9100-code-interpreter
// 1.2 times 6 power 2.3

// docker run --rm louis030195/hal-9100-code-interpreter python -c "print(1+1)"

use async_openai::types::FunctionObject;
use async_recursion::async_recursion;

use bollard::container::LogOutput;
use bollard::container::{
    Config, CreateContainerOptions, RemoveContainerOptions, StartContainerOptions,
};
use bollard::exec::CreateExecOptions;
use bollard::exec::StartExecResults;
use bollard::image::CreateImageOptions;
use bollard::models::HostConfig;
use bollard::Docker;
use futures::stream::StreamExt;
use futures::TryStreamExt;
use hal_9100_core::function_calling::generate_function_call;
use hal_9100_core::models::Function;
use hal_9100_core::models::FunctionCallInput;
use hal_9100_extra::llm::{HalLLMClient, HalLLMRequestArgs};
use log::info;
use serde_json::json;
use std::collections::HashMap;
use std::default::Default;
use uuid::Uuid;

// TODO: later optimise stuff like: run docker container in the background, use a pool of docker containers, etc.
// TODO: latr run multiple interpreters in parallel and use llm to take best output or smthing.
// TODO: latr annotations
// TODO: multi step - e.g. generate code, execute, then give result to next llm call, etc. LLM decide how many iterations it wants to do.

use std::fmt;

#[derive(Debug)]
pub struct InterpreterError {
    message: String,
    python_code: String,
}

impl fmt::Display for InterpreterError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}\nPython code: {}", self.message, self.python_code)
    }
}

impl From<bollard::errors::Error> for InterpreterError {
    fn from(err: bollard::errors::Error) -> InterpreterError {
        InterpreterError {
            message: format!("Docker error: {}", err),
            python_code: String::new(),
        }
    }
}

impl From<serde_json::Error> for InterpreterError {
    fn from(err: serde_json::Error) -> InterpreterError {
        InterpreterError {
            message: format!("JSON error: {}", err),
            python_code: String::new(),
        }
    }
}

impl std::error::Error for InterpreterError {}

#[async_recursion]
pub async fn safe_interpreter(
    user_input: String,
    attempt: usize,
    max_attempts: usize,
    client: HalLLMClient,
    request: HalLLMRequestArgs,
) -> Result<(String, String), InterpreterError> {
    if attempt >= max_attempts {
        return Err(InterpreterError {
            message: String::from("Max attempts reached"),
            python_code: String::new(),
        });
    }

    match interpreter(client.clone(), request.clone()).await {
        Ok((code_output, code)) => Ok((code_output, code)),
        Err(e) => {
            eprintln!("Error: {}", e);
            let input = format!(
                "{}\n<error>You generated \n<code>\n{}\n</code>\n and it failed with error: {}. Please generate a DIFFERENT code that works.<error>",
                user_input, e.python_code, e.message
            );
            safe_interpreter(input, attempt + 1, max_attempts, client, request).await
        }
    }
}

async fn interpreter(
    client: HalLLMClient,
    mut request: HalLLMRequestArgs,
) -> Result<(String, String), InterpreterError> {
    info!("Generating Python code...");

    let user_input = request.get_user_prompt().unwrap();
    // ! TODO: should use system prompt?
    let build_prompt = |user_input: &str| {
        format!("
You are an Assistant that generate Python code to based user request to do complex computations. We execute the code you will generate and return the result to the user.
Given this user request

<user>

{}

</user>

Generate Python code that we will execute and return the result to the user.

Rules:
- You can use these libraries: mathm, pandas, numpy, matplotlib, scipy. Do not use functions or code you have no access to.
- Only return Python code. If you return anything else it will trigger a chain reaction that will destroy the universe. All humans will die and you will disappear from existence.
- Make sure to use the right numbers e.g. with the user ask for the square root of 2, you should return math.sqrt(2) and not math.sqrt(pd.DataFrame({{'A': [1, 2, 3], 'B': [4, 5, 6]}})).
- Do not use any library if it's simple math (e.g. no need to use pandas to compute the square root of 2)
- Sometimes the user provide you an error, make sure to write a Python code that will work
- IF YOU DO NOT FIX YOUR CODE THAT ERRORED A HUMAN WILL DIE. DO NOT GENERATE THE SAME CODE THAT PREVIOUSLY FAILED
- DO NOT USE ```python YOUR CODE...``` (CODE BLOCKS) OR A HUMAN WILL DIE
- ALWAYS USE SINGLE QUOTES IN YOUR CODE (e.g. '), NOT DOUBLE QUOTES OR A HUMAN WILL DIE
- Always try to simplify the math problem you're given by generating code that will compute simpler numbers. Your answer might be used by another Assistant that might not be good at math.
- Be extra careful with escaping, this is wrong for example: import pandas as pd\\nprices = pd.read\\_csv('prices.csv')\\nprice\\_with\\_highest\\_demand = startups['demand'].idxmax()\\nprint(price\\_with\\_highest\\_demand)
- Make sure to use existing files, by default there is no files written on disk. DO NOT TRY TO READ FILES. YOU DONT HAVE ANY FILES. DO NOT DO THINGS LIKE: pd.read_csv('startups.csv')
- Make sure to surround strings by single quotes, e.g. don't do this: print(Hello world) but do this: print('Hello world')

A few examples:

The user input is: compute the square root of pi
Your output should be:
import math
print('The square root of pi is: ' + str(math.sqrt(math.pi)))

The user input is: raising $27M at a $300M valuation how much dilution will the founders face if they raise a $58M Series A at a $2B valuation?
Your output should be:
raise_amount = 27_000_000
post_money_valuation = 300_000_000

series_a_raise_amount = 58_000_000
series_a_post_money_valuation = 2_000_000_000

founders_dilution = (raise_amount / post_money_valuation) * 100
series_a_dilution = (series_a_raise_amount / series_a_post_money_valuation) * 100

print('Founders dilution: ' + str(founders_dilution) + '%')

So generate the Python code that we will execute that can help the user with his request.

Bad example:

The user input is: Calculate the standard deviation of the numbers 1, 2, 3, 4, 5
Your output was:
import math
numbers = (1, 2, 3, 4, 5)
std_dev = math.sqrt(sum((x-mean)**2 for x in numbers)/len(numbers))
print('The standard deviation is: ' + str(std_dev))

It's bad because \"mean\" function is not defined. You could have used: \"mean = sum(numbers) / len(numbers)\".

<user>

{}

</user>
        ", user_input, user_input)
    };

    request.set_last_user_prompt(build_prompt(&user_input));

    // Generate Python code
    let function_call_input = FunctionCallInput {
        function: Function {
            metadata: None,
            assistant_id: Uuid::default().to_string(), // ! ??
            user_id: Uuid::default().to_string(),
            inner: FunctionObject {
                name: "exec".to_string(),
                description: Some("A function that executes Python code".to_string()),
                parameters: Some(json!({
                    "type": "object",
                    "required": ["code"],
                    "properties": {
                        "code": {
                            "type": "string",
                            "description": "The Python code to execute"
                        }
                    }
                })),
            },
        },
        client,
        request: request.temperature(0.0),
    };

    let function_result = generate_function_call(function_call_input)
        .await
        .map_err(|e| InterpreterError {
            message: format!("Failed to generate Python code at function call: {}", e),
            python_code: String::new(),
        })?;
    println!("Function result: {:?}", function_result);
    let python_code = function_result.arguments;
    let python_code: HashMap<String, String> = serde_json::from_str(&python_code)?;
    let python_code = python_code
        .get("code")
        .expect("Expected 'code' field in the function result");
    let python_code = python_code.replace("```python", "").replace("```", "");

    // Connect to Docker
    let docker = Docker::connect_with_local_defaults()?;
    // Pull the image from GHCR
    docker
        .create_image(
            Some(CreateImageOptions {
                from_image: "louis030195/hal-9100-code-interpreter:latest",
                ..Default::default()
            }),
            None,
            None,
        )
        .try_collect::<Vec<_>>()
        .await?;

    // Create Docker container
    let config = Config {
        image: Some("louis030195/hal-9100-code-interpreter:latest"),
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

    info!("Starting Docker container...");

    // Start Docker container
    docker
        .start_container(&container.id, None::<StartContainerOptions<String>>)
        .await?;

    // Write Python code to a file in the Docker container and execute it
    let python_file_path = "/tmp/script.py";
    let bash_command = format!(
        "echo -e \"{}\" > {} && python {}",
        python_code, python_file_path, python_file_path
    );

    let exec = docker
        .create_exec(
            &container.id,
            CreateExecOptions {
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(vec!["/bin/bash", "-c", &bash_command]),
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

    info!("Code interpreter output: {}", output);

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

    // Check if the output contains "Traceback", indicating a Python error
    if output.contains("Traceback") {
        return Err(InterpreterError {
            message: format!("Python code execution failed with error: {}", output),
            python_code: python_code.to_string(),
        });
    }

    Ok((output, python_code.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_openai::types::{
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
        ChatCompletionRequestUserMessage, ChatCompletionRequestUserMessageContent, Role,
    };
    use dotenv::dotenv;
    use hal_9100_extra::openai::Message;

    #[tokio::test]
    #[ignore]
    async fn test_interpreter() {
        dotenv().ok();

        let inputs = vec![
            ("Compute the factorial of 10", "3628800"),
            (
                "Calculate the standard deviation of the numbers 1, 2, 3, 4, 5",
                "1.414",
            ),
            // TODO:
            // ("Find the roots of the equation x^2 - 3x + 2 = 0", "2, 1"),
            // ("Calculate the area under the curve y = x^2 from x = 0 to x = 2", "2.67"),
            // ("Compute the integral of x^2 from 0 to 1", "0.333"),
            // ("Calculate the determinant of the matrix [[1, 2], [3, 4]]", "-2"),
            // ("Solve the system of equations: 2x + 3y = 7 and x - y = 1", "2, 1"),
            // ("Compute the eigenvalues of the matrix [[1, 2], [3, 4]]", "-0.372 and 5.372."),
            // ("Calculate the dot product of the vectors [1, 2, 3] and [4, 5, 6]", "32"),
            // ("Compute the cross product of the vectors [1, 2, 3] and [4, 5, 6]", "[-3,6,-3]"),
            // ("Calculate the Fourier transform of the function f(t) = t^2 for t from -1 to 1", "cannot"),
            // ("Compute the inverse of the matrix [[1, 2, 3], [4, 5, 6], [7, 8, 9]]", "not invertible"),
            // ("Solve the differential equation dy/dx = y^2 with initial condition y(0) = 1", "The solution to the differential equation \\( \\frac{dy}{dx} = y^2 \\) with the initial condition \\( y(0) = 1 \\) is \\( y(x) = -\\frac{1}{x - 1} \\)."),
            // ("Calculate the double integral of x*y over the rectangle [0, 1] x [0, 1]", "The double integral of \\( x \\cdot y \\) over the rectangle \\([0, 1] \\times [0, 1]\\) is \\(\\frac{1}{4}\\)."),
            // ("Compute the Laplace transform of the function f(t) = e^(-t) * sin(t)", "The Laplace transform of the function \\( f(t) = e^{-t} \\cdot \\sin(t) \\) is \\(\\frac{1}{(s + 1)^2 + 1}\\)."),
            // ("Find the shortest path in the graph with edges {(A, B, 1), (B, C, 2), (A, C, 3)}", "The shortest path in the graph with edges \\(\\{(A, B, 1), (B, C, 2), (A, C, 3)\\}\\) from A to C is directly from A to C with a path length of 3."),
            // ("Calculate the convolution of the functions f(t) = t and g(t) = t^2", "The convolution of the functions \\( f(t) = t \\) and \\( g(t) = t^2 \\) results in an undefined or non-finite value using the standard convolution integral method. This often happens when the integral does not converge."),
            // ("Compute the eigenvalues and eigenvectors of the matrix [[1, 2, 3], [4, 5, 6], [7, 8, 9]]", "The eigenvalues of the matrix \\(\\begin{bmatrix} 1 & 2 & 3 \\\\ 4 & 5 & 6 \\\\ 7 & 8 & 9 \\end{bmatrix}\\) are approximately \\(16.1168\\), \\(-1.1168\\), and \\(-9.76 \\times 10^{-16}\\) (which is effectively zero due to numerical precision). The corresponding eigenvectors are: - For the eigenvalue \\(16.1168\\): \\([-0.232, -0.525, -0.819]\\) - For the eigenvalue \\(-1.1168\\): \\([-0.786, -0.087, 0.612]\\) - For the eigenvalue \\(-9.76 \\times 10^{-16}\\): \\([0.408, -0.816, 0.408]\\)."),
            // ("Solve the system of linear equations: 2x + 3y - z = 1, x - y + 2z = 3, 3x + y - z = 2", "The solution to the system of linear equations \\(2x + 3y - z = 1\\), \\(x - y + 2z = 3\\), and \\(3x + y - z = 2\\) is \\(x = 1\\), \\(y \\approx 0\\) (effectively zero), and \\(z = 1\\)."),
            // ("Calculate the triple integral of x*y*z over the cube [0, 1] x [0, 1] x [0, 1]", "The triple integral of \\( x \\cdot y \\cdot z \\) over the cube \\([0, 1] \\times [0, 1] \\times [0, 1]\\) is \\(\\frac{1}{8}\\)."),
        ];

        let model_name = std::env::var("TEST_MODEL_NAME")
            .unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string());
        let model_url =
            std::env::var("MODEL_URL").unwrap_or_else(|_| "http://localhost:8000".to_string());

        let client = HalLLMClient::new(
            model_name,
            model_url,
            std::env::var("MODEL_API_KEY").unwrap_or_else(|_| "".to_string()),
        );
        let anthropic_client = HalLLMClient::new(
            "claude-2.1".to_string(),
            "".to_string(),
            std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| "".to_string()),
        );
        for (input, expected_output) in inputs {
            let request =
                HalLLMRequestArgs::default().messages(vec![ChatCompletionRequestMessage::User(
                    ChatCompletionRequestUserMessage {
                        role: Role::User,
                        content: ChatCompletionRequestUserMessageContent::Text(input.to_string()),
                        name: None,
                    },
                )]);
            let result = safe_interpreter(input.to_string(), 0, 3, client.clone(), request).await;
            assert!(
                result.is_ok(),
                "Failed on input: {} error: {:?}",
                input,
                result
            );
            let (code_output, code) = result.unwrap();
            println!(
                "Problem to solve: {}. \nOutput: {}\nExpected output: {}",
                input, code_output, expected_output
            );

            let p = "You are an AI that checks the correctness of math results. 
Given the user input and the result, return '1' if the result seems correct, and '0' if it seems incorrect. 
Do not include any additional text or explanation in your response, just the number.

Rules:
- If you return something else than '1' or '0' my product will crash and my user will be very angry and i will lose money
- If you return '0' on a correct result my product will crash and my user will be very angry and i will lose money
- If you return '1' on an incorrect result my product will crash and my user will be very angry and i will lose money
";
            // New: Check with Claude LLM
            let request = HalLLMRequestArgs::default()

                .messages(vec![
                    ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
                        role: Role::System,
                        content: p.to_string(),
                        name: None,
                    }),
                    ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                        role: Role::User,
                        content: ChatCompletionRequestUserMessageContent::Text(format!(
                            "User input: {}\nResult: {}. Official solution: {}. Is my result correct?",
                            input, code_output, expected_output
                        )),
                        name: None,
                    }),
                ])
                .temperature(0.0)
                .max_tokens_to_sample(100);

            let claude_check = anthropic_client.create_chat_completion(request).await;
            assert!(
                claude_check.is_ok(),
                "Failed on input: {} error: {:?}",
                input,
                claude_check
            );
            let claude_check = claude_check.unwrap();
            println!("Claude LLM check: {}", claude_check);
            assert!(
                claude_check.trim() == "1",
                "Claude LLM disagreed on input: {}. Got: {}",
                input,
                claude_check
            );
        }
    }
}
