//! Multi-page documentation site: one page per module and guide, shared navigation, local search.
//!
//! Pages are plain files in one flat directory so they open from disk without a server. The
//! fixed script only filters the bundled search index and toggles navigation/theme state; all
//! source-derived text is escaped and every link destination passes the Markdown link policy.
use super::{
    Declaration, DeclarationKind, SourceDocument, declarations, inferred, limit, markdown,
};
use crate::{Diagnostic, ast, check::editor::FunctionInfo, parse};
use std::collections::{HashMap, HashSet};

const MAX_DOCUMENTS: usize = 256;
const MAX_EXTRA_BYTES: usize = 1024 * 1024;
const MAX_LINKS: usize = 64;
const MAX_OUTPUT: usize = 64 * 1024 * 1024;
const PAGE_LIMIT: usize = 16 * 1024 * 1024;
const SUMMARY_CHARS: usize = 160;
/// Page stems owned by the generator; modules and guides may not claim them.
const RESERVED: &[&str] = &["index", "fern-docs", "fern-search"];
const STYLE: &str = include_str!("site.css");
const SCRIPT: &str = include_str!("site.js");

/// One Markdown guide with its path relative to the documented project root.
#[derive(Clone, Copy)]
pub struct Extra<'a> {
    pub path: &'a str,
    pub source: &'a str,
}

/// One external navigation link shown in the sidebar.
#[derive(Clone, Copy)]
pub struct Link<'a> {
    pub label: &'a str,
    pub url: &'a str,
}

/// Everything one site build reads; modules and extras are borrowed, never re-read.
pub struct Site<'a> {
    pub title: &'a str,
    pub version: Option<&'a str>,
    pub modules: &'a [SourceDocument<'a>],
    /// Checked signatures per module path, produced by the caller's module-graph check.
    pub schemes: Option<&'a HashMap<&'a str, &'a [FunctionInfo]>>,
    pub extras: &'a [Extra<'a>],
    pub links: &'a [Link<'a>],
}

/// One output file with a path relative to the site directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub path: String,
    pub contents: Vec<u8>,
}

struct Module<'a> {
    path: &'a str,
    name: String,
    slug: String,
    program: &'a ast::Program,
    declarations: Vec<Declaration<'a>>,
    summary: String,
}

struct Guide<'a> {
    path: &'a str,
    title: String,
    slug: String,
    body: String,
}

/// Resolves cross-references and relative links to generated page paths.
struct Resolver {
    module_slugs: HashMap<String, String>,
    module_paths: HashMap<String, String>,
    anchors: HashMap<String, HashMap<String, String>>,
    guide_paths: HashMap<String, String>,
}

struct SearchEntry {
    kind: &'static str,
    name: String,
    context: String,
    url: String,
    description: String,
}

struct Builder<'a> {
    site: &'a Site<'a>,
    modules: Vec<Module<'a>>,
    guides: Vec<Guide<'a>>,
    resolver: Resolver,
    search: Vec<SearchEntry>,
}

enum Current {
    Index,
    Module(usize),
    Guide(usize),
}

/// Render every page in memory; nothing is published unless the whole site succeeds.
pub fn render_site(site: &Site<'_>) -> Result<Vec<Page>, Diagnostic> {
    validate(site)?;
    let mut sorted: Vec<SourceDocument<'_>> = site.modules.to_vec();
    sorted.sort_by_key(|document| document.path);
    let mut programs = Vec::new();
    for document in &sorted {
        let program =
            parse::parse(document.source).map_err(|error| source_error(document, error))?;
        programs.push(program);
    }
    let mut modules = Vec::new();
    let mut count = 0;
    for (document, program) in sorted.iter().zip(&programs) {
        let mut items = declarations(document.source, program)
            .map_err(|error| source_error(document, error))?;
        if let Some(schemes) = site.schemes {
            let metadata = schemes
                .get(document.path)
                .ok_or_else(|| limit("missing checked module metadata"))?;
            inferred::attach(program, &mut items, metadata)
                .map_err(|error| source_error(document, error))?;
        }
        count += items.len();
        if count > super::MAX_DECLARATIONS {
            return Err(limit("project documentation declaration limit exceeded"));
        }
        let name = program
            .module
            .clone()
            .unwrap_or_else(|| module_name(document.path));
        let summary = program
            .module_doc
            .as_ref()
            .map(|doc| markdown::summary(&doc.text, SUMMARY_CHARS))
            .unwrap_or_default();
        modules.push(Module {
            path: document.path,
            name: name.clone(),
            slug: file_slug(&name),
            program,
            declarations: items,
            summary,
        });
    }
    let guides = guides(site.extras);
    let mut builder = Builder {
        site,
        resolver: resolver(&modules, &guides),
        modules,
        guides,
        search: Vec::new(),
    };
    builder.check_collisions()?;
    builder.pages()
}

