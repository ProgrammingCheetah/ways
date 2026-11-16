//! The page's files, embedded in the binary at compile time.
//!
//! They are what Vite builds from `zaf/web` into `zaf/web/dist`, which is committed, so
//! `cargo build` and `cargo install` need no Node. The build gives each file a fixed name
//! (`zaf/web/vite.config.js`), so the table below does not change with a rebuild; the test
//! `every_built_file_is_served` fails when a rebuild makes a file the table lacks.

/// One embedded file and the type it is served as.
#[derive(Debug, Clone, Copy)]
pub struct Asset {
    /// The `Content-Type` of the answer.
    pub content_type: &'static str,
    /// The file's bytes.
    pub body: &'static [u8],
}

/// What the page may load: only this server. The one outside thing, an icon, is an inline data
/// URI that names no host.
pub const POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self'; \
     connect-src 'self'; img-src data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

const HTML: &str = "text/html; charset=utf-8";
const JS: &str = "text/javascript; charset=utf-8";
const CSS: &str = "text/css; charset=utf-8";

/// Every embedded file: its path on the server, its type and its bytes.
const FILES: &[(&str, &str, &[u8])] = &[
    ("/", HTML, include_bytes!("../../../../web/dist/index.html")),
    (
        "/index.html",
        HTML,
        include_bytes!("../../../../web/dist/index.html"),
    ),
    (
        "/assets/app.js",
        JS,
        include_bytes!("../../../../web/dist/assets/app.js"),
    ),
    (
        "/assets/app.css",
        CSS,
        include_bytes!("../../../../web/dist/assets/app.css"),
    ),
];

/// The embedded file served at `path`, if there is one.
pub fn find(path: &str) -> Option<Asset> {
    FILES
        .iter()
        .find(|(served, _, _)| *served == path)
        .map(|&(_, content_type, body)| Asset { content_type, body })
}

#[cfg(test)]
mod tests;
