//! Importa arquivos para uma biblioteca e lista o resultado.
//!
//! cargo run -p r-ereader-library --example import -- <pasta-da-biblioteca> livro.epub outro.pdf

use std::path::PathBuf;
use std::time::Instant;

use r_ereader_library::{BookQuery, ImportOutcome, Library, SortOrder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().ok_or("uso: import <biblioteca> <arquivos...>")?);
    let mut library = Library::open(&root)?;

    for path in args.map(PathBuf::from) {
        let started = Instant::now();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .take(50)
            .collect::<String>();
        match library.import(&path) {
            Ok(ImportOutcome::Added(id)) => {
                println!("+ [{id:>3}] {name} ({} ms)", started.elapsed().as_millis())
            }
            Ok(ImportOutcome::Duplicate(id)) => println!("= [{id:>3}] {name} (duplicado)"),
            Err(e) => println!("! {name}: {e}"),
        }
    }

    println!();
    for book in library.books(&BookQuery {
        sort: SortOrder::Author,
        ..Default::default()
    })? {
        let series = book
            .series
            .map(|s| format!(" [{} #{}]", s.name, s.index.unwrap_or(0.)))
            .unwrap_or_default();
        let formats: Vec<_> = book.formats.iter().map(|f| f.as_str()).collect();
        println!(
            "{:<28} {}{series} ({}){}",
            book.authors.join(", ").chars().take(28).collect::<String>(),
            book.title,
            formats.join("/"),
            if book.thumbnail.is_some() {
                ""
            } else {
                " — sem capa"
            },
        );
    }
    Ok(())
}
