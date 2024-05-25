use hal_9100_core::models::Chunk;
use hal_9100_extra::llm::HalLLMClient;
use hal_9100_extra::llm::HalLLMRequestArgs;
use log::error;
use log::info;
use serde_json::{self, Value};
use sqlx::types::JsonValue;
use sqlx::PgPool;
use std::collections::HashMap;
use std::error::Error;
use tiktoken_rs::cl100k_base;

use hal_9100_core::file_storage::minio_storage::MinioStorage;
use hal_9100_core::pdf_utils::pdf_mem_to_text;

use hal_9100_core::models::PartialChunk;

use crate::file_storage::file_storage::FileStorage;

// logic

// Function to split a string into smaller chunks
pub fn split_into_chunks(text: &str, chunk_size: usize) -> Vec<PartialChunk> {
    let bpe = cl100k_base().unwrap();
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
    pool: &PgPool, text: &str, chunk_size: usize, file_id: &str, metadata: Option<HashMap<String, Value>>,
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
    pool: &PgPool, client: HalLLMClient, mut request: HalLLMRequestArgs,
) -> Result<Vec<Chunk>, Box<dyn Error>> {
    // Generate full-text search queries using the llm() function
    let p = "You are a helpful assistant that generate full-text search queries for the user.
You must return return the best full-text search query for the given context to solve the user's problem.

Your output will be used in the following code:

// Convert the query to tsquery and execute it on the database
let rows = sqlx::query!(
    r#\"
    SELECT * FROM chunks 
    WHERE to_tsvector(data) @@ to_tsquery($1)
    \"#,
    // This is where your answer will be used, so make sure to keep the right format
    query,
)
.fetch_all(pool)
.await?;

Where \"query\" is your output.

Rules:
- If your output is not correctly a string containing a full-text search query, the universe will be terminated.
- If your output is not a valid full-text search query, this will be the end of the universe.
- Do not add SPACES between words, only use the | character or & character to combine words.
- Only return a query, NOTHING ELSE, do not comment on the query! or the big crunch will be initiated.

Bad examples:

1. Math: wrong output: \"To | find | solutions | to | the | equation | `3x | +11 | = | 14`, | a | full-text | search | query ...\"

2. Engineering: wrong output: \"neuromorphic computing\" instead of \"neuromorphic | computing\".

3. Greentech: wrong output: \"solar panels | PDF manual | deal with | installation | maintenance\" instead of \"solar | panels | PDF | manual | deal | with | installation | maintenance\".

Good examples:

1. Healthcare: your output could be \"heart & disease | stroke\".

2. Finance: your output could be \"stocks | bonds\".

3. Education: your output could be \"mathematics | physics\".

4. Automotive: your output could be \"sedan | SUV\".

5. Agriculture: your output could be \"organic | conventional & farming\".

Query:";

    request.set_system_prompt(p.to_string());
    let query = client.create_chat_completion(request).await?;

    // TODO: bad processing
    // if the llm return two words like "dog food", just add a | between them
    // using a regex for safety
    // let re = regex::Regex::new(r"\s+").unwrap();
    // let query = re.replace_all(&query, " | ").to_string();

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
                Some(JsonValue::Object(map)) => Some(map.into_iter().collect::<HashMap<String, JsonValue>>()),
                _ => None,
            },
            created_at: row.created_at,
        })
        .collect();

    Ok(chunks)
}

// TODO: kinda dirty function could be better
// This function retrieves file contents given a list of file_ids
pub async fn retrieve_file_contents(file_ids: &Vec<String>, file_storage: &dyn FileStorage) -> Vec<String> {
    info!("Retrieving file contents for file_ids: {:?}", file_ids);
    let mut file_contents = Vec::new();
    for file_id in file_ids {
        let file_string_content = match file_storage.get_file_content(file_id).await {
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
    use crate::file_storage;
    use crate::file_storage::file_storage::FileStorage;

    use super::*;
    use dotenv::dotenv;
    use hal_9100_extra::config::Hal9100Config;
    use sqlx::postgres::PgPoolOptions;
    use sqlx::{Pool, Postgres};
    use std::env;
    use std::io::Write;
    use tokio::io::AsyncWriteExt;

    async fn setup() -> (
        Pool<Postgres>,
        hal_9100_extra::config::Hal9100Config,
        Box<dyn FileStorage>,
    ) {
        dotenv().ok();
        let hal_9100_config = Hal9100Config::default();
        let database_url = hal_9100_config.database_url.clone();
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to create pool.");
        // Initialize the logger with an info level filter
        match env_logger::builder().filter_level(log::LevelFilter::Info).try_init() {
            Ok(_) => (),
            Err(_) => (),
        };
        return (
            pool,
            hal_9100_config.clone(),
            Box::new(MinioStorage::new(hal_9100_config).await),
        );
    }
    async fn reset_db(pool: &PgPool) {
        sqlx::query!(
            "TRUNCATE assistants, threads, messages, runs, functions, tool_calls, chunks, run_steps RESTART IDENTITY"
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

        assert_eq!(chunks.len(), 9, "Incorrect number of chunks");

        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.sequence, i as i32, "Incorrect sequence number for chunk");
        }
    }

    #[tokio::test]
    async fn test_insert_chunks_into_db() {
        dotenv().ok();
        let (pool, _, __) = setup().await;
        reset_db(&pool).await;
        // Test data
        let text = "This is a test string that will be split into chunks and inserted into the database.";
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
        let (pool, _, __) = setup().await;
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
        let llm_client = HalLLMClient::new(
            std::env::var("TEST_MODEL_NAME").unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string()),
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY must be set"),
        );
        let mut request = HalLLMRequestArgs::default();
        request.set_last_user_prompt(context.to_string());
        let result = generate_queries_and_fetch_chunks(&pool, llm_client, request).await;

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
        let (pool, hal_9100_config, file_storage) = setup().await;

        reset_db(&pool).await;

        // Create a temporary file.
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        writeln!(temp_file, "Hello, world!").unwrap();

        // Get the path of the temporary file.
        let temp_file_path = temp_file.path();

        // Create a new FileStorage instance.
        let fs = MinioStorage::new(hal_9100_config).await;

        // Upload the file.
        let file_id = fs.upload_file(&temp_file_path).await.unwrap();

        // Retrieve the file.
        let file_id_clone = file_id.clone();
        let file_contents = retrieve_file_contents(&vec![file_id.id], &fs).await;

        // Check that the retrieval was successful and the content is correct.
        assert_eq!(file_contents, vec!["Hello, world!\n"]);

        // Delete the file.
        fs.delete_file(&file_id_clone.id).await.unwrap();
    }

    #[tokio::test]
    async fn test_retrieve_file_contents_pdf() {
        let (pool, _, file_storage) = setup().await;

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
        let file_contents = retrieve_file_contents(&vec![String::from(file_path.id)], &*file_storage).await;

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