fn validate(site: &Site<'_>) -> Result<(), Diagnostic> {
    if site.modules.is_empty() && site.extras.is_empty() {
        return Err(limit(
            "site documentation requires at least one Fern source or Markdown guide",
        ));
    }
    if site.modules.len() > MAX_DOCUMENTS || site.extras.len() > MAX_DOCUMENTS {
        return Err(limit(
            "site documentation accepts at most 256 modules and 256 guides",
        ));
    }
    if site.title.is_empty() || site.title.len() > 4096 {
        return Err(limit("site title must be 1–4096 bytes"));
    }
    if site.version.is_some_and(|version| version.len() > 256) {
        return Err(limit("site version exceeds 256 bytes"));
    }
    if site.links.len() > MAX_LINKS {
        return Err(limit("site accepts at most 64 external links"));
    }
    for link in site.links {
        if link.label.is_empty() || link.label.len() > 256 || !markdown::safe_destination(link.url)
        {
            return Err(limit(
                "site links need a label within 256 bytes and a safe destination",
            ));
        }
    }
    let mut paths = HashSet::new();
    let mut bytes = 0usize;
    for document in site.modules {
        if document.path.is_empty() || document.path.len() > 4096 || !paths.insert(document.path) {
            return Err(limit(
                "site module paths must be unique and within 4096 bytes",
            ));
        }
        bytes = bytes.saturating_add(document.source.len());
    }
    if bytes > 8 * 1024 * 1024 {
        return Err(limit("project documentation source exceeds 8 MiB"));
    }
    let mut extras = HashSet::new();
    for extra in site.extras {
        if extra.path.is_empty() || extra.path.len() > 4096 || !extras.insert(normalize(extra.path))
        {
            return Err(limit(
                "site guide paths must be unique and within 4096 bytes",
            ));
        }
        if extra.source.len() > MAX_EXTRA_BYTES {
            return Err(limit(&format!("{}: guide exceeds 1 MiB", extra.path)));
        }
    }
    Ok(())
}

/// Derive guide titles and page stems; the first README becomes the landing page.
fn guides<'a>(extras: &[Extra<'a>]) -> Vec<Guide<'a>> {
    let mut guides = Vec::new();
    let mut readme = false;
    for extra in extras {
        let stem = file_stem(extra.path);
        let (title, body) = split_title(extra.source, &stem);
        let is_readme = stem.eq_ignore_ascii_case("readme") && !readme;
        readme |= is_readme;
        let slug = if is_readme {
            "index".to_string()
        } else {
            markdown::slug(&stem)
        };
        guides.push(Guide {
            path: extra.path,
            title,
            slug,
            body,
        });
    }
    guides
}

fn resolver(modules: &[Module<'_>], guides: &[Guide<'_>]) -> Resolver {
    let mut module_slugs = HashMap::new();
    let mut module_paths = HashMap::new();
    let mut anchors = HashMap::new();
    for module in modules {
        module_slugs.insert(module.name.clone(), module.slug.clone());
        module_slugs.insert(module_name(module.path), module.slug.clone());
        module_paths.insert(normalize(module.path), module.slug.clone());
        let mut names = HashMap::new();
        for declaration in &module.declarations {
            // Functions win a shared spelling so `Name` links to the value most docs discuss.
            match declaration.kind {
                DeclarationKind::Function => {
                    names.insert(declaration.name.to_string(), anchor(declaration));
                }
                DeclarationKind::Type => {
                    names
                        .entry(declaration.name.to_string())
                        .or_insert_with(|| anchor(declaration));
                }
            }
        }
        anchors.insert(module.slug.clone(), names);
    }
    let guide_paths = guides
        .iter()
        .map(|guide| (normalize(guide.path), guide.slug.clone()))
        .collect();
    Resolver {
        module_slugs,
        module_paths,
        anchors,
        guide_paths,
    }
}

