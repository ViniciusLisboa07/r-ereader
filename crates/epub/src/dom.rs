//! Árvore XML mínima e própria, construída sobre o tokenizador do `quick-xml`.
//!
//! Guardamos apenas o que importa para EPUB: nomes locais (sem prefixo de
//! namespace), atributos e texto. Comentários, PIs e DOCTYPE são descartados.

use quick_xml::XmlVersion;
use quick_xml::escape::{resolve_html5_entity, resolve_predefined_entity};
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Element(Element),
    Text(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attr {
    pub prefix: Option<String>,
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Element {
    pub prefix: Option<String>,
    pub name: String,
    pub attrs: Vec<Attr>,
    pub children: Vec<Node>,
}

impl Element {
    /// Valor do atributo pelo nome local, ignorando o prefixo (`epub:type` → `type`).
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.as_str())
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.elements().filter(move |e| e.name == name)
    }

    /// Percorre a subárvore em pré-ordem, incluindo o próprio elemento.
    pub fn descendants(&self) -> Descendants<'_> {
        Descendants { stack: vec![self] }
    }

    pub fn find(&self, name: &str) -> Option<&Element> {
        self.descendants().find(|e| e.name == name)
    }

    /// Todo o texto da subárvore, concatenado.
    pub fn text(&self) -> String {
        let mut out = String::new();
        collect_text(self, &mut out);
        out
    }
}

fn collect_text(element: &Element, out: &mut String) {
    for node in &element.children {
        match node {
            Node::Text(t) => out.push_str(t),
            Node::Element(e) => collect_text(e, out),
        }
    }
}

pub struct Descendants<'a> {
    stack: Vec<&'a Element>,
}

impl<'a> Iterator for Descendants<'a> {
    type Item = &'a Element;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.stack.pop()?;
        self.stack
            .extend(current.elements().collect::<Vec<_>>().into_iter().rev());
        Some(current)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct XmlError {
    pub message: String,
    pub position: u64,
}

impl std::fmt::Display for XmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (byte {})", self.message, self.position)
    }
}

impl std::error::Error for XmlError {}

/// Faz o parse de um documento e devolve o elemento raiz.
///
/// É tolerante no final do arquivo: elementos não fechados são fechados
/// implicitamente, já que EPUBs reais frequentemente vêm truncados.
pub fn parse(source: &str) -> Result<Element, XmlError> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut reader = Reader::from_str(source);
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;

    loop {
        let event = reader.read_event().map_err(|e| XmlError {
            message: e.to_string(),
            position: reader.error_position(),
        })?;

        match event {
            Event::Start(start) => stack.push(element_from(&start)),
            Event::Empty(start) => attach(&mut stack, &mut root, element_from(&start)),
            Event::End(_) => {
                if let Some(done) = stack.pop() {
                    attach(&mut stack, &mut root, done);
                }
            }
            Event::Text(text) => push_text(&mut stack, &text.xml_content(XmlVersion::Implicit1_0)),
            Event::CData(data) => push_text(&mut stack, &data.xml_content(XmlVersion::Implicit1_0)),
            Event::GeneralRef(reference) => {
                let resolved = match reference.resolve_char_ref() {
                    Ok(Some(ch)) => ch.to_string(),
                    _ => resolve_entity(&reference)
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("&{};", &*reference)),
                };
                push_text(&mut stack, &resolved);
            }
            Event::Eof => break,
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) | Event::DocType(_) => {}
        }
    }

    while let Some(open) = stack.pop() {
        attach(&mut stack, &mut root, open);
    }

    root.ok_or_else(|| XmlError {
        message: "documento sem elemento raiz".into(),
        position: 0,
    })
}

fn resolve_entity(name: &str) -> Option<&'static str> {
    resolve_predefined_entity(name).or_else(|| resolve_html5_entity(name))
}

fn split_name(qualified: &str) -> (Option<String>, String) {
    match qualified.split_once(':') {
        Some((prefix, local)) => (Some(prefix.to_owned()), local.to_owned()),
        None => (None, qualified.to_owned()),
    }
}

fn element_from(start: &BytesStart<'_>) -> Element {
    let (prefix, name) = split_name(start.name().0);
    let attrs = start
        .attributes()
        .with_checks(false)
        .filter_map(Result::ok)
        .map(|attr| {
            let (prefix, name) = split_name(attr.key.0);
            let value = attr
                .normalized_value_with(XmlVersion::Implicit1_0, 8, resolve_entity)
                .map(|v| v.into_owned())
                .unwrap_or_else(|_| attr.value.to_string());
            Attr { prefix, name, value }
        })
        .collect();

    Element {
        prefix,
        name,
        attrs,
        children: Vec::new(),
    }
}

fn attach(stack: &mut [Element], root: &mut Option<Element>, element: Element) {
    match stack.last_mut() {
        Some(parent) => parent.children.push(Node::Element(element)),
        None if root.is_none() => *root = Some(element),
        // Conteúdo depois da raiz é ignorado.
        None => {}
    }
}

fn push_text(stack: &mut [Element], text: &str) {
    let Some(parent) = stack.last_mut() else {
        return;
    };
    match parent.children.last_mut() {
        Some(Node::Text(previous)) => previous.push_str(text),
        _ => parent.children.push(Node::Text(text.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_tree_with_local_names_and_attrs() {
        let root = parse(
            r#"<?xml version="1.0"?>
            <package xmlns:dc="x" version="3.0"><metadata><dc:title id="t">Olá &amp; adeus</dc:title></metadata></package>"#,
        )
        .unwrap();

        assert_eq!(root.name, "package");
        assert_eq!(root.attr("version"), Some("3.0"));
        let title = root.find("title").unwrap();
        assert_eq!(title.prefix.as_deref(), Some("dc"));
        assert_eq!(title.attr("id"), Some("t"));
        assert_eq!(title.text(), "Olá & adeus");
    }

    #[test]
    fn resolves_char_and_html_entities() {
        let root = parse("<p>a&#233;&#x41;&nbsp;b&desconhecida;</p>").unwrap();
        assert_eq!(root.text(), "aéA\u{a0}b&desconhecida;");
    }

    #[test]
    fn closes_unterminated_elements_at_eof() {
        let root = parse("<a><b>texto").unwrap();
        assert_eq!(root.find("b").unwrap().text(), "texto");
    }

    #[test]
    fn descendants_are_preorder() {
        let root = parse("<a><b><c/></b><d/></a>").unwrap();
        let names: Vec<_> = root.descendants().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c", "d"]);
    }
}
