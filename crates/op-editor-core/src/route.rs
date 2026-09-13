//! Document routes — what the address says, with no browser in sight.
//!
//! The editor's state has three things worth putting in a URL: which document
//! is open, which page of it, and which node is selected. Figma does the same
//! thing (`/file/<key>/<slug>?node-id=…`), and the shape is worth copying for
//! the same reason: a link to a screen is what people actually send each
//! other, and the decorative slug means renaming a file never breaks a link
//! that was already shared.
//!
//! This module is deliberately platform-free. The browser turns routes into
//! `history.pushState` / `replaceState`, the desktop app turns them into its
//! window title and its own back/forward stack, and neither of those decisions
//! belongs here.

use crate::NodeId;
use crate::editor_ui_state::EmbedHost;

/// `/f/<key>` — the editor, on a document the server knows by key.
pub const DOCUMENT_PREFIX: &str = "/f/";
/// `/files` — the file browser (the `figma.com/files` screen).
pub const FILES_PATH: &str = "/files";

/// Which document the route names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteFile {
    /// A server-side document, addressed by its key. The name is a separate
    /// field so a rename does not change the address.
    Key(String),
    /// A document that has never been saved anywhere the server knows about.
    Untitled,
}

/// Everything the address carries about one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentRoute {
    pub file: RouteFile,
    /// Decorative path segment — the file's name, slugged. Never used to
    /// resolve anything: two links that differ only here name one document.
    pub slug: Option<String>,
    pub page: Option<usize>,
    pub node: Option<NodeId>,
    pub embed: Option<EmbedHost>,
}

/// What the address names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteTarget {
    /// The file browser screen.
    Files,
    /// The editor, on a document.
    Document(DocumentRoute),
}

/// The outcome of reading an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutePath {
    Known(RouteTarget),
    /// Not one of ours — an API call, an asset, anything else the daemon
    /// serves. The caller decides what to do (the web host leaves the URL
    /// alone; the static layer answers 404 for genuinely unknown paths).
    NotARoute,
}

impl DocumentRoute {
    /// A route for an unsaved document with nothing selected.
    pub fn untitled() -> Self {
        Self {
            file: RouteFile::Untitled,
            slug: None,
            page: None,
            node: None,
            embed: None,
        }
    }

    /// The route for a file the server knows by `key`.
    pub fn document(key: impl Into<String>, name: Option<&str>) -> Self {
        Self {
            file: RouteFile::Key(key.into()),
            slug: name.map(slugify).filter(|slug| !slug.is_empty()),
            page: None,
            node: None,
            embed: None,
        }
    }

    /// The key this route names, when it names one.
    pub fn key(&self) -> Option<&str> {
        match &self.file {
            RouteFile::Key(key) => Some(key.as_str()),
            RouteFile::Untitled => None,
        }
    }
}

/// Read a request path plus its query string.
///
/// `query` accepts a leading `?` or not, matching `window.location.search`
/// and the shape the tests want to write.
pub fn parse(path: &str, query: &str) -> RoutePath {
    // A caller may hand us `pathname` alone (the browser) or a full path with
    // its query glued on (a pasted link, a test). Split it rather than
    // silently ignoring the parameters.
    let (path, query) = match path.split_once('?') {
        Some((path, inline)) if query.is_empty() => (path, inline),
        _ => (path, query),
    };
    let params = QueryParams::parse(query);
    let embed = EmbedHost::from_query(query);
    let embed = (embed != EmbedHost::None).then_some(embed);

    if path == FILES_PATH || path == "/files/" {
        return RoutePath::Known(RouteTarget::Files);
    }

    if path == "/" || path == "/index.html" {
        return RoutePath::Known(RouteTarget::Document(DocumentRoute {
            file: RouteFile::Untitled,
            slug: None,
            page: params.page(),
            node: params.node(),
            embed,
        }));
    }

    let Some(rest) = path.strip_prefix(DOCUMENT_PREFIX) else {
        return RoutePath::NotARoute;
    };
    // `/f/<key>` or `/f/<key>/<slug>`; anything deeper or empty is not ours.
    let mut segments = rest.split('/');
    let Some(key) = segments.next().filter(|key| !key.is_empty()) else {
        return RoutePath::NotARoute;
    };
    let slug = segments.next().filter(|slug| !slug.is_empty());
    if segments.next().is_some() {
        return RoutePath::NotARoute;
    }
    RoutePath::Known(RouteTarget::Document(DocumentRoute {
        file: RouteFile::Key(key.to_string()),
        slug: slug.map(str::to_string),
        page: params.page(),
        node: params.node(),
        embed,
    }))
}

