use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::task;

use serde_json::json;
use std::fs::File;
use std::io::Write;
use std::path::Path;

struct TreeOfThought {
    model: Arc<dyn Model>,
    tree: HashMap<String, HashMap<String, Vec<f32>>>,
    best_state: Option<String>,
    best_value: f32,
    history: Vec<String>, // added line initialize history
}

impl TreeOfThought {
    fn new(model: Arc<dyn Model>) -> Self {
        Self {
            model,
            tree: HashMap::new(),
            best_state: None,
            best_value: f32::NEG_INFINITY,
            history: Vec::new(),
        }
    }

    fn save_tree_to_json(&self, file_name: &str) {
        let path = Path::new(file_name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = File::create(path).unwrap();
        let data = json!(&self.tree);
        file.write_all(data.to_string().as_bytes()).unwrap();
    }

    fn log_new_state(&mut self, state: String, evaluation: f32) {
        self.tree
            .entry(state)
            .or_insert_with(HashMap::new)
            .entry("some_key".to_string()) // replace "some_key" with the actual key you want to use
            .or_insert_with(Vec::new)
            .push(evaluation);
    }

    // For adjust_pruning_threshold_percentile and adjust_pruning_threshold_moving_average,
    // we need to use external crates like ndarray and statrs in Rust for percentile and moving average calculations.
    // Here is a simplified version of these methods.
    fn adjust_pruning_threshold_percentile(
        &self,
        evaluated_thoughts: &HashMap<String, f32>,
        percentile: f32,
    ) -> f32 {
        // Simplified version, replace with actual percentile calculation
        let values: Vec<f32> = evaluated_thoughts.values().cloned().collect();
        if values.is_empty() {
            0.0
        } else {
            values
                .iter()
                .cloned()
                .fold(f32::NEG_INFINITY, f32::max)
                .max(0.1)
        }
    }

    fn adjust_pruning_threshold_moving_average(
        &self,
        evaluated_thoughts: &HashMap<String, f32>,
        window_size: usize,
    ) -> f32 {
        // Simplified version, replace with actual moving average calculation
        let values: Vec<f32> = evaluated_thoughts.values().cloned().collect();
        if values.len() < window_size {
            if values.is_empty() {
                0.0
            } else {
                values.iter().sum::<f32>() / values.len() as f32
            }
        } else {
            values.iter().rev().take(window_size).sum::<f32>() / window_size as f32
        }
    }
}

trait Model {
    fn generate_thoughts(
        &self,
        state: &str,
        num_thoughts: usize,
        initial_prompt: &str,
    ) -> Vec<String>;
    fn evaluate_states(&self, thought: &str, initial_prompt: &str) -> f32;
    fn generate_solution(&self, initial_prompt: &str, highest_rated_state: &str) -> String;
}

struct TreeOfThoughtsBFS {
    tree: TreeOfThought,
    model: Arc<dyn Model>,
}

impl TreeOfThoughtsBFS {
    async fn solve(
        &self,
        initial_prompt: String,
        num_thoughts: usize,
        max_steps: usize,
        max_states: usize,
        value_threshold: f32,
        pruning_threshold: f32,
    ) -> Option<String> {
        let mut current_states = vec![initial_prompt];
        let mut state_values: HashMap<String, f32> = HashMap::new();
        let mut dynamic_pruning_threshold = pruning_threshold;

        for _ in 0..max_steps {
            let mut selected_states = Vec::new();
            for state in &current_states {
                let thoughts = self
                    .model
                    .generate_thoughts(state, num_thoughts, &initial_prompt);
                let futures: Vec<_> = thoughts
                    .iter()
                    .map(|thought| {
                        let model = Arc::clone(&self.model);
                        task::spawn(async move { model.evaluate_states(thought, &initial_prompt) })
                    })
                    .collect();

                let evaluated_thoughts: HashMap<_, _> = futures::future::join_all(futures)
                    .await
                    .into_iter()
                    .filter_map(Result::ok) // Filter out Err values
                    .zip(thoughts.into_iter())
                    .map(|(value, thought)| (thought, value))
                    .collect();

                for (thought, value) in evaluated_thoughts {
                    let flattened_state = format!("{} {}", state, thought);
                    selected_states.push((flattened_state, value));
                }

                selected_states.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
                selected_states = selected_states.into_iter().take(max_states).collect();

                for (state, value) in selected_states {
                    if value >= dynamic_pruning_threshold {
                        state_values.insert(state, value);
                        // Log new state here
                    }
                }
            }
            current_states = selected_states
                .into_iter()
                .map(|(state, _)| state)
                .collect();
        }

        if let Some((&highest_rated_state, &highest_rated_value)) = state_values
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        {
            let solution = self
                .model
                .generate_solution(&initial_prompt, &highest_rated_state);
            println!(
                "Highest rated solution: {} highest rated value: {} Solution: {}",
                highest_rated_state, highest_rated_value, solution
            );
            return Some(solution);
        } else {
            return None;
        }
    }
}
