use assistants_core::models::Chunk;
use assistants_extra::llm::llm;
use log::error;
use log::info;
use serde_json::{self, Value};
use sqlx::types::JsonValue;
use sqlx::PgPool;
use std::collections::HashMap;
use std::error::Error;
use tiktoken_rs::p50k_base;

use assistants_core::file_storage::FileStorage;
use assistants_core::pdf_utils::pdf_mem_to_text;

use assistants_core::models::PartialChunk;

// logic

// Function to split a string into smaller chunks
pub fn split_into_chunks(text: &str, chunk_size: usize) -> Vec<PartialChunk> {
    let bpe = p50k_base().unwrap();
    let tokens = bpe.encode_with_special_tokens(text);

    let mut chunks = Vec::new();

    for (sequence, chunk) in tokens.chunks(chunk_size).enumerate() {
        let chunk_str = bpe.decode(chunk.to_vec()).unwrap();
        let start_index = (sequence * chunk_size) as i32;
        let end_index = start_index + chunk_str.len() as i32;

        chunks.push(PartialChunk {
            sequence: sequence as i32,
            data: chunk_str,
            start_index,
            end_index,
        });
    }

    chunks
}

// TODO: embeddings using either Huggingface Candle or an API
// Function to insert chunks into the database
pub async fn split_and_insert(
    pool: &PgPool,
    text: &str,
    chunk_size: usize,
    file_id: &str,
    metadata: Option<HashMap<String, Value>>,
) -> Result<Vec<Chunk>, sqlx::Error> {
    let chunks = split_into_chunks(text, chunk_size);
    let chunks_data: Vec<(i32, String, String, i32, i32, Value)> = chunks
        .into_iter()
        .enumerate()
        .map(|(_, chunk)| {
            (
                chunk.sequence,
                chunk.data,
                file_id.to_string(),
                chunk.start_index,
                chunk.end_index,
                serde_json::to_value(metadata.clone()).unwrap(),
            )
        })
        .collect();

    let mut tx = pool.begin().await?;

    for (sequence, chunk, file_id, start_index, end_index, metadata) in chunks_data {
        sqlx::query!(
            r#"
                    INSERT INTO chunks (sequence, data, file_id, start_index, end_index, metadata)
                    VALUES ($1, $2, $3, $4, $5, $6)
                    "#,
            sequence,
            chunk,
            file_id,
            start_index,
            end_index,
            metadata
        )
        .execute(&mut *tx)
        .await?;
    }

    // get the chunks from the database
    let chunks = sqlx::query!(
        r#"
        SELECT * FROM chunks WHERE file_id = $1
        "#,
        file_id,
    )
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(chunks
        .into_iter()
        .map(|row| Chunk {
            id: row.id,
            sequence: row.sequence,
            data: row.data,
            file_id: row.file_id,
            start_index: row.start_index,
            end_index: row.end_index,
            metadata: serde_json::from_value(row.metadata.unwrap_or_default()).unwrap(),
            created_at: row.created_at,
        })
        .collect())
}

pub async fn generate_queries_and_fetch_chunks(
    pool: &PgPool,
    context: &str,
    model_name: &str,
) -> Result<Vec<Chunk>, Box<dyn Error>> {
    // Generate full-text search queries using the llm() function
    let query = llm(
        model_name,
        None, // TODO: better prompt + testing/benchmarking over multiple contexts and llms
        "You are a helpful assistant that generate full-text search queries for the user.
You must return return the best full-text search query for the given context to solve the user's problem.

Your output will be used in the following code:

// Convert the query to tsquery and execute it on the database
let rows = sqlx::query!(
    r#\"
    SELECT sequence, data, file_name, metadata FROM chunks 
    WHERE to_tsvector(data) @@ to_tsquery($1)
    \"#,
    query,
)
.fetch_all(pool)
.await?;

Where \"query\" is your output.

Rules:
- If your output is not correctly a string containing a full-text search query, the universe will be terminated.
- If your output is not a valid full-text search query, i will kill a human.
- Only return a query, NOTHING ELSE OR I WILL KILL A HUMAN.

1. Healthcare: the output could be \"heart & disease | stroke\".

2. Finance: the output could be \"stocks | bonds\".

3. Education: the output could be \"mathematics | physics\".

4. Automotive: the output could be \"sedan | SUV\".

5. Agriculture: the output could be \"organic | conventional & farming\".

Query?",
        context,
        Some(0.0),
        -1,
        None,
        None,
        None,
        None,
        None,
    )
    .await?;

    // Convert the query to tsquery and execute it on the database
    let rows = sqlx::query!(
        r#"
        SELECT * FROM chunks 
        WHERE to_tsvector(data) @@ to_tsquery($1)
        "#,
        query,
    )
    .fetch_all(pool)
    .await?;

    let chunks = rows
        .into_iter()
        .map(|row| Chunk {
            id: row.id,
            sequence: row.sequence,
            data: row.data,
            file_id: row.file_id,
            start_index: row.start_index,
            end_index: row.end_index,
            metadata: match row.metadata {
                Some(JsonValue::Object(map)) => {
                    Some(map.into_iter().collect::<HashMap<String, JsonValue>>())
                }
                _ => None,
            },
            created_at: row.created_at,
        })
        .collect();

    Ok(chunks)
}