/// The address for a route, without an origin. Always starts with `/`.
pub fn to_path(target: &RouteTarget) -> String {
    match target {
        RouteTarget::Files => FILES_PATH.to_string(),
        RouteTarget::Document(route) => {
            let mut path = match &route.file {
                RouteFile::Untitled => "/".to_string(),
                RouteFile::Key(key) => match route.slug.as_deref().filter(|slug| !slug.is_empty()) {
                    Some(slug) => format!("{DOCUMENT_PREFIX}{key}/{slug}"),
                    None => format!("{DOCUMENT_PREFIX}{key}"),
                },
            };
            let mut query: Vec<String> = Vec::new();
            if let Some(page) = route.page {
                query.push(format!("page={page}"));
            }
            if let Some(node) = &route.node {
                query.push(format!("node={}", node.as_str()));
            }
            if route.embed == Some(EmbedHost::VsCode) {
                query.push("embed=vscode".to_string());
            }
            if !query.is_empty() {
                path.push('?');
                path.push_str(&query.join("&"));
            }
            path
        }
    }
}

/// A readable path segment for a file name.
///
/// Figma puts the file name in the address for people, not for routers. Latin
/// text is lowercased and hyphenated; Cyrillic is transliterated rather than
/// dropped, because a URL of dashes is worse than a URL of latin letters.
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in name.trim().chars() {
        let mapped: String = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase().to_string()
        } else if let Some(latin) = translit(ch) {
            latin.to_string()
        } else if ch.is_whitespace() || matches!(ch, '-' | '_' | '.' | '/' | '\\' | '·') {
            pending_dash = !out.is_empty();
            continue;
        } else {
            continue;
        };
        if pending_dash && !out.is_empty() {
            out.push('-');
        }
        pending_dash = false;
        out.push_str(&mapped);
    }
    out.truncate(64);
    out.trim_matches('-').to_string()
}

/// Cyrillic (and the few Latin look-alikes Figma sees) → ASCII.
fn translit(ch: char) -> Option<&'static str> {
    Some(match ch {
        'а' => "a", 'б' => "b", 'в' => "v", 'г' => "g", 'д' => "d", 'е' => "e",
        'ё' => "e", 'ж' => "zh", 'з' => "z", 'и' => "i", 'й' => "y", 'к' => "k",
        'л' => "l", 'м' => "m", 'н' => "n", 'о' => "o", 'п' => "p", 'р' => "r",
        'с' => "s", 'т' => "t", 'у' => "u", 'ф' => "f", 'х' => "h", 'ц' => "ts",
        'ч' => "ch", 'ш' => "sh", 'щ' => "sch", 'ъ' => "", 'ы' => "y", 'ь' => "",
        'э' => "e", 'ю' => "yu", 'я' => "ya",
        'А' => "a", 'Б' => "b", 'В' => "v", 'Г' => "g", 'Д' => "d", 'Е' => "e",
        'Ё' => "e", 'Ж' => "zh", 'З' => "z", 'И' => "i", 'Й' => "y", 'К' => "k",
        'Л' => "l", 'М' => "m", 'Н' => "n", 'О' => "o", 'П' => "p", 'Р' => "r",
        'С' => "s", 'Т' => "t", 'У' => "u", 'Ф' => "f", 'Х' => "h", 'Ц' => "ts",
        'Ч' => "ch", 'Ш' => "sh", 'Щ' => "sch", 'Ъ' => "", 'Ы' => "y", 'Ь' => "",
        'Э' => "e", 'Ю' => "yu", 'Я' => "ya",
        _ => return None,
    })
}

/// The handful of query parameters a route understands.
struct QueryParams {
    page: Option<usize>,
    node: Option<NodeId>,
}

