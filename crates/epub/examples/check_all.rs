//! Faz o parse de todos os capítulos de cada EPUB recebido e reporta falhas.
//!
//! cargo run -p r-ereader-epub --example check_all -- ~/Livros/*.epub

use r_ereader_epub::Epub;

fn main() {
    let mut failures = 0;
    for path in std::env::args().skip(1) {
        let mut book = match Epub::open(&path) {
            Ok(book) => book,
            Err(e) => {
                failures += 1;
                println!("ERRO  {path}: {e}");
                continue;
            }
        };
        let total = book.chapters().len();
        let broken: Vec<String> = (0..total)
            .filter_map(|i| book.chapter_document(i).err().map(|e| e.to_string()))
            .collect();
        failures += broken.len();
        let title = book.metadata().title.clone().unwrap_or(path);
        println!(
            "{}  {title}: {}/{total} capítulos",
            if broken.is_empty() { "ok  " } else { "FALHA" },
            total - broken.len()
        );
        for error in broken {
            println!("      {error}");
        }
    }
    std::process::exit(if failures == 0 { 0 } else { 1 });
}
