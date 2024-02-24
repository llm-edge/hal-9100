#[allow(unused_extern_crates)]
extern crate self as hal_9100_api_communication;

pub mod cli;
pub mod executor;
pub mod models;

pub mod routes {
    pub mod assistants;
    pub mod chat;
    pub mod files;
    pub mod messages;
    pub mod router;
    pub mod run_steps;
    pub mod runs;
    pub mod threads;
}
