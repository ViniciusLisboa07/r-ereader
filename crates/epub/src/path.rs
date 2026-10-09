//! Resolução de caminhos dentro do container ZIP.
//!
//! Hrefs no EPUB são URLs relativas ao documento que as contém, podem ter
//! percent-encoding e fragmento (`cap1.xhtml#sec2`).

/// Diretório de um caminho do container (`OEBPS/content.opf` → `OEBPS`).
pub fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Separa o fragmento: `a.xhtml#b` → (`a.xhtml`, Some(`b`)).
pub fn split_fragment(href: &str) -> (&str, Option<&str>) {
    match href.split_once('#') {
        Some((path, fragment)) => (path, Some(fragment)),
        None => (href, None),
    }
}

/// Resolve `href` relativo ao diretório `base_dir`, normalizando `.` e `..`.
/// O fragmento, se houver, é preservado.
pub fn resolve(base_dir: &str, href: &str) -> String {
    let (path, fragment) = split_fragment(href);
    let path = percent_decode(path);

    let mut segments: Vec<&str> = Vec::new();
    if !path.starts_with('/') {
        segments.extend(base_dir.split('/').filter(|s| !s.is_empty()));
    }
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }

    let mut resolved = segments.join("/");
    if let Some(fragment) = fragment {
        resolved.push('#');
        resolved.push_str(fragment);
    }
    resolved
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = bytes
                .get(i + 1..i + 3)
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_of_root_file_is_empty() {
        assert_eq!(parent("content.opf"), "");
        assert_eq!(parent("OEBPS/text/cap1.xhtml"), "OEBPS/text");
    }

    #[test]
    fn resolves_relative_paths() {
        assert_eq!(resolve("OEBPS", "text/cap1.xhtml"), "OEBPS/text/cap1.xhtml");
        assert_eq!(resolve("OEBPS/text", "../img/a.png"), "OEBPS/img/a.png");
        assert_eq!(resolve("OEBPS", "./cap.xhtml#s1"), "OEBPS/cap.xhtml#s1");
        assert_eq!(resolve("", "cap.xhtml"), "cap.xhtml");
        assert_eq!(resolve("OEBPS/text", "/raiz.xhtml"), "raiz.xhtml");
    }

    #[test]
    fn decodes_percent_encoding() {
        assert_eq!(
            resolve("OEBPS", "cap%C3%ADtulo%201.xhtml"),
            "OEBPS/capítulo 1.xhtml"
        );
        assert_eq!(resolve("", "100%.xhtml"), "100%.xhtml");
    }
}