impl Resolver {
    /// Resolve inline code such as `name`, `Module.name` or `Module` from one module's page.
    fn reference(&self, text: &str, current: Option<&str>) -> Option<String> {
        if text.is_empty() || text.len() > 256 || text.contains(char::is_whitespace) {
            return None;
        }
        if let Some(slug) = current
            && let Some(anchor) = self.anchors.get(slug).and_then(|names| names.get(text))
        {
            return Some(format!("#{anchor}"));
        }
        if let Some(slug) = self.module_slugs.get(text) {
            return Some(format!("{slug}.html"));
        }
        let (module, name) = text.rsplit_once('.')?;
        let slug = self.module_slugs.get(module)?;
        let anchor = self.anchors.get(slug)?.get(name)?;
        Some(format!("{slug}.html#{anchor}"))
    }

    /// Rewrite a relative link from `from` to another documented guide or module source.
    fn link(&self, target: &str, from: &str) -> Option<String> {
        if target.contains(':') || target.starts_with('/') || target.starts_with('#') {
            return None;
        }
        let base = match from.rsplit_once('/') {
            Some((directory, _)) => format!("{directory}/{target}"),
            None => target.to_string(),
        };
        let normalized = normalize(&base);
        if let Some(slug) = self.guide_paths.get(&normalized) {
            return Some(format!("{slug}.html"));
        }
        self.module_paths
            .get(&normalized)
            .map(|slug| format!("{slug}.html"))
    }
}

