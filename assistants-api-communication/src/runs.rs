
// Fetch a specific run
async fn get_run_handler(
    Path((assistant_id, run_id)): Path<(i32, i32)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Run>, (StatusCode, String)> {
    // TODO: Implement the logic to fetch a specific run from the database
    Err((StatusCode::NOT_IMPLEMENTED, "Not implemented".to_string()))
}

// Update a specific run
async fn update_run_handler(
    Path((assistant_id, run_id)): Path<(i32, i32)>,
    State(app_state): State<AppState>,
    Json(run_input): Json<RunInput>,
) -> Result<JsonResponse<Run>, (StatusCode, String)> {
    // TODO: Implement the logic to update a specific run in the database
    Err((StatusCode::NOT_IMPLEMENTED, "Not implemented".to_string()))
}

// Delete a specific run
async fn delete_run_handler(
    Path((assistant_id, run_id)): Path<(i32, i32)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Run>, (StatusCode, String)> {
    // TODO: Implement the logic to delete a specific run from the database
    Err((StatusCode::NOT_IMPLEMENTED, "Not implemented".to_string()))
}

// List all runs from an assistant
async fn list_runs_handler(
    Path((assistant_id,)): Path<(i32,)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Vec<Run>>, (StatusCode, String)> {
    // TODO: Implement the logic to list all runs from a specific assistant
    Err((StatusCode::NOT_IMPLEMENTED, "Not implemented".to_string()))
}

// Router::new()
//     // ... other routes ...
//     .route("/v1/assistants/:assistant_id/runs/:run_id", get(get_run_handler))
//     .route("/v1/assistants/:assistant_id/runs/:run_id", patch(update_run_handler))
//     .route("/v1/assistants/:assistant_id/runs/:run_id", delete(delete_run_handler))
//     .route("/v1/assistants/:assistant_id/runs", get(list_runs_handler))
//     // ... other routes ...