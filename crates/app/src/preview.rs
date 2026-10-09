//! Extração provisória de blocos de texto de um capítulo XHTML.
//!
//! Serve só para enxergar o conteúdo enquanto o motor de layout próprio
//! (crates `doc`, `style` e `layout`) não existe. Ignora CSS por completo.

use r_ereader_epub::dom::{Element, Node};
use r_ereader_epub::path;

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        text: String,
    },
    Paragraph(String),
    Quote(String),
    ListItem(String),
    Preformatted(String),
    /// Caminho completo da imagem dentro do container.
    Image(String),
}

impl Block {
    /// Texto do bloco; imagens contam como texto vazio.
    pub fn text(&self) -> &str {
        match self {
            Block::Heading { text, .. }
            | Block::Paragraph(text)
            | Block::Quote(text)
            | Block::ListItem(text)
            | Block::Preformatted(text) => text,
            Block::Image(_) => "",
        }
    }
}

const LEAF_BLOCKS: &[&str] = &[
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "dt",
    "dd",
    "pre",
    "figcaption",
    "td",
    "th",
];
const CONTAINER_BLOCKS: &[&str] = &[
    "div",
    "section",
    "article",
    "aside",
    "blockquote",
    "ul",
    "ol",
    "dl",
    "figure",
    "table",
    "tbody",
    "thead",
    "tr",
    "nav",
    "header",
    "footer",
    "main",
    "body",
    "hr",
];
const SKIPPED: &[&str] = &["head", "script", "style", "title"];

pub fn extract(document: &Element, chapter_path: &str) -> Vec<Block> {
    let mut walker = Walker {
        base_dir: path::parent(chapter_path),
        blocks: Vec::new(),
        loose_text: String::new(),
    };
    let body = document.find("body").unwrap_or(document);
    walker.walk(body, false);
    walker.flush_loose(false);
    walker.blocks
}

struct Walker<'a> {
    base_dir: &'a str,
    blocks: Vec<Block>,
    /// Texto solto direto em containers (ex.: `<div>texto</div>`).
    loose_text: String,
}

impl Walker<'_> {
    fn walk(&mut self, element: &Element, in_quote: bool) {
        for node in &element.children {
            match node {
                Node::Text(text) => self.loose_text.push_str(text),
                Node::Element(child) => self.visit(child, in_quote),
            }
        }
    }

    fn visit(&mut self, element: &Element, in_quote: bool) {
        let name = element.name.as_str();
        if SKIPPED.contains(&name) {
            return;
        }
        if let Some(src) = image_source(element) {
            self.flush_loose(in_quote);
            self.blocks.push(Block::Image(path::resolve(self.base_dir, src)));
            return;
        }
        if LEAF_BLOCKS.contains(&name) && !has_block_child(element) {
            self.flush_loose(in_quote);
            self.leaf(element, in_quote);
            return;
        }
        if LEAF_BLOCKS.contains(&name) || CONTAINER_BLOCKS.contains(&name) {
            self.flush_loose(in_quote);
            self.walk(element, in_quote || name == "blockquote");
            self.flush_loose(in_quote);
            return;
        }
        // Elementos inline fora de um bloco: o texto entra no parágrafo solto.
        if element.descendants().skip(1).any(|e| image_source(e).is_some()) {
            self.walk(element, in_quote);
        } else {
            self.loose_text.push_str(&element.text());
        }
    }

    fn leaf(&mut self, element: &Element, in_quote: bool) {
        let name = element.name.as_str();
        if name == "pre" {
            let text = element.text();
            if !text.trim().is_empty() {
                self.blocks.push(Block::Preformatted(text.trim_end().to_owned()));
            }
            return;
        }

        let text = collapse(&element.text());
        if !text.is_empty() {
            self.blocks.push(match name {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Block::Heading {
                    level: name[1..].parse().unwrap_or(6),
                    text,
                },
                "li" => Block::ListItem(text),
                _ if in_quote => Block::Quote(text),
                _ => Block::Paragraph(text),
            });
        }
        for image in element.descendants().filter_map(image_source) {
            self.blocks
                .push(Block::Image(path::resolve(self.base_dir, image)));
        }
    }

    fn flush_loose(&mut self, in_quote: bool) {
        let text = collapse(&std::mem::take(&mut self.loose_text));
        if !text.is_empty() {
            self.blocks.push(if in_quote {
                Block::Quote(text)
            } else {
                Block::Paragraph(text)
            });
        }
    }
}

fn image_source(element: &Element) -> Option<&str> {
    match element.name.as_str() {
        "img" => element.attr("src"),
        // <svg><image xlink:href="..."/></svg>, comum em páginas de capa.
        "image" => element.attr("href"),
        _ => None,
    }
}

fn has_block_child(element: &Element) -> bool {
    element.elements().any(|child| {
        let name = child.name.as_str();
        LEAF_BLOCKS.contains(&name) || CONTAINER_BLOCKS.contains(&name)
    })
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use r_ereader_epub::dom;

    fn blocks(xhtml: &str) -> Vec<Block> {
        extract(&dom::parse(xhtml).unwrap(), "OEBPS/Text/c1.xhtml")
    }

    #[test]
    fn extracts_headings_paragraphs_and_quotes() {
        let result = blocks(
            "<html><head><title>x</title></head><body>
               <h2 class='t'>Capítulo <em>I</em></h2>
               <p>Era   uma <b>vez</b>.</p>
               <blockquote><p>Citação</p></blockquote>
             </body></html>",
        );
        assert_eq!(
            result,
            [
                Block::Heading {
                    level: 2,
                    text: "Capítulo I".into()
                },
                Block::Paragraph("Era uma vez.".into()),
                Block::Quote("Citação".into()),
            ]
        );
    }

    #[test]
    fn resolves_images_including_svg_covers() {
        let result = blocks(
            "<html><body>
               <svg><image xlink:href='../Images/capa.jpg'/></svg>
               <p><img src='../Images/fig.png'/> Legenda</p>
             </body></html>",
        );
        assert_eq!(
            result,
            [
                Block::Image("OEBPS/Images/capa.jpg".into()),
                Block::Paragraph("Legenda".into()),
                Block::Image("OEBPS/Images/fig.png".into()),
            ]
        );
    }

    #[test]
    fn keeps_loose_text_in_divs() {
        let result = blocks("<html><body><div>solto <i>aqui</i></div><div><p>dentro</p></div></body></html>");
        assert_eq!(
            result,
            [
                Block::Paragraph("solto aqui".into()),
                Block::Paragraph("dentro".into())
            ]
        );
    }
}
