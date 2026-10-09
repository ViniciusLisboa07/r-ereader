//! Mostra metadados, spine e sumário de um EPUB.
//!
//! cargo run -p r-ereader-epub --example inspect -- livro.epub

use r_ereader_epub::{Epub, TocEntry};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("uso: inspect <arquivo.epub>")?;
    let mut book = Epub::open(&path)?;

    let meta = book.metadata();
    println!("Título:   {}", meta.title.as_deref().unwrap_or("?"));
    println!("Autores:  {}", meta.creators.join(", "));
    println!("Idioma:   {}", meta.language.as_deref().unwrap_or("?"));
    println!("Editora:  {}", meta.publisher.as_deref().unwrap_or("?"));
    println!("Versão:   EPUB {}", book.package().version);
    println!("Capítulos na spine: {}", book.chapters().len());

    if let Some((item, bytes)) = book.cover()? {
        println!("Capa:     {} ({} KB)", item.path, bytes.len() / 1024);
    }

    println!("\nSumário:");
    for (depth, entry) in TocEntry::flatten(book.toc()) {
        let chapter = entry
            .href
            .as_deref()
            .and_then(|href| book.chapter_index(href))
            .map_or("-".to_owned(), |i| i.to_string());
        println!("{:indent$}[{chapter:>3}] {}", "", entry.title, indent = depth * 2);
    }
    Ok(())
}