impl Builder<'_> {
    /// Every page stem is claimed once; the landing README is the only legitimate `index`.
    fn check_collisions(&self) -> Result<(), Diagnostic> {
        let mut seen: HashMap<&str, Option<&str>> =
            RESERVED.iter().map(|stem| (*stem, None)).collect();
        let claims = self
            .modules
            .iter()
            .map(|module| (module.slug.as_str(), module.path))
            .chain(
                self.guides
                    .iter()
                    .filter(|guide| guide.slug != "index")
                    .map(|guide| (guide.slug.as_str(), guide.path)),
            );
        for (slug, path) in claims {
            match seen.insert(slug, Some(path)) {
                None => {}
                Some(Some(owner)) => {
                    return Err(limit(&format!(
                        "{path}: page {slug}.html collides with {owner}; rename the file or module"
                    )));
                }
                Some(None) => {
                    return Err(limit(&format!(
                        "{path}: page {slug}.html is reserved by the documentation generator"
                    )));
                }
            }
        }
        Ok(())
    }

    fn pages(&mut self) -> Result<Vec<Page>, Diagnostic> {
        let mut pages = Vec::new();
        let mut total = 0usize;
        let mut add =
            |pages: &mut Vec<Page>, path: String, text: String| -> Result<(), Diagnostic> {
                total = total.saturating_add(text.len());
                if total > MAX_OUTPUT {
                    return Err(limit("site documentation output exceeds 64 MiB"));
                }
                pages.push(Page {
                    path,
                    contents: text.into_bytes(),
                });
                Ok(())
            };
        for index in 0..self.modules.len() {
            let page = self.module_page(index)?;
            add(
                &mut pages,
                format!("{}.html", self.modules[index].slug),
                page,
            )?;
        }
        for index in 0..self.guides.len() {
            let page = self.guide_page(index)?;
            add(
                &mut pages,
                format!("{}.html", self.guides[index].slug),
                page,
            )?;
        }
        if !self.guides.iter().any(|guide| guide.slug == "index") {
            let page = self.index_page()?;
            add(&mut pages, "index.html".into(), page)?;
        }
        add(&mut pages, "fern-search.js".into(), self.search_index()?)?;
        add(&mut pages, "fern-docs.css".into(), STYLE.to_string())?;
        add(&mut pages, "fern-docs.js".into(), SCRIPT.to_string())?;
        Ok(pages)
    }

    fn markdown_options<'r>(
        &'r self,
        base_level: u8,
        id_prefix: &'r str,
        reference: &'r dyn Fn(&str) -> Option<String>,
        link: &'r dyn Fn(&str) -> Option<String>,
    ) -> markdown::Options<'r> {
        markdown::Options {
            base_level,
            reference,
            link,
            id_prefix,
            reserved: &[],
            limit: PAGE_LIMIT,
        }
    }

    fn module_page(&mut self, index: usize) -> Result<String, Diagnostic> {
        let module = &self.modules[index];
        let slug = module.slug.clone();
        let path = module.path;
        let resolver = &self.resolver;
        let reference = move |text: &str| resolver.reference(text, Some(&slug));
        let link = move |target: &str| resolver.link(target, path);
        let mut body = String::new();
        let mut members = Vec::new();
        let mut entries = Vec::new();
        let url = format!("{}.html", module.slug);
        body.push_str("<article class=\"module\">\n<header class=\"page-header\"><p class=\"kind\">module</p><h1 class=\"page-title\">");
        markdown::escape(&module.name, &mut body);
        body.push_str("</h1><p class=\"source-path\">");
        markdown::escape(module.path, &mut body);
        body.push_str("</p></header>\n");
        entries.push(SearchEntry {
            kind: "module",
            name: module.name.clone(),
            context: module.path.to_string(),
            url: url.clone(),
            description: module.summary.clone(),
        });
        if let Some(doc) = &module.program.module_doc {
            let reserved: Vec<String> = module.declarations.iter().map(anchor).collect();
            let options = markdown::Options {
                reserved: &reserved,
                ..self.markdown_options(1, "", &reference, &link)
            };
            let rendered = markdown::render(&doc.text, &options)?;
            body.push_str("<section class=\"moduledoc\">\n");
            body.push_str(&rendered.html);
            body.push_str("</section>\n");
            for heading in rendered.headings {
                entries.push(SearchEntry {
                    kind: "section",
                    name: heading.text,
                    context: module.name.clone(),
                    url: format!("{url}#{}", heading.id),
                    description: String::new(),
                });
            }
        }
        let types: Vec<&Declaration<'_>> = module
            .declarations
            .iter()
            .filter(|d| d.kind == DeclarationKind::Type)
            .collect();
        let functions: Vec<&Declaration<'_>> = module
            .declarations
            .iter()
            .filter(|d| d.kind == DeclarationKind::Function)
            .collect();
        if !types.is_empty() || !functions.is_empty() {
            body.push_str("<section class=\"summary\">\n<h2 id=\"summary\">Summary</h2>\n");
            summary_group(&mut body, "Types", &types);
            summary_group(&mut body, "Functions", &functions);
            body.push_str("</section>\n");
        }
        for (label, group) in [("Types", &types), ("Functions", &functions)] {
            if group.is_empty() {
                continue;
            }
            let id = label.to_ascii_lowercase();
            body.push_str(&format!(
                "<section class=\"declarations\" id=\"{id}\">\n<h2>{label}</h2>\n"
            ));
            for declaration in group {
                let anchor = anchor(declaration);
                members.push((anchor.clone(), declaration.name.to_string()));
                let prefix = format!("{anchor}-");
                let options = self.markdown_options(4, &prefix, &reference, &link);
                let rendered = declaration_section(declaration, &anchor, &options)?;
                body.push_str(&rendered);
                entries.push(SearchEntry {
                    kind: if declaration.kind == DeclarationKind::Type {
                        "type"
                    } else {
                        "function"
                    },
                    name: declaration.name.to_string(),
                    context: module.name.clone(),
                    url: format!("{url}#{anchor}"),
                    description: markdown::summary(declaration.doc, SUMMARY_CHARS),
                });
            }
            body.push_str("</section>\n");
        }
        body.push_str("</article>\n");
        let title = module.name.clone();
        self.search.extend(entries);
        self.shell(&title, Current::Module(index), &body, &members)
    }

    fn guide_page(&mut self, index: usize) -> Result<String, Diagnostic> {
        let guide = &self.guides[index];
        let path = guide.path;
        let resolver = &self.resolver;
        let reference = move |text: &str| resolver.reference(text, None);
        let link = move |target: &str| resolver.link(target, path);
        let options = self.markdown_options(1, "", &reference, &link);
        let rendered = markdown::render(&guide.body, &options)?;
        let mut body = String::new();
        body.push_str(
            "<article class=\"guide\">\n<header class=\"page-header\"><h1 class=\"page-title\">",
        );
        markdown::escape(&guide.title, &mut body);
        body.push_str("</h1></header>\n");
        body.push_str(&rendered.html);
        body.push_str("</article>\n");
        let url = format!("{}.html", guide.slug);
        let members: Vec<(String, String)> = rendered
            .headings
            .iter()
            .filter(|heading| heading.level == 2)
            .map(|heading| (heading.id.clone(), heading.text.clone()))
            .collect();
        let title = guide.title.clone();
        self.search.push(SearchEntry {
            kind: "guide",
            name: title.clone(),
            context: guide.path.to_string(),
            url: url.clone(),
            description: markdown::summary(&guide.body, SUMMARY_CHARS),
        });
        for heading in rendered.headings {
            self.search.push(SearchEntry {
                kind: "section",
                name: heading.text,
                context: title.clone(),
                url: format!("{url}#{}", heading.id),
                description: String::new(),
            });
        }
        self.shell(&title, Current::Guide(index), &body, &members)
    }

    fn index_page(&self) -> Result<String, Diagnostic> {
        let mut body = String::new();
        body.push_str(
            "<article class=\"overview\">\n<header class=\"page-header\"><h1 class=\"page-title\">",
        );
        markdown::escape(self.site.title, &mut body);
        body.push_str("</h1>");
        if let Some(version) = self.site.version {
            body.push_str("<p class=\"version\">Version ");
            markdown::escape(version, &mut body);
            body.push_str("</p>");
        }
        body.push_str("</header>\n");
        if !self.guides.is_empty() {
            body.push_str(
                "<section>\n<h2 id=\"guides\">Guides</h2>\n<ul class=\"overview-list\">\n",
            );
            for guide in &self.guides {
                body.push_str(&format!("<li><a href=\"{}.html\">", guide.slug));
                markdown::escape(&guide.title, &mut body);
                body.push_str("</a></li>\n");
            }
            body.push_str("</ul>\n</section>\n");
        }
        if !self.modules.is_empty() {
            body.push_str(
                "<section>\n<h2 id=\"modules\">Modules</h2>\n<ul class=\"overview-list\">\n",
            );
            for module in &self.modules {
                body.push_str(&format!("<li><a href=\"{}.html\">", module.slug));
                markdown::escape(&module.name, &mut body);
                body.push_str("</a>");
                if !module.summary.is_empty() {
                    body.push_str("<span class=\"summary-text\">");
                    markdown::escape(&module.summary, &mut body);
                    body.push_str("</span>");
                }
                body.push_str("</li>\n");
            }
            body.push_str("</ul>\n</section>\n");
        }
        body.push_str("</article>\n");
        self.shell("Overview", Current::Index, &body, &[])
    }

    /// Wrap page content in the shared layout: top bar, sidebar navigation and footer.
    fn shell(
        &self,
        title: &str,
        current: Current,
        body: &str,
        members: &[(String, String)],
    ) -> Result<String, Diagnostic> {
        let mut out = String::with_capacity(body.len() + 4096);
        out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"generator\" content=\"fern doc\">\n<title>");
        markdown::escape(title, &mut out);
        out.push_str(" — ");
        markdown::escape(self.site.title, &mut out);
        out.push_str("</title>\n<link rel=\"stylesheet\" href=\"fern-docs.css\">\n<script src=\"fern-search.js\" defer></script>\n<script src=\"fern-docs.js\" defer></script>\n</head>\n<body>\n<a class=\"skip\" href=\"#content\">Skip to content</a>\n<header class=\"topbar\">\n<button class=\"menu\" id=\"menu\" type=\"button\" aria-label=\"Toggle navigation\" aria-controls=\"sidebar\" aria-expanded=\"false\">&#9776;</button>\n<a class=\"topbar-title\" href=\"index.html\">");
        markdown::escape(self.site.title, &mut out);
        out.push_str("</a>\n<button class=\"theme\" id=\"theme\" type=\"button\" aria-label=\"Toggle dark mode\">&#9681;</button>\n</header>\n<div class=\"layout\">\n<nav class=\"sidebar\" id=\"sidebar\" aria-label=\"Documentation\">\n<div class=\"brand\"><a href=\"index.html\">");
        markdown::escape(self.site.title, &mut out);
        out.push_str("</a>");
        if let Some(version) = self.site.version {
            out.push_str(" <span class=\"version\">v");
            markdown::escape(version, &mut out);
            out.push_str("</span>");
        }
        out.push_str("</div>\n<div class=\"search\"><input id=\"search\" type=\"search\" placeholder=\"Search\" aria-label=\"Search documentation\" autocomplete=\"off\"><kbd>/</kbd></div>\n<ul id=\"search-results\" class=\"results\" hidden></ul>\n<div id=\"nav-lists\">\n");
        if !self.guides.is_empty() {
            out.push_str("<section class=\"nav-group\">\n<h2>Guides</h2>\n<ul>\n");
            for (index, guide) in self.guides.iter().enumerate() {
                let is_current = matches!(current, Current::Guide(i) if i == index);
                nav_item(
                    &mut out,
                    &format!("{}.html", guide.slug),
                    &guide.title,
                    is_current,
                    members,
                );
            }
            out.push_str("</ul>\n</section>\n");
        }
        if !self.modules.is_empty() {
            out.push_str("<section class=\"nav-group\">\n<h2>Modules</h2>\n<ul>\n");
            for (index, module) in self.modules.iter().enumerate() {
                let is_current = matches!(current, Current::Module(i) if i == index);
                nav_item(
                    &mut out,
                    &format!("{}.html", module.slug),
                    &module.name,
                    is_current,
                    members,
                );
            }
            out.push_str("</ul>\n</section>\n");
        }
        if !self.site.links.is_empty() {
            out.push_str("<section class=\"nav-group\">\n<h2>Links</h2>\n<ul>\n");
            for link in self.site.links {
                out.push_str("<li><a href=\"");
                markdown::escape(link.url, &mut out);
                out.push_str("\" rel=\"noopener\">");
                markdown::escape(link.label, &mut out);
                out.push_str("</a></li>\n");
            }
            out.push_str("</ul>\n</section>\n");
        }
        out.push_str("</div>\n</nav>\n<main id=\"content\" class=\"content\">\n");
        out.push_str(body);
        out.push_str("<footer class=\"footer\">Generated by <code>fern doc</code>.</footer>\n</main>\n</div>\n</body>\n</html>\n");
        if out.len() > PAGE_LIMIT {
            return Err(limit("site page exceeds 16 MiB"));
        }
        Ok(out)
    }

    /// Serialize the search index as a script assigning one JSON array of string fields.
    fn search_index(&self) -> Result<String, Diagnostic> {
        let mut out = String::from("window.FERN_SEARCH_INDEX = [");
        for (index, entry) in self.search.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("\n{\"t\":");
            json_string(entry.kind, &mut out);
            out.push_str(",\"n\":");
            json_string(&entry.name, &mut out);
            out.push_str(",\"m\":");
            json_string(&entry.context, &mut out);
            out.push_str(",\"u\":");
            json_string(&entry.url, &mut out);
            out.push_str(",\"d\":");
            json_string(&entry.description, &mut out);
            out.push('}');
        }
        out.push_str("\n];\n");
        if out.len() > PAGE_LIMIT {
            return Err(limit("site search index exceeds 16 MiB"));
        }
        Ok(out)
    }
}

