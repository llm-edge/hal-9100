use lopdf::{Document, Object};
use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::path::Path;
use std::io::{Error as IoError, ErrorKind};

pub fn pdf_to_text(path: &Path) -> Result<String, Box<dyn StdError>> {
    let doc = Document::load(path)?;

    let mut text: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let pages: Vec<Result<(u32, Vec<String>), Box<dyn StdError>>> = doc
        .get_pages()
        .iter()
        .map(|(page_num, _)| {
            let page_text = doc.extract_text(&[*page_num]).map_err(|e| {
                IoError::new(
                    ErrorKind::Other,
                    format!("Failed to extract text from page {}: {:?}", page_num, e),
                )
            })?;
            Ok((
                *page_num, // Dereference page_num here
                page_text.split('\n')
                    .map(|s| s.trim_end().to_string())
                    .collect::<Vec<String>>(),
            ))
        })
        .collect();

    for page in pages {
        match page {
            Ok((page_num, lines)) => {
                text.insert(page_num, lines);
            }
            Err(e) => {
                eprintln!("Error extracting text from page: {}", e);
            }
        }
    }

    // return joined text
    let joined_text = text
        .iter()
        .map(|(_, lines)| lines.join("\n"))
        .collect::<Vec<String>>()
        .join("\n");
    Ok(joined_text)
}

pub fn pdf_mem_to_text(data: &[u8]) -> Result<String, Box<dyn StdError>> {
    let doc = Document::load_mem(data)?;
    let mut text: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let pages: Vec<Result<(u32, Vec<String>), Box<dyn StdError>>> = doc
        .get_pages()
        .iter()
        .map(|(page_num, _)| {
            let page_text = doc.extract_text(&[*page_num]).map_err(|e| {
                IoError::new(
                    ErrorKind::Other,
                    format!("Failed to extract text from page {}: {:?}", page_num, e),
                )
            })?;
            Ok((
                *page_num, // Dereference page_num here
                page_text.split('\n')
                    .map(|s| s.trim_end().to_string())
                    .collect::<Vec<String>>(),
            ))
        })
        .collect();

    for page in pages {
        match page {
            Ok((page_num, lines)) => {
                text.insert(page_num, lines);
            }
            Err(e) => {
                eprintln!("Error extracting text from page: {}", e);
            }
        }
    }

    // return joined text
    let joined_text = text
        .iter()
        .map(|(_, lines)| lines.join("\n"))
        .collect::<Vec<String>>()
        .join("\n");
    Ok(joined_text)
}