impl QueryParams {
    fn parse(query: &str) -> Self {
        let mut page = None;
        let mut node = None;
        let trimmed = query.strip_prefix('?').unwrap_or(query);
        for pair in trimmed.split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            match key {
                "page" => page = page.or_else(|| value.parse::<usize>().ok()),
                "node" => {
                    if !value.is_empty() {
                        node = node.or_else(|| Some(NodeId::new(value.to_string())));
                    }
                }
                _ => {}
            }
        }
        Self { page, node }
    }

    fn page(&self) -> Option<usize> {
        self.page
    }

    fn node(&self) -> Option<NodeId> {
        self.node.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(path: &str, query: &str) -> DocumentRoute {
        match parse(path, query) {
            RoutePath::Known(RouteTarget::Document(route)) => route,
            other => panic!("expected a document route, got {other:?}"),
        }
    }

    #[test]
    fn the_file_browser_has_its_own_path() {
        assert_eq!(
            parse("/files", ""),
            RoutePath::Known(RouteTarget::Files)
        );
        assert_eq!(
            parse("/files/", ""),
            RoutePath::Known(RouteTarget::Files)
        );
    }

    #[test]
    fn the_root_is_the_untitled_editor() {
        let route = document("/", "");
        assert_eq!(route.file, RouteFile::Untitled);
        assert_eq!(route.node, None);
        assert_eq!(to_path(&RouteTarget::Document(route)), "/");
    }

    #[test]
    fn a_key_and_slug_address_one_document() {
        let route = document("/f/01hqx/kommutatory", "");
        assert_eq!(route.key(), Some("01hqx"));
        assert_eq!(route.slug.as_deref(), Some("kommutatory"));
        // The slug is decorative: a link without it names the same file.
        let bare = document("/f/01hqx", "");
        assert_eq!(bare.file, route.file);
    }

    #[test]
    fn a_node_and_a_page_survive_the_round_trip() {
        let route = document("/f/abc?node=n42&page=3", "");
        assert_eq!(route.node, Some(NodeId::new("n42")));
        assert_eq!(route.page, Some(3));
        assert_eq!(
            to_path(&RouteTarget::Document(route.clone())),
            "/f/abc?page=3&node=n42"
        );
        // Parsing our own output yields the same route.
        let again = document("/f/abc", "?page=3&node=n42");
        assert_eq!(again, route);
    }

    #[test]
    fn the_embed_flag_is_preserved() {
        let route = document("/f/abc", "?embed=vscode");
        assert_eq!(route.embed, Some(EmbedHost::VsCode));
        assert_eq!(
            to_path(&RouteTarget::Document(route)),
            "/f/abc?embed=vscode"
        );
    }

    #[test]
    fn other_paths_are_not_routes() {
        for path in ["/api/mcp/document", "/pkg/op_host_web.js", "/f/", "/f/a/b/c", "/files2"] {
            assert_eq!(parse(path, ""), RoutePath::NotARoute, "{path}");
        }
    }

    #[test]
    fn junk_parameters_are_ignored() {
        let route = document("/f/abc", "?node=&page=abc&utm_source=x");
        assert_eq!(route.node, None);
        assert_eq!(route.page, None);
        assert_eq!(to_path(&RouteTarget::Document(route)), "/f/abc");
    }

    #[test]
    fn a_trailing_slash_after_the_key_is_not_a_slug() {
        let route = document("/f/abc/", "");
        assert_eq!(route.slug, None);
    }

    #[test]
    fn slugs_are_readable_for_both_alphabets() {
        assert_eq!(slugify("Коммутаторы 2026"), "kommutatory-2026");
        assert_eq!(slugify("Ops / Servers — v2"), "ops-servers-v2");
        assert_eq!(slugify("  ...  "), "");
        assert_eq!(slugify("Токены доступа"), "tokeny-dostupa");
    }

    #[test]
    fn a_route_built_from_a_name_carries_its_slug() {
        let route = DocumentRoute::document("k1", Some("Список токенов"));
        assert_eq!(route.slug.as_deref(), Some("spisok-tokenov"));
        assert_eq!(
            to_path(&RouteTarget::Document(route)),
            "/f/k1/spisok-tokenov"
        );
    }
}