fn nav_item(
    out: &mut String,
    href: &str,
    label: &str,
    current: bool,
    members: &[(String, String)],
) {
    out.push_str(if current {
        "<li class=\"current\"><a href=\""
    } else {
        "<li><a href=\""
    });
    markdown::escape(href, out);
    out.push_str("\">");
    markdown::escape(label, out);
    out.push_str("</a>");
    if current && !members.is_empty() {
        out.push_str("\n<ul class=\"members\">\n");
        for (anchor, name) in members {
            out.push_str("<li><a href=\"#");
            markdown::escape(anchor, out);
            out.push_str("\">");
            markdown::escape(name, out);
            out.push_str("</a></li>\n");
        }
        out.push_str("</ul>\n");
    }
    out.push_str("</li>\n");
}

fn summary_group(out: &mut String, label: &str, group: &[&Declaration<'_>]) {
    if group.is_empty() {
        return;
    }
    out.push_str(&format!("<h3>{label}</h3>\n<ul class=\"summary-list\">\n"));
    for declaration in group {
        out.push_str("<li><a href=\"#");
        markdown::escape(&anchor(declaration), out);
        out.push_str("\">");
        markdown::escape(declaration.name, out);
        out.push_str("</a>");
        let summary = markdown::summary(declaration.doc, SUMMARY_CHARS);
        if !summary.is_empty() {
            out.push_str("<span class=\"summary-text\">");
            markdown::escape(&summary, out);
            out.push_str("</span>");
        }
        out.push_str("</li>\n");
    }
    out.push_str("</ul>\n");
}

fn declaration_section(
    declaration: &Declaration<'_>,
    anchor: &str,
    options: &markdown::Options<'_>,
) -> Result<String, Diagnostic> {
    let mut out = String::new();
    out.push_str("<section class=\"declaration\" id=\"");
    markdown::escape(anchor, &mut out);
    out.push_str("\">\n<h3><a class=\"anchor\" href=\"#");
    markdown::escape(anchor, &mut out);
    out.push_str("\">");
    markdown::escape(declaration.name, &mut out);
    out.push_str("</a>");
    out.push_str(if declaration.public {
        " <span class=\"badge public\">pub</span>"
    } else {
        " <span class=\"badge private\">private</span>"
    });
    out.push_str("</h3>\n");
    for header in &declaration.headers {
        // Function headers end where the body suite begins; the colon is layout, not signature.
        let header = match declaration.kind {
            DeclarationKind::Function => header.trim_end().trim_end_matches(':'),
            DeclarationKind::Type => header.as_str(),
        };
        out.push_str("<pre class=\"signature\"><code class=\"language-fern\">");
        markdown::highlight_fern(header, &mut out);
        out.push_str("</code></pre>\n");
    }
    if let Some(checked) = &declaration.checked {
        out.push_str("<p class=\"checked-label\">Checked signature</p>\n<pre class=\"signature checked\"><code class=\"language-fern\">");
        markdown::highlight_fern(checked, &mut out);
        out.push_str("</code></pre>\n");
    }
    if !declaration.doc.trim().is_empty() {
        let rendered = markdown::render(declaration.doc, options)?;
        out.push_str("<div class=\"doc\">\n");
        out.push_str(&rendered.html);
        out.push_str("</div>\n");
    }
    out.push_str("</section>\n");
    Ok(out)
}

/// Types and values keep distinct anchors so a shared spelling never collides.
fn anchor(declaration: &Declaration<'_>) -> String {
    match declaration.kind {
        DeclarationKind::Function => declaration.name.to_string(),
        DeclarationKind::Type => format!("t:{}", declaration.name),
    }
}

/// `lib/math.fn` becomes `lib.math` when a source declares no module name.
fn module_name(path: &str) -> String {
    let stem = path.strip_suffix(".fn").unwrap_or(path);
    stem.replace(['/', '\\'], ".")
}

/// Keep page stems portable: ASCII alphanumerics, dots, dashes and underscores only.
fn file_slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
            out.push(c);
        } else if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else {
            out.push('-');
        }
    }
    if out.is_empty() { "module".into() } else { out }
}

