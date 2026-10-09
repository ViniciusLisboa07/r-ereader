//! Exporta os destaques de todos os livros para uma pasta de notas do Obsidian.
//!
//! cargo run -p r-ereader-library --example obsidian -- <biblioteca> <pasta-no-vault>

use std::path::PathBuf;

use r_ereader_library::{Library, obsidian};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().ok_or("uso: obsidian <biblioteca> <pasta>")?);
    let folder = PathBuf::from(args.next().ok_or("uso: obsidian <biblioteca> <pasta>")?);
    let library = Library::open(root)?;
    let count = obsidian::export_all(&library, &folder)?;
    println!("{count} nota(s) em {}", folder.display());
    Ok(())
}
