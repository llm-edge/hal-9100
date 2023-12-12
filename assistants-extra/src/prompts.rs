use assistants_extra::llm::llm;
use dotenv::dotenv;
use std::collections::HashMap;
use std::error::Error;

async fn generate_text(prompt: &str, k: i32) -> Result<Vec<String>, Box<dyn Error>> {
    let mut thoughts = Vec::new();
    for _ in 0..k {
        let response = llm(
            "gpt-3.5-turbo",
            None,
            "You help the user discover deep truths about themselves and the world.",
            prompt,
            Some(0.5),
            60,
            None,
            Some(1.0),
            None,
            None,
        )
        .await?;
        thoughts.push(response);
    }
    Ok(thoughts)
}

async fn generate_thoughts(
    state: &str,
    k: i32,
    initial_prompt: &str,
    rejected_solutions: Option<Vec<String>>,
) -> Result<Vec<String>, Box<dyn Error>> {
    let prompt = format!(
        "You're an TreeofThoughts, an superintelligent AI model devoted to helping Humans by any means necessary. You're purpose is to generate a series of solutions to comply with the user's instructions, you must generate solutions on the basis of determining the most reliable solution in the shortest amount of time, while taking rejected solutions into account and learning from them. 
        Considering the reasoning provided:\n\n
        ###'{}'\n\n###
        Devise the best possible solution for the task: {}, Here are evaluated solutions that were rejected: 
        ###{:?}###, 
        complete the {} without making the same mistakes you did with the evaluated rejected solutions. Be simple. Be direct. Provide intuitive solutions as soon as you think of them.",
        state, initial_prompt, rejected_solutions, initial_prompt
    );
    generate_text(&prompt, k).await
}

async fn generate_solution(
    initial_prompt: &str,
    state: &str,
    rejected_solutions: Option<Vec<String>>,
) -> Result<String, Box<dyn Error>> {
    let prompt = format!(
        "You're an TreeofThoughts, an superintelligent AI model devoted to helping Humans by any means necessary. You're purpose is to generate a series of solutions to comply with the user's instructions, you must generate solutions on the basis of determining the most reliable solution in the shortest amount of time, while taking rejected solutions into account and learning from them. 
        Considering the reasoning provided:\n\n
        ###'{}'\n\n###
        Devise the best possible solution for the task: {}, Here are evaluated solutions that were rejected: 
        ###{:?}###, 
        complete the {} without making the same mistakes you did with the evaluated rejected solutions. Be simple. Be direct. Provide intuitive solutions as soon as you think of them.",
        state, initial_prompt, rejected_solutions, initial_prompt
    );
    let thoughts = generate_text(&prompt, 1).await?;
    Ok(thoughts[0].clone())
}

async fn evaluate_states(
    states: Vec<String>,
    initial_prompt: &str,
) -> Result<HashMap<String, f32>, Box<dyn Error>> {
    let mut state_values = HashMap::new();
    for state in states {
        let prompt = format!(
            "To achieve the following goal: '{}', pessimistically value the context of the past solutions and more importantly the latest generated solution you had AS A FLOAT BETWEEN 0 AND 1\n
            Past solutions:\n\n
            {}\n       
            If the solutions is not directly concretely making fast progress in achieving the goal, give it a lower score.
            Evaluate all solutions AS A FLOAT BETWEEN 0 and 1:\n,  DO NOT RETURN ANYTHING ELSE",
            initial_prompt, state
        );
        let response = generate_text(&prompt, 1).await?;
        let value: f32 = response[0].parse().unwrap_or(0.0);
        state_values.insert(state, value);
    }
    Ok(state_values)
}

#[tokio::test]
async fn test_generate_thoughts() {
    dotenv().ok();
    let thoughts = generate_thoughts(
        "According to the Hitchhiker guide to the galaxy, what is the meaning of life, the universe, and everything?",
        3,
        "Find the meaning of life",
        Some(vec!["42".to_string()]),
    )
    .await
    .unwrap();
    println!("{:?}", thoughts);
}
