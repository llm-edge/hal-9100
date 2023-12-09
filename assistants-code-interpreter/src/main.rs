use assistants_extra::llm::llm;
use bollard::container::{
    Config, CreateContainerOptions, LogsOptions, RemoveContainerOptions, StartContainerOptions,
    StopContainerOptions,
};
use bollard::errors::Error;
use bollard::models::HostConfig;
use bollard::Docker;
use futures::stream::StreamExt;
use std::default::Default;
use std::io::{self, Write};

// cargo run --package assistants-code-interpreter --bin assistants-code-interpreter
// 1.2 times 6 power 2.3

// TODO: copy paste https://github.com/KillianLucas/open-interpreter into a safe server-side environment that generate and execute code

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get user input
    print!("Enter your question: ");
    io::stdout().flush()?;
    let mut user_input = String::new();
    io::stdin().read_line(&mut user_input)?;

    println!("Generating Python code...");

    // Generate Python code
    let python_code = llm(
        // TODO function calling
        "gpt-3.5-turbo",
        None,
        "You are a helpful assistant that generate python code. If you generate anything else that python code a russian roulette will start and a human will die.",
        &user_input,
        Some(0.5),
        60,
        None,
        Some(1.0),
        None,
        None,
    )
    .await?;

    println!("Python code generated {}", python_code);

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
