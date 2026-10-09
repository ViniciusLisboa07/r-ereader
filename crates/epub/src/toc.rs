//! Sumário: Navigation Document (EPUB 3) e NCX (EPUB 2).

use crate::dom::Element;
use crate::package::collapse_whitespace;
use crate::path;

#[derive(Debug, Clone, PartialEq)]
pub struct TocEntry {
    pub title: String,
    /// Caminho completo no container, com fragmento opcional.
    /// `None` para entradas que só agrupam filhos (ex.: `<span>` no nav).
    pub href: Option<String>,
    pub children: Vec<TocEntry>,
}

impl TocEntry {
    /// Itera a árvore em pré-ordem junto com a profundidade de cada entrada.
    pub fn flatten(entries: &[TocEntry]) -> Vec<(usize, &TocEntry)> {
        let mut out = Vec::new();
        fn walk<'a>(entries: &'a [TocEntry], depth: usize, out: &mut Vec<(usize, &'a TocEntry)>) {
            for entry in entries {
                out.push((depth, entry));
                walk(&entry.children, depth + 1, out);
            }
        }
        walk(entries, 0, &mut out);
        out
    }
}

/// Lê o `<nav epub:type="toc">` de um Navigation Document.
pub fn from_nav(root: &Element, nav_path: &str) -> Vec<TocEntry> {
    let base_dir = path::parent(nav_path);
    let navs: Vec<&Element> = root.descendants().filter(|e| e.name == "nav").collect();
    let toc_nav = navs
        .iter()
        .find(|nav| {
            nav.attr("type")
                .is_some_and(|t| t.split_whitespace().any(|t| t == "toc"))
        })
        .or(navs.first());

    toc_nav
        .and_then(|nav| nav.find("ol"))
        .map(|ol| nav_list(ol, base_dir))
        .unwrap_or_default()
}

fn nav_list(ol: &Element, base_dir: &str) -> Vec<TocEntry> {
    ol.children_named("li")
        .filter_map(|li| {
            let label = li.elements().find(|e| e.name == "a" || e.name == "span");
            let children = li
                .child("ol")
                .map(|ol| nav_list(ol, base_dir))
                .unwrap_or_default();
            let title = label.map(|l| collapse_whitespace(&l.text())).unwrap_or_default();
            if title.is_empty() && children.is_empty() {
                return None;
            }
            Some(TocEntry {
                title,
                href: label
                    .and_then(|l| l.attr("href"))
                    .map(|href| path::resolve(base_dir, href)),
                children,
            })
        })
        .collect()
}

/// Lê o `<navMap>` de um NCX.
pub fn from_ncx(root: &Element, ncx_path: &str) -> Vec<TocEntry> {
    let base_dir = path::parent(ncx_path);
    root.find("navMap")
        .map(|map| ncx_points(map, base_dir))
        .unwrap_or_default()
}

fn ncx_points(parent: &Element, base_dir: &str) -> Vec<TocEntry> {
    parent
        .children_named("navPoint")
        .map(|point| TocEntry {
            title: point
                .child("navLabel")
                .and_then(|label| label.child("text"))
                .map(|text| collapse_whitespace(&text.text()))
                .unwrap_or_default(),
            href: point
                .child("content")
                .and_then(|c| c.attr("src"))
                .map(|src| path::resolve(base_dir, src)),
            children: ncx_points(point, base_dir),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom;

    #[test]
    fn parses_nested_nav_toc() {
        let doc = dom::parse(
            r##"<html xmlns:epub="http://www.idpf.org/2007/ops"><body>
              <nav epub:type="landmarks"><ol><li><a href="x.xhtml">Ignorar</a></li></ol></nav>
              <nav epub:type="toc"><h1>Sumário</h1><ol>
                <li><a href="texto/c1.xhtml">Capítulo
                    um</a></li>
                <li><span>Parte II</span><ol>
                  <li><a href="texto/c2.xhtml#s1">Seção</a></li>
                </ol></li>
              </ol></nav>
            </body></html>"##,
        )
        .unwrap();

        let toc = from_nav(&doc, "OEBPS/nav.xhtml");
        assert_eq!(toc.len(), 2);
        assert_eq!(toc[0].title, "Capítulo um");
        assert_eq!(toc[0].href.as_deref(), Some("OEBPS/texto/c1.xhtml"));
        assert_eq!(toc[1].title, "Parte II");
        assert_eq!(toc[1].href, None);
        assert_eq!(
            toc[1].children[0].href.as_deref(),
            Some("OEBPS/texto/c2.xhtml#s1")
        );
    }

    #[test]
    fn parses_nested_ncx() {
        let doc = dom::parse(
            r#"<ncx><navMap>
              <navPoint id="p1"><navLabel><text>Um</text></navLabel><content src="c1.xhtml"/>
                <navPoint id="p2"><navLabel><text>Um.1</text></navLabel><content src="c1.xhtml#a"/></navPoint>
              </navPoint>
              <navPoint id="p3"><navLabel><text>Dois</text></navLabel><content src="../c2.xhtml"/></navPoint>
            </navMap></ncx>"#,
        )
        .unwrap();

        let toc = from_ncx(&doc, "OEBPS/toc/toc.ncx");
        let flat: Vec<_> = TocEntry::flatten(&toc)
            .into_iter()
            .map(|(depth, e)| (depth, e.title.as_str(), e.href.as_deref().unwrap()))
            .collect();
        assert_eq!(
            flat,
            [
                (0, "Um", "OEBPS/toc/c1.xhtml"),
                (1, "Um.1", "OEBPS/toc/c1.xhtml#a"),
                (0, "Dois", "OEBPS/c2.xhtml"),
            ]
        );
    }
}