// TODO: kinda dirty function could be better
// This function retrieves file contents given a list of file_ids
pub async fn retrieve_file_contents(
    file_ids: &Vec<String>,
    file_storage: &FileStorage,
) -> Vec<String> {
    info!("Retrieving file contents for file_ids: {:?}", file_ids);
    let mut file_contents = Vec::new();
    for file_id in file_ids {
        let file_string_content = match file_storage.retrieve_file(file_id).await {
            Ok(file_byte_content) => {
                // info!("Retrieved file from storage: {:?}", file_byte_content);
                // Check if the file is a PDF
                if file_id.ends_with(".pdf") {
                    // If it's a PDF, extract the text
                    match pdf_mem_to_text(&file_byte_content) {
                        Ok(text) => text,
                        Err(e) => {
                            error!("Failed to extract text from PDF: {}", e);
                            continue;
                        }
                    }
                } else {
                    // If it's not a PDF, use the content as is (bytes to string)
                    match String::from_utf8(file_byte_content.to_vec()) {
                        Ok(text) => text,
                        Err(e) => {
                            error!("Failed to convert bytes to string: {}", e);
                            continue;
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to retrieve file: {}", e);
                continue; // Skip this iteration and move to the next file
            }
        };
        file_contents.push(file_string_content);
    }
    file_contents
}

#[cfg(test)]
mod tests {
    use super::*;
    use dotenv::dotenv;
    use sqlx::postgres::PgPoolOptions;
    use std::env;
    use std::io::Write;
    use tokio::io::AsyncWriteExt;

    async fn setup() -> PgPool {
        dotenv().ok();

        let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .unwrap()
    }
    async fn reset_db(pool: &PgPool) {
        sqlx::query!(
            "TRUNCATE assistants, threads, messages, runs, functions, tool_calls, chunks RESTART IDENTITY"
        )
        .execute(pool)
        .await
        .unwrap();
    }
    #[test]
    fn test_split_into_chunks() {
        let text = "This is a test string for splitting into chunks.
The president of the United States is Donald Trump.

The president of France is Emmanuel Macron.

The president of Mars is Elon Musk.

The president of the Moon is TDB.
";
        let chunk_size = 5;
        let chunks = split_into_chunks(text, chunk_size);

        assert_eq!(chunks.len(), 11, "Incorrect number of chunks");

        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(
                chunk.sequence, i as i32,
                "Incorrect sequence number for chunk"
            );
        }
    }

    #[tokio::test]
    async fn test_insert_chunks_into_db() {
        dotenv().ok();
        let pool = setup().await;
        reset_db(&pool).await;
        // Test data
        let text =
            "This is a test string that will be split into chunks and inserted into the database.";
        let chunk_size = 5;
        let file_name = "test_file";
        let metadata = Some(HashMap::new());

        // Call the function
        let result = split_and_insert(&pool, text, chunk_size, file_name, metadata).await;

        // Check the result
        assert!(result.is_ok(), "Failed to insert chunks into database");

        let chunks = result.unwrap();

        // Check the chunks
        assert_eq!(chunks.len(), 4, "Incorrect number of chunks");
    }

    #[tokio::test]
    async fn test_generate_queries_and_fetch_chunks() {
        dotenv().ok();
        let pool = setup().await;
        reset_db(&pool).await;

        // Insert chunks into the database
        let text = "Once upon a time, in the bustling city of San Francisco, a young startup founder named Alex was on a mission. His idea? Disrupt the pet food industry with AI-driven, personalized meal plans for dogs. He called it 'BarkByte'. Alex was a hacker at heart, but he knew the importance of funding. So, he found himself in the sleek, intimidating office of a VC firm, 'CashCow Capital'. The VC, a seasoned player named Richard, was intrigued. 'An AI for dog food, huh? That's... unique.' Alex, undeterred by Richard's skepticism, launched into his pitch. He spoke of market sizes, growth rates, and unit economics. But most importantly, he spoke of his vision - a world where every dog, be it a pampered poodle or a scrappy stray, had access to nutrition that was just right for them. Richard, who was usually hard to impress, found himself nodding along. Maybe it was Alex's passion, or maybe it was the fact that Richard's own dog, a chubby corgi, could do with a better diet. Either way, by the end of the meeting, Alex had secured his first round of funding. And thus, BarkByte was born.";
        let chunk_size = 5;
        let file_name = "test_file";
        let metadata = Some(HashMap::new());
        let _ = split_and_insert(&pool, text, chunk_size, file_name, metadata.clone())
            .await
            .unwrap();

        // Call the function
        let context = "dog food";
        let result =
            generate_queries_and_fetch_chunks(&pool, context, "open-source/mistral-7b-instruct")
                .await;

        // Check the result
        assert!(
            result.is_ok(),
            "Failed to generate queries and fetch chunks {:?}",
            result.err().unwrap()
        );

        let chunks = result.unwrap();

        // Check the chunks
        assert!(!chunks.is_empty(), "No chunks returned");
    }

    #[tokio::test]
    async fn test_retrieve_file_contents() {
        let pool = setup().await;
        reset_db(&pool).await;

        // Create a temporary file.
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        writeln!(temp_file, "Hello, world!").unwrap();

        // Get the path of the temporary file.
        let temp_file_path = temp_file.path();

        // Create a new FileStorage instance.
        let fs = FileStorage::new().await;

        // Upload the file.
        let file_id = fs.upload_file(&temp_file_path).await.unwrap();

        // Retrieve the file.
        let file_id_clone = file_id.clone();
        let file_contents = retrieve_file_contents(&vec![file_id], &fs).await;

        // Check that the retrieval was successful and the content is correct.
        assert_eq!(file_contents, vec!["Hello, world!\n"]);

        // Delete the file.
        fs.delete_file(&file_id_clone).await.unwrap();
    }

    #[tokio::test]
    async fn test_retrieve_file_contents_pdf() {
        setup().await;
        // Setup
        let file_storage = FileStorage::new().await;

        let url = "https://arxiv.org/pdf/2311.10122.pdf";
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/58.0.3029.110 Safari/537.3")
            .build()
            .unwrap();
        let response = client.get(url).send().await.unwrap();

        let bytes = response.bytes().await.unwrap();
        let mut out = tokio::fs::File::create("2311.10122.pdf").await.unwrap();
        out.write_all(&bytes).await.unwrap();
        out.sync_all().await.unwrap(); // Ensure all bytes are written to the file

        let file_path = file_storage
            .upload_file(std::path::Path::new("2311.10122.pdf"))
            .await
            .unwrap();

        // Retrieve the file contents
        let file_contents =
            retrieve_file_contents(&vec![String::from(file_path)], &file_storage).await;

        // Check the file contents
        assert!(
            file_contents[0].contains("Abstract"),
            "The PDF content should contain the word 'Abstract'. Instead, it contains: {}",
            file_contents[0]
        );
        // Check got the end of the pdf too!
        assert!(
            file_contents[0].contains("For Image Understanding As shown in Fig"),
            "The PDF content should contain the word 'Abstract'. Instead, it contains: {}",
            file_contents[0]
        );

        // Delete the file locally
        std::fs::remove_file("2311.10122.pdf").unwrap();
    }
}
