//! Geradores de EPUB e PDF para testes (feature `test-support`).

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use zip::write::{SimpleFileOptions, ZipWriter};

fn cover_png() -> Vec<u8> {
    let image = image::RgbImage::from_pixel(60, 90, image::Rgb([200, 120, 40]));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// EPUB 2 mínimo com três capítulos, com capa, série do Calibre e descrição em HTML.
pub fn write_epub(dir: &Path, name: &str, title: &str, author: &str, subjects: &[&str]) -> PathBuf {
    let subjects: String = subjects
        .iter()
        .map(|s| format!("<dc:subject>{s}</dc:subject>"))
        .collect();
    let opf = format!(
        r#"<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>{title}</dc:title><dc:creator>{author}</dc:creator><dc:language>pt</dc:language>
    <dc:identifier id="id" opf:scheme="ISBN">978-85-359-3258-4</dc:identifier>
    <dc:description>&lt;p&gt;Uma &lt;b&gt;história&lt;/b&gt; antiga.&lt;/p&gt;</dc:description>
    {subjects}
    <meta name="cover" content="capa"/>
    <meta name="calibre:series" content="Romances do Sul"/><meta name="calibre:series_index" content="2"/>
  </metadata>
  <manifest>
    <item id="capa" href="capa.png" media-type="image/png"/>
    <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/>
    <item id="c3" href="c3.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="c1"/><itemref idref="c2"/><itemref idref="c3"/></spine>
</package>"#
    );
    let path = dir.join(name);
    let mut zip = ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options = SimpleFileOptions::default();
    for (file, content) in [
        ("mimetype", b"application/epub+zip".to_vec()),
        (
            "META-INF/container.xml",
            br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#.to_vec(),
        ),
        ("OEBPS/content.opf", opf.into_bytes()),
        ("OEBPS/c1.xhtml", b"<html><body><h1>Um</h1><p>Era uma vez</p></body></html>".to_vec()),
        ("OEBPS/c2.xhtml", b"<html><body><h1>Dois</h1><p>No meio</p></body></html>".to_vec()),
        ("OEBPS/c3.xhtml", b"<html><body><h1>Tr\xc3\xaas</h1><p>Fim</p></body></html>".to_vec()),
        ("OEBPS/capa.png", cover_png()),
    ] {
        zip.start_file(file, options).unwrap();
        zip.write_all(&content).unwrap();
    }
    zip.finish().unwrap();
    path
}

pub fn write_pdf(dir: &Path, name: &str) -> PathBuf {
    use lopdf::{Document, Object, dictionary};

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    let info = doc.add_object(dictionary! {
        "Title" => Object::string_literal("Manual de Liturgia"),
        // UTF-16BE com BOM, como manda a especificação.
        "Author" => Object::String(
            [0xFE, 0xFF].into_iter()
                .chain("Fulano de Tál; Beltrano".encode_utf16().flat_map(u16::to_be_bytes))
                .collect(),
            lopdf::StringFormat::Hexadecimal,
        ),
        // UTF-8 cru, comum em PDFs reais.
        "Keywords" => Object::string_literal("liturgia, oração"),
    });
    doc.trailer.set("Root", catalog);
    doc.trailer.set("Info", info);
    let path = dir.join(name);
    doc.save(&path).unwrap();
    path
}
