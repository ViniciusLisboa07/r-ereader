use std::io::{Cursor, Write};

use r_ereader_epub::{Epub, Error, TocEntry};
use zip::write::{SimpleFileOptions, ZipWriter};

fn build_epub(files: &[(&str, &str)]) -> Cursor<Vec<u8>> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default();
    for (name, content) in files {
        zip.start_file(*name, options).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    Cursor::new(zip.finish().unwrap().into_inner())
}

const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;

const OPF: &str = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Livro de Teste</dc:title><dc:creator>Fulano</dc:creator><dc:identifier id="id">x</dc:identifier>
  </metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="c1" href="Text/c1.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="Text/c2.xhtml" media-type="application/xhtml+xml"/>
    <item id="fantasma" href="nao-existe.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine toc="ncx"><itemref idref="c1"/><itemref idref="c2"/><itemref idref="sem-item"/></spine>
</package>"#;

const NCX: &str = r#"<ncx><navMap>
  <navPoint><navLabel><text>Primeiro</text></navLabel><content src="Text/c1.xhtml"/></navPoint>
  <navPoint><navLabel><text>Segundo</text></navLabel><content src="Text/c2.xhtml#meio"/></navPoint>
</navMap></ncx>"#;

const C1: &str = "<html><body><p>Era uma vez</p></body></html>";
const C2: &str = "<html><body><p id=\"meio\">Fim</p></body></html>";

fn sample() -> Epub<Cursor<Vec<u8>>> {
    Epub::from_reader(build_epub(&[
        ("mimetype", "application/epub+zip"),
        ("META-INF/container.xml", CONTAINER),
        ("OEBPS/content.opf", OPF),
        ("OEBPS/toc.ncx", NCX),
        ("OEBPS/Text/c1.xhtml", C1),
        ("OEBPS/Text/c2.xhtml", C2),
    ]))
    .unwrap()
}

#[test]
fn opens_epub2_with_ncx() {
    let book = sample();
    assert_eq!(book.opf_path(), "OEBPS/content.opf");
    assert_eq!(book.metadata().title.as_deref(), Some("Livro de Teste"));

    let paths: Vec<_> = book.chapters().iter().map(|c| c.path.as_str()).collect();
    assert_eq!(paths, ["OEBPS/Text/c1.xhtml", "OEBPS/Text/c2.xhtml"]);

    let titles: Vec<_> = TocEntry::flatten(book.toc())
        .iter()
        .map(|(_, e)| e.title.clone())
        .collect();
    assert_eq!(titles, ["Primeiro", "Segundo"]);
}

#[test]
fn maps_toc_href_to_chapter() {
    let book = sample();
    let second = book.toc()[1].href.as_deref().unwrap();
    assert_eq!(book.chapter_index(second), Some(1));
}

#[test]
fn reads_chapter_documents() {
    let mut book = sample();
    let doc = book.chapter_document(1).unwrap();
    assert_eq!(doc.find("p").unwrap().attr("id"), Some("meio"));
    assert_eq!(doc.text(), "Fim");
}

#[test]
fn reports_missing_files() {
    let mut book = sample();
    assert!(matches!(
        book.read_bytes("OEBPS/nao-existe.xhtml"),
        Err(Error::MissingFile(_))
    ));
}

#[test]
fn fails_without_container() {
    let result = Epub::from_reader(build_epub(&[("mimetype", "application/epub+zip")]));
    assert!(matches!(result, Err(Error::MissingFile(_))));
}