fn file_stem(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.rsplit_once('.')
        .map_or(name, |(stem, _)| stem)
        .to_string()
}

/// Use a leading `#` heading (or a raw `<h1>` in a leading HTML block) as the page title and
/// remove it from the rendered body.
fn split_title(source: &str, stem: &str) -> (String, String) {
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            offset += line.len();
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            let title = markdown::plain(rest.trim_end_matches('#').trim());
            if !title.is_empty() {
                return (title, source[offset + line.len()..].to_string());
            }
        }
        break;
    }
    if let Some((title, body)) = html_title(source) {
        return (title, body);
    }
    (stem.replace(['_', '-'], " "), source.to_string())
}

/// Extract `<h1 …>text</h1>` from the leading raw HTML region, if the guide starts with one.
fn html_title(source: &str) -> Option<(String, String)> {
    let leading = source.get(..4096).unwrap_or(source);
    let lower = leading.to_ascii_lowercase();
    let start = lower.find("<h1")?;
    // Only raw HTML (or blank lines) may precede the heading; Markdown content means the
    // author chose a different structure.
    if source[..start]
        .lines()
        .any(|line| !line.trim().is_empty() && !line.trim_start().starts_with('<'))
    {
        return None;
    }
    let open_end = start + lower[start..].find('>')? + 1;
    let close = open_end + lower[open_end..].find("</h1>")?;
    let title = markdown::plain(source[open_end..close].trim());
    if title.is_empty() {
        return None;
    }
    let mut body = String::with_capacity(source.len());
    body.push_str(&source[..start]);
    body.push_str(&source[close + "</h1>".len()..]);
    Some((title, body))
}

/// Collapse `.` and `..` segments of a relative path without touching the filesystem.
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn source_error(document: &SourceDocument<'_>, error: Diagnostic) -> Diagnostic {
    let prefix = document.source.get(..error.span.start).unwrap_or("");
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    Diagnostic::new(
        error.span,
        format!("{}:{line}:{column}: {}", document.path, error.message),
    )
}

fn json_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
