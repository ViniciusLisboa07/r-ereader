//! Package Document (OPF): metadados, manifest e spine.

use crate::dom::{self, Element};
use crate::error::{Error, Result};
use crate::path;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Metadata {
    pub title: Option<String>,
    pub creators: Vec<String>,
    pub language: Option<String>,
    pub identifier: Option<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub date: Option<String>,
    pub subjects: Vec<String>,
    /// Todos os `<dc:identifier>`, na ordem do documento.
    pub identifiers: Vec<Identifier>,
    pub series: Option<Series>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Identifier {
    /// Esquema declarado (`opf:scheme="ISBN"`) ou inferido do prefixo `urn:<esquema>:`.
    pub scheme: Option<String>,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub name: String,
    pub index: Option<f64>,
}

impl Metadata {
    /// ISBN do livro, só com dígitos (e `X` final, no ISBN-10).
    pub fn isbn(&self) -> Option<String> {
        self.identifiers.iter().find_map(|id| {
            let declared = id
                .scheme
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case("isbn"));
            let digits: String = id
                .value
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x')
                .map(|c| c.to_ascii_uppercase())
                .collect();
            let looks_like_isbn = (digits.len() == 13
                && (digits.starts_with("978") || digits.starts_with("979")))
                || (digits.len() == 10 && declared);
            (looks_like_isbn && (declared || !id.value.contains("uuid"))).then_some(digits)
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManifestItem {
    pub id: String,
    /// Caminho completo dentro do container (já resolvido a partir do OPF).
    pub path: String,
    pub media_type: String,
    pub properties: Vec<String>,
}

impl ManifestItem {
    pub fn has_property(&self, property: &str) -> bool {
        self.properties.iter().any(|p| p == property)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpineItem {
    pub idref: String,
    pub linear: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    pub version: String,
    pub metadata: Metadata,
    pub manifest: Vec<ManifestItem>,
    pub spine: Vec<SpineItem>,
    /// Id do NCX declarado em `<spine toc="...">` (EPUB 2).
    pub spine_toc: Option<String>,
    /// Id da capa declarado em `<meta name="cover">` (EPUB 2).
    cover_meta: Option<String>,
}

impl Package {
    pub fn parse(source: &str, opf_path: &str) -> Result<Self> {
        let root = dom::parse(source).map_err(|source| Error::Xml {
            path: opf_path.to_owned(),
            source,
        })?;
        if root.name != "package" {
            return Err(Error::Invalid(format!(
                "raiz do OPF deveria ser <package>, encontrado <{}>",
                root.name
            )));
        }

        let base_dir = path::parent(opf_path);
        let empty = Element::default();
        let metadata_el = root.child("metadata").unwrap_or(&empty);
        let manifest_el = root
            .child("manifest")
            .ok_or_else(|| Error::Invalid("OPF sem <manifest>".into()))?;
        let spine_el = root
            .child("spine")
            .ok_or_else(|| Error::Invalid("OPF sem <spine>".into()))?;

        let manifest = manifest_el
            .children_named("item")
            .filter_map(|item| {
                Some(ManifestItem {
                    id: item.attr("id")?.to_owned(),
                    path: path::resolve(base_dir, item.attr("href")?),
                    media_type: item.attr("media-type").unwrap_or_default().to_owned(),
                    properties: item
                        .attr("properties")
                        .unwrap_or_default()
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect(),
                })
            })
            .collect();

        let spine = spine_el
            .children_named("itemref")
            .filter_map(|itemref| {
                Some(SpineItem {
                    idref: itemref.attr("idref")?.to_owned(),
                    linear: itemref.attr("linear") != Some("no"),
                })
            })
            .collect();

        let cover_meta = metadata_el
            .children_named("meta")
            .find(|m| m.attr("name") == Some("cover"))
            .and_then(|m| m.attr("content"))
            .map(str::to_owned);

        Ok(Package {
            version: root.attr("version").unwrap_or("2.0").to_owned(),
            metadata: parse_metadata(metadata_el, root.attr("unique-identifier")),
            manifest,
            spine,
            spine_toc: spine_el.attr("toc").map(str::to_owned),
            cover_meta,
        })
    }

    pub fn item(&self, id: &str) -> Option<&ManifestItem> {
        self.manifest.iter().find(|item| item.id == id)
    }

    pub fn item_by_path(&self, path: &str) -> Option<&ManifestItem> {
        self.manifest.iter().find(|item| item.path == path)
    }

    /// Navigation Document do EPUB 3.
    pub fn nav_item(&self) -> Option<&ManifestItem> {
        self.manifest.iter().find(|item| item.has_property("nav"))
    }

    /// NCX do EPUB 2 (também presente em muitos EPUB 3 por compatibilidade).
    pub fn ncx_item(&self) -> Option<&ManifestItem> {
        self.spine_toc
            .as_deref()
            .and_then(|id| self.item(id))
            .or_else(|| {
                self.manifest
                    .iter()
                    .find(|item| item.media_type == "application/x-dtbncx+xml")
            })
    }

    pub fn cover_item(&self) -> Option<&ManifestItem> {
        self.manifest
            .iter()
            .find(|item| item.has_property("cover-image"))
            .or_else(|| self.cover_meta.as_deref().and_then(|id| self.item(id)))
            .filter(|item| is_image(item))
            .or_else(|| {
                // Sem declaração: uma imagem chamada "cover" (id ou nome do arquivo).
                self.manifest.iter().filter(|item| is_image(item)).find(|item| {
                    let stem = item.path.rsplit('/').next().unwrap_or_default();
                    let stem = stem.split('.').next().unwrap_or_default();
                    item.id.eq_ignore_ascii_case("cover") || stem.eq_ignore_ascii_case("cover")
                })
            })
    }
}

fn is_image(item: &ManifestItem) -> bool {
    item.media_type.starts_with("image/")
}

fn parse_metadata(metadata: &Element, unique_identifier: Option<&str>) -> Metadata {
    let texts = |name: &str| -> Vec<String> {
        metadata
            .children_named(name)
            .map(|e| collapse_whitespace(&e.text()))
            .filter(|t| !t.is_empty())
            .collect()
    };
    let first = |name: &str| texts(name).into_iter().next();

    let identifier = unique_identifier
        .and_then(|id| {
            metadata
                .children_named("identifier")
                .find(|e| e.attr("id") == Some(id))
        })
        .map(|e| collapse_whitespace(&e.text()))
        .or_else(|| first("identifier"));

    Metadata {
        title: first("title"),
        creators: texts("creator"),
        language: first("language"),
        identifier,
        publisher: first("publisher"),
        description: metadata
            .child("description")
            .map(|e| e.text().trim().to_owned())
            .filter(|t| !t.is_empty()),
        date: first("date"),
        subjects: texts("subject"),
        identifiers: metadata
            .children_named("identifier")
            .filter_map(parse_identifier)
            .collect(),
        series: calibre_series(metadata).or_else(|| epub3_series(metadata)),
    }
}

fn parse_identifier(element: &Element) -> Option<Identifier> {
    let raw = collapse_whitespace(&element.text());
    if raw.is_empty() {
        return None;
    }
    if let Some(scheme) = element.attr("scheme") {
        return Some(Identifier {
            scheme: Some(scheme.to_owned()),
            value: raw,
        });
    }
    // `urn:isbn:978...` → esquema "isbn"; `urn:uuid:...` → "uuid".
    let inferred = raw
        .strip_prefix("urn:")
        .and_then(|rest| rest.split_once(':'))
        .map(|(scheme, value)| (scheme.to_owned(), value.to_owned()));
    Some(match inferred {
        Some((scheme, value)) => Identifier {
            scheme: Some(scheme),
            value,
        },
        None => Identifier {
            scheme: None,
            value: raw,
        },
    })
}

/// `<meta name="calibre:series" content="..."/>`, usado por EPUBs gerados pelo Calibre.
fn calibre_series(metadata: &Element) -> Option<Series> {
    let meta = |name: &str| {
        metadata
            .children_named("meta")
            .find(|m| m.attr("name") == Some(name))
            .and_then(|m| m.attr("content"))
            .map(str::trim)
            .filter(|v| !v.is_empty())
    };
    Some(Series {
        name: meta("calibre:series")?.to_owned(),
        index: meta("calibre:series_index").and_then(|i| i.parse().ok()),
    })
}

/// EPUB 3: `<meta property="belongs-to-collection" id="c">Nome</meta>` refinado por
/// `collection-type` = `series` e `group-position`.
fn epub3_series(metadata: &Element) -> Option<Series> {
    let refinement = |id: &str, property: &str| {
        let target = format!("#{id}");
        metadata
            .children_named("meta")
            .find(|m| m.attr("refines") == Some(target.as_str()) && m.attr("property") == Some(property))
            .map(|m| collapse_whitespace(&m.text()))
    };

    metadata
        .children_named("meta")
        .filter(|m| m.attr("property") == Some("belongs-to-collection"))
        .find_map(|collection| {
            let name = collapse_whitespace(&collection.text());
            let id = collection.attr("id");
            let kind = id.and_then(|id| refinement(id, "collection-type"));
            if name.is_empty() || kind.as_deref().is_some_and(|k| k != "series") {
                return None;
            }
            Some(Series {
                name,
                index: id
                    .and_then(|id| refinement(id, "group-position"))
                    .and_then(|i| i.parse().ok()),
            })
        })
}

pub(crate) fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPF: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="isbn">978-0</dc:identifier>
    <dc:identifier id="uid">urn:uuid:123</dc:identifier>
    <dc:title>  Dom
        Casmurro </dc:title>
    <dc:creator>Machado de Assis</dc:creator>
    <dc:language>pt-BR</dc:language>
    <meta name="cover" content="capa"/>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="capa" href="img/capa.jpg" media-type="image/jpeg"/>
    <item id="c1" href="texto/cap%201.xhtml" media-type="application/xhtml+xml"/>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
  </manifest>
  <spine toc="ncx">
    <itemref idref="c1"/>
    <itemref idref="nav" linear="no"/>
  </spine>
</package>"#;

    #[test]
    fn parses_metadata() {
        let package = Package::parse(OPF, "OEBPS/content.opf").unwrap();
        assert_eq!(package.version, "3.0");
        assert_eq!(package.metadata.title.as_deref(), Some("Dom Casmurro"));
        assert_eq!(package.metadata.creators, ["Machado de Assis"]);
        assert_eq!(package.metadata.language.as_deref(), Some("pt-BR"));
        assert_eq!(package.metadata.identifier.as_deref(), Some("urn:uuid:123"));
    }

    #[test]
    fn resolves_manifest_paths_and_spine() {
        let package = Package::parse(OPF, "OEBPS/content.opf").unwrap();
        assert_eq!(package.item("c1").unwrap().path, "OEBPS/texto/cap 1.xhtml");
        assert_eq!(package.spine.len(), 2);
        assert!(package.spine[0].linear);
        assert!(!package.spine[1].linear);
    }

    #[test]
    fn finds_nav_ncx_and_cover() {
        let package = Package::parse(OPF, "OEBPS/content.opf").unwrap();
        assert_eq!(package.nav_item().unwrap().id, "nav");
        assert_eq!(package.ncx_item().unwrap().id, "ncx");
        assert_eq!(package.cover_item().unwrap().path, "OEBPS/img/capa.jpg");
    }

    #[test]
    fn parses_identifiers_and_isbn() {
        let package = Package::parse(
            r#"<package><metadata>
                 <dc:identifier opf:scheme="uuid">abc</dc:identifier>
                 <dc:identifier>urn:isbn:978-85-359-3258-4</dc:identifier>
               </metadata><manifest/><spine/></package>"#,
            "c.opf",
        )
        .unwrap();
        let ids = &package.metadata.identifiers;
        assert_eq!(
            ids[0],
            Identifier {
                scheme: Some("uuid".into()),
                value: "abc".into()
            }
        );
        assert_eq!(ids[1].scheme.as_deref(), Some("isbn"));
        assert_eq!(package.metadata.isbn().as_deref(), Some("9788535932584"));
    }

    #[test]
    fn parses_calibre_series() {
        let package = Package::parse(
            r#"<package><metadata>
                 <meta name="calibre:series" content="Crônicas de Nárnia"/>
                 <meta name="calibre:series_index" content="2.0"/>
               </metadata><manifest/><spine/></package>"#,
            "c.opf",
        )
        .unwrap();
        assert_eq!(
            package.metadata.series,
            Some(Series {
                name: "Crônicas de Nárnia".into(),
                index: Some(2.0)
            })
        );
    }

    #[test]
    fn parses_epub3_series() {
        let package = Package::parse(
            r##"<package><metadata>
                 <meta property="belongs-to-collection" id="c1">Trilogia</meta>
                 <meta refines="#c1" property="collection-type">series</meta>
                 <meta refines="#c1" property="group-position">3</meta>
               </metadata><manifest/><spine/></package>"##,
            "c.opf",
        )
        .unwrap();
        assert_eq!(
            package.metadata.series,
            Some(Series {
                name: "Trilogia".into(),
                index: Some(3.0)
            })
        );
    }

    #[test]
    fn falls_back_to_image_named_cover() {
        let package = Package::parse(
            r#"<package><metadata/><manifest>
                 <item id="cov" href="00-Cover.xhtml" media-type="application/xhtml+xml"/>
                 <item id="p1" href="images/page1.jpg" media-type="image/jpeg"/>
                 <item id="img9" href="images/cover.jpg" media-type="image/jpeg"/>
               </manifest><spine/></package>"#,
            "OEBPS/c.opf",
        )
        .unwrap();
        assert_eq!(package.cover_item().unwrap().path, "OEBPS/images/cover.jpg");
    }

    #[test]
    fn rejects_opf_without_spine() {
        let err = Package::parse("<package><manifest/></package>", "c.opf").unwrap_err();
        assert!(matches!(err, Error::Invalid(_)));
    }
}
