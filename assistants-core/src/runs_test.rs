use super::*;
use async_trait::async_trait;
use mockall::mock;

mock! {
    pub DBPool {
        fn get_tool_calls(&self, tool_call_ids: Vec<&str>) -> Result<Vec<SubmittedToolCall>, sqlx::Error>;
        fn submit_tool_outputs(&self, thread_id: &str, run_id: &str, user_id: &str, tool_outputs: Vec<SubmittedToolCall>) -> Result<Run, sqlx::Error>;
    }
}

#[async_trait]
impl DBPool for MockDBPool {
    async fn get_tool_calls(&self, tool_call_ids: Vec<&str>) -> Result<Vec<SubmittedToolCall>, sqlx::Error> {
        self.get_tool_calls(tool_call_ids)
    }
    async fn submit_tool_outputs(&self, thread_id: &str, run_id: &str, user_id: &str, tool_outputs: Vec<SubmittedToolCall>) -> Result<Run, sqlx::Error> {
        self.submit_tool_outputs(thread_id, run_id, user_id, tool_outputs)
    }
}

#[tokio::test]
async fn test_get_tool_calls() {
    let mut pool = MockDBPool::new();
    let tool_call_ids = vec!["id1", "id2"];
    
    pool.expect_get_tool_calls()
        .returning(|_| Ok(vec![
            SubmittedToolCall {
                id: "id1".to_string(),
                output: "output1".to_string(),
                run_id: "run_id1".to_string(),
                created_at: 0,
                user_id: "user_id1".to_string(),
            },
            SubmittedToolCall {
                id: "id2".to_string(),
                output: "output2".to_string(),
                run_id: "run_id2".to_string(),
                created_at: 0,
                user_id: "user_id2".to_string(),
            },
        ]));
    
    let result = pool.get_tool_calls(tool_call_ids).await;
    
    assert!(result.is_ok());
    let tool_calls = result.unwrap();
    assert_eq!(tool_calls.len(), 2);
}

#[tokio::test]
async fn test_submit_tool_outputs() {
    let mut pool = MockDBPool::new();
    let thread_id = "thread_id";
    let run_id = "run_id";
    let user_id = "user_id";
    let tool_outputs = vec![
        SubmittedToolCall {
            id: "id1".to_string(),
            output: "output1".to_string(),
            run_id: "run_id1".to_string(),
            created_at: 0,
            user_id: "user_id1".to_string(),
        },
        SubmittedToolCall {
            id: "id2".to_string(),
            output: "output2".to_string(),
            run_id: "run_id2".to_string(),
            created_at: 0,
            user_id: "user_id2".to_string(),
        },
    ];
    
    pool.expect_submit_tool_outputs()
        .returning(|_, _, _, _| Ok(Run {
            inner: RunObject {
                id: "run_id".to_string(),
                thread_id: "thread_id".to_string(),
                assistant_id: Some("assistant_id".to_string()),
                instructions: "instructions".to_string(),
                created_at: 0,
                object: "object".to_string(),
                status: RunStatus::Queued,
                required_action: None,
                last_error: None,
                expires_at: None,
                started_at: None,
                cancelled_at: None,
                failed_at: None,
                completed_at: None,
                model: "model".to_string(),
                tools: vec![],
                file_ids: vec![],
                metadata: None,
            },
            user_id: "user_id".to_string(),
        }));
    
    let result = pool.submit_tool_outputs(thread_id, run_id, user_id, tool_outputs).await;
    
    assert!(result.is_ok());
}
