//! Bounded Markdown rendering for documentation pages: escaped HTML, safe links, Morrow highlighting.
//!
//! The supported dialect is the CommonMark/GFM subset used by Morrow documentation: ATX and
//! setext headings, paragraphs, fenced code, nested lists and task lists, block quotes and
//! GitHub-style alerts, thematic breaks, pipe tables, footnotes, emphasis, strikethrough, code
//! spans, inline and reference links, images and autolinks. Raw HTML passes through an
//! allowlist of structural tags and attributes and is re-balanced; only `http`, `https`,
//! `mailto`, fragment and relative destinations become links or image sources.
use crate::{Diagnostic, Span};
use std::collections::{HashMap, HashSet};

/// Default output bound for one rendered document.
pub const DEFAULT_LIMIT: usize = 4 * 1024 * 1024;
const MAX_DEPTH: usize = 16;
const MAX_INLINE_DEPTH: usize = 8;

/// Rendering controls; callers resolve cross-references and rewrite relative links.
pub struct Options<'a> {
    /// HTML heading level for a `#` heading; deeper headings clamp at `h6`.
    pub base_level: u8,
    /// Resolve exact inline code text (for example `List.map`) to a link destination.
    pub reference: &'a dyn Fn(&str) -> Option<String>,
    /// Rewrite a safe link destination (for example `GUIDE.md` to `guide.html`).
    pub link: &'a dyn Fn(&str) -> Option<String>,
    /// Prefix for generated heading IDs, keeping them unique within a larger page.
    pub id_prefix: &'a str,
    /// Element IDs already used by the page; generated heading IDs avoid them.
    pub reserved: &'a [String],
    /// Maximum rendered bytes; exceeding it is an error rather than truncated output.
    pub limit: usize,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Self {
            base_level: 1,
            reference: &|_: &str| None,
            link: &|_: &str| None,
            id_prefix: "",
            reserved: &[],
            limit: DEFAULT_LIMIT,
        }
    }
}

/// One rendered heading with its plain text and generated element ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub id: String,
}

/// Complete HTML output and the heading outline used for navigation and search.
#[derive(Clone, Debug)]
pub struct Rendered {
    pub html: String,
    pub headings: Vec<Heading>,
}

/// Render one Markdown document; malformed markup degrades to literal text, never to raw HTML.
pub fn render(markdown: &str, options: &Options<'_>) -> Result<Rendered, Diagnostic> {
    let (lines, definitions) = extract_definitions(markdown.lines().collect());
    let mut renderer = Renderer {
        options,
        out: String::new(),
        headings: Vec::new(),
        ids: options.reserved.iter().cloned().collect(),
        definitions,
        footnote_order: Vec::new(),
        open_tags: Vec::new(),
    };
    renderer.blocks(&lines, 0)?;
    renderer.footnotes()?;
    renderer.close_open_tags()?;
    Ok(Rendered {
        html: renderer.out,
        headings: renderer.headings,
    })
}

/// Link reference and footnote definitions collected before rendering; they are removed
/// from the document body.
#[derive(Default)]
struct Definitions<'l> {
    links: HashMap<String, (String, Option<String>)>,
    footnotes: HashMap<String, Vec<&'l str>>,
}

/// Normalize a reference label: case-folded with collapsed internal whitespace.
fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Recognize `[label]: destination "title"` and `[^id]: text` lines outside code fences.
fn extract_definitions<'l>(lines: Vec<&'l str>) -> (Vec<&'l str>, Definitions<'l>) {
    let mut kept = Vec::with_capacity(lines.len());
    let mut definitions = Definitions::default();
    let mut fence: Option<(char, usize)> = None;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if let Some((marker, width)) = fence {
            let candidate = line.trim_start();
            let run = candidate.chars().take_while(|c| *c == marker).count();
            if run >= width && candidate[run..].trim().is_empty() {
                fence = None;
            }
            kept.push(line);
            index += 1;
            continue;
        }
        if let Some((marker, width, _)) = fence_open(line) {
            fence = Some((marker, width));
            kept.push(line);
            index += 1;
            continue;
        }
        match definition(line) {
            Some(Definition::Footnote(id)) => {
                let mut body = vec![footnote_first_line(line)];
                index += 1;
                // Indented continuation lines (and blank lines between them) belong to the note.
                while index < lines.len() {
                    let next = lines[index];
                    if next.trim().is_empty() {
                        if lines
                            .get(index + 1)
                            .is_some_and(|after| after.starts_with("    "))
                        {
                            body.push("");
                            index += 1;
                            continue;
                        }
                        break;
                    }
                    let Some(rest) = next.strip_prefix("    ") else {
                        break;
                    };
                    body.push(rest);
                    index += 1;
                }
                definitions
                    .footnotes
                    .entry(normalize_label(&id))
                    .or_insert(body);
            }
            Some(Definition::Link(label, destination, title)) => {
                definitions
                    .links
                    .entry(normalize_label(&label))
                    .or_insert((destination, title));
                index += 1;
            }
            None => {
                kept.push(line);
                index += 1;
            }
        }
    }
    (kept, definitions)
}

enum Definition {
    Footnote(String),
    Link(String, String, Option<String>),
}

fn definition(line: &str) -> Option<Definition> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = line[indent..].strip_prefix('[')?;
    let close = rest.find("]:")?;
    let label = &rest[..close];
    if label.is_empty() || label.len() > 999 || label.contains(['[', ']']) {
        return None;
    }
    let after = rest[close + 2..].trim();
    if let Some(id) = label.strip_prefix('^') {
        if id.is_empty() || id.contains(char::is_whitespace) {
            return None;
        }
        return Some(Definition::Footnote(id.to_string()));
    }
    if label.trim().is_empty() || after.is_empty() {
        return None;
    }
    let (destination, title) = split_destination(after);
    if destination.is_empty() || destination.contains(char::is_whitespace) {
        return None;
    }
    Some(Definition::Link(
        label.to_string(),
        destination.to_string(),
        title.map(str::to_string),
    ))
}

fn footnote_first_line(line: &str) -> &str {
    let start = line.find("]:").map_or(line.len(), |at| at + 2);
    line[start..].trim_start()
}

/// First sentence of the first paragraph as plain text, bounded to `max` characters.
pub fn summary(markdown: &str, max: usize) -> String {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if line.trim().is_empty() || heading(line).is_some() || thematic(line) {
            index += 1;
            continue;
        }
        if fence_open(line).is_some() || html_block_start(line) {
            index += 1;
            while index < lines.len() && !lines[index].trim().is_empty() {
                index += 1;
            }
            continue;
        }
        break;
    }
    let mut paragraph = String::new();
    while index < lines.len()
        && !lines[index].trim().is_empty()
        && fence_open(lines[index]).is_none()
    {
        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(lines[index].trim());
        index += 1;
    }
    let text = plain(&paragraph);
    let mut sentence = text.as_str();
    let mut search = 0;
    while let Some(offset) = sentence[search..].find('.') {
        let end = search + offset + 1;
        if sentence[end..]
            .chars()
            .next()
            .is_none_or(char::is_whitespace)
        {
            sentence = &sentence[..end];
            break;
        }
        search = end;
    }
    let mut result: String = sentence.chars().take(max).collect();
    if result.chars().count() < sentence.chars().count() {
        result.push('…');
    }
    result
}

/// Strip inline markup so headings and summaries can be used as plain text.
pub fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        match c {
            '`' | '*' => {}
            '_' => {
                let before = index.checked_sub(1).map(|i| chars[i]);
                let after = chars.get(index + 1);
                if before.is_some_and(char::is_alphanumeric)
                    && after.is_some_and(|c| c.is_alphanumeric())
                {
                    out.push('_');
                }
            }
            '\\' if chars
                .get(index + 1)
                .is_some_and(|c| c.is_ascii_punctuation()) =>
            {
                index += 1;
                out.push(chars[index]);
            }
            '!' if chars.get(index + 1) == Some(&'[') => {}
            '[' => {}
            ']' => {
                if chars.get(index + 1) == Some(&'(') {
                    let mut depth = 0;
                    let mut end = index + 1;
                    while end < chars.len() {
                        match chars[end] {
                            '(' => depth += 1,
                            ')' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        end += 1;
                    }
                    index = end;
                }
            }
            '<' => {
                if let Some(close) = chars[index..].iter().position(|c| *c == '>') {
                    let inner: String = chars[index + 1..index + close].iter().collect();
                    if is_tag(&inner) {
                        index += close;
                    } else {
                        out.push('<');
                    }
                } else {
                    out.push('<');
                }
            }
            _ => out.push(c),
        }
        index += 1;
    }
    out.trim().to_string()
}

/// Escape HTML metacharacters so every source-derived string remains literal text.
pub fn escape(text: &str, out: &mut String) {
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(character),
        }
    }
}

/// Lowercase ASCII-alphanumeric slug used for heading and declaration anchors.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut dash = true;
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    let trimmed = out.trim_end_matches('-');
    if trimmed.is_empty() {
        "section".into()
    } else {
        trimmed.to_string()
    }
}

/// Accept fragments, relative paths and the http, https and mailto schemes only.
pub fn safe_destination(target: &str) -> bool {
    if target.is_empty() || target.len() > 4096 {
        return false;
    }
    if target.starts_with('#') || target.starts_with('/') || target.starts_with("./") {
        return true;
    }
    let lower = target.to_ascii_lowercase();
    if ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        return true;
    }
    // A colon before any path separator or query marks an unknown scheme.
    let stop = target.find(['/', '?', '#']).unwrap_or(target.len());
    !target[..stop].contains(':')
}

struct Renderer<'a, 'l> {
    options: &'a Options<'a>,
    out: String,
    headings: Vec<Heading>,
    ids: HashSet<String>,
    definitions: Definitions<'l>,
    /// Footnote IDs in first-reference order; the index is the displayed number.
    footnote_order: Vec<String>,
    /// Raw HTML elements opened and not yet closed, so output stays well formed.
    open_tags: Vec<&'static str>,
}

/// Heading text lives in the line slice or, for setext headings, in joined paragraph lines.
enum HeadingText<'a> {
    Borrowed(&'a str),
    Owned(String),
}

enum Block<'a> {
    Heading(u8, HeadingText<'a>),
    Fence {
        language: &'a str,
        code: Vec<&'a str>,
    },
    Quote(Vec<String>),
    List {
        ordered: bool,
        start: usize,
        loose: bool,
        items: Vec<Vec<String>>,
    },
    Table {
        header: Vec<String>,
        align: Vec<Align>,
        rows: Vec<Vec<String>>,
    },
    Rule,
    Html(Vec<&'a str>),
    Paragraph(Vec<&'a str>),
}

#[derive(Clone, Copy)]
enum Align {
    None,
    Left,
    Center,
    Right,
}

const ALERTS: &[(&str, &str)] = &[
    ("note", "Note"),
    ("tip", "Tip"),
    ("important", "Important"),
    ("warning", "Warning"),
    ("caution", "Caution"),
];

impl Renderer<'_, '_> {
    fn push(&mut self, text: &str) -> Result<(), Diagnostic> {
        if text.len() > self.options.limit.saturating_sub(self.out.len()) {
            return Err(limit("documentation output exceeds its byte limit"));
        }
        self.out.push_str(text);
        Ok(())
    }

    /// Insert into already rendered output, holding the same byte limit as `push`.
    fn insert(&mut self, at: usize, text: &str) -> Result<(), Diagnostic> {
        if text.len() > self.options.limit.saturating_sub(self.out.len()) {
            return Err(limit("documentation output exceeds its byte limit"));
        }
        self.out.insert_str(at, text);
        Ok(())
    }

    fn text(&mut self, text: &str) -> Result<(), Diagnostic> {
        let mut escaped = String::new();
        escape(text, &mut escaped);
        self.push(&escaped)
    }

    /// Parse and render a sequence of lines as blocks at one container depth.
    fn blocks(&mut self, lines: &[&str], depth: usize) -> Result<(), Diagnostic> {
        if depth > MAX_DEPTH {
            return Err(limit("documentation Markdown nesting exceeds its limit"));
        }
        let mut index = 0;
        while index < lines.len() {
            let (block, next) = parse_block(lines, index);
            index = next;
            match block {
                None => {}
                Some(Block::Heading(level, text)) => {
                    let text = match &text {
                        HeadingText::Borrowed(text) => *text,
                        HeadingText::Owned(text) => text.as_str(),
                    };
                    self.heading(level, text)?;
                }
                Some(Block::Fence { language, code }) => self.fence(language, &code)?,
                Some(Block::Quote(inner)) => {
                    let mut inner: Vec<&str> = inner.iter().map(String::as_str).collect();
                    if let Some((class, title)) = alert_kind(inner.first().copied()) {
                        inner.remove(0);
                        self.push(&format!(
                            "<div class=\"alert alert-{class}\"><p class=\"alert-title\">{title}</p>\n"
                        ))?;
                        self.blocks(&inner, depth + 1)?;
                        self.push("</div>\n")?;
                    } else {
                        self.push("<blockquote>\n")?;
                        self.blocks(&inner, depth + 1)?;
                        self.push("</blockquote>\n")?;
                    }
                }
                Some(Block::List {
                    ordered,
                    start,
                    loose,
                    items,
                }) => {
                    let open = match (ordered, start) {
                        (false, _) => "<ul>\n".to_string(),
                        (true, 1) => "<ol>\n".to_string(),
                        (true, start) => format!("<ol start=\"{start}\">\n"),
                    };
                    self.push(&open)?;
                    for item in items {
                        let mut inner: Vec<&str> = item.iter().map(String::as_str).collect();
                        let task = inner.first().and_then(|first| task_marker(first));
                        match task {
                            Some((_, rest)) => {
                                inner[0] = rest;
                                self.push("<li class=\"task\">")?;
                            }
                            None => self.push("<li>")?,
                        }
                        let start = self.out.len();
                        self.blocks(&inner, depth + 1)?;
                        if !loose {
                            tighten(&mut self.out, start);
                        }
                        if let Some((checked, _)) = task {
                            // A loose item wraps its text in a paragraph, and a checkbox placed
                            // ahead of that block renders on a line above the text it labels.
                            let at = match self.out[start..].starts_with("<p>") {
                                true => start + "<p>".len(),
                                false => start,
                            };
                            self.insert(
                                at,
                                if checked {
                                    "<input type=\"checkbox\" disabled checked> "
                                } else {
                                    "<input type=\"checkbox\" disabled> "
                                },
                            )?;
                        }
                        self.push("</li>\n")?;
                    }
                    self.push(if ordered { "</ol>\n" } else { "</ul>\n" })?;
                }
                Some(Block::Table {
                    header,
                    align,
                    rows,
                }) => self.table(&header, &align, &rows)?,
                Some(Block::Rule) => self.push("<hr>\n")?,
                Some(Block::Html(raw)) => {
                    // Raw HTML keeps its literal text; only allowlisted tags survive.
                    for (offset, line) in raw.iter().enumerate() {
                        if offset > 0 {
                            self.push("\n")?;
                        }
                        self.raw_html(line)?;
                    }
                    self.push("\n")?;
                }
                Some(Block::Paragraph(text)) => {
                    let joined = text
                        .iter()
                        .map(|line| line.trim())
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.push("<p>")?;
                    self.inline(&joined, 0)?;
                    self.push("</p>\n")?;
                }
            }
        }
        Ok(())
    }

    fn heading(&mut self, level: u8, text: &str) -> Result<(), Diagnostic> {
        let level = (level + self.options.base_level - 1).min(6);
        let plain_text = plain(text);
        let base = format!("{}{}", self.options.id_prefix, slug(&plain_text));
        let mut id = base.clone();
        let mut counter = 1;
        while !self.ids.insert(id.clone()) {
            counter += 1;
            if counter > 10_000 {
                return Err(limit("documentation heading limit exceeded"));
            }
            id = format!("{base}-{counter}");
        }
        self.push(&format!("<h{level} id=\""))?;
        self.text(&id)?;
        self.push("\">")?;
        self.inline(text, 0)?;
        self.push(&format!("</h{level}>\n"))?;
        self.headings.push(Heading {
            level,
            text: plain_text,
            id,
        });
        Ok(())
    }

    fn fence(&mut self, language: &str, code: &[&str]) -> Result<(), Diagnostic> {
        let language: String = language
            .split_whitespace()
            .next()
            .unwrap_or("")
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+' | '#'))
            .collect();
        if language.is_empty() {
            self.push("<pre><code>")?;
        } else {
            self.push("<pre><code class=\"language-")?;
            self.text(&language)?;
            self.push("\">")?;
        }
        let mut body = String::new();
        for line in code {
            body.push_str(line);
            body.push('\n');
        }
        let mut rendered = String::new();
        if matches!(language.as_str(), "morrow" | "fern") {
            highlight_morrow(&body, &mut rendered);
        } else {
            escape(&body, &mut rendered);
        }
        self.push(&rendered)?;
        self.push("</code></pre>\n")
    }

    fn table(
        &mut self,
        header: &[String],
        align: &[Align],
        rows: &[Vec<String>],
    ) -> Result<(), Diagnostic> {
        let style = |align: Align| match align {
            Align::None => "",
            Align::Left => " style=\"text-align:left\"",
            Align::Center => " style=\"text-align:center\"",
            Align::Right => " style=\"text-align:right\"",
        };
        self.push("<table>\n<thead>\n<tr>")?;
        for (index, cell) in header.iter().enumerate() {
            let align = align.get(index).copied().unwrap_or(Align::None);
            self.push(&format!("<th{}>", style(align)))?;
            self.inline(cell.trim(), 0)?;
            self.push("</th>")?;
        }
        self.push("</tr>\n</thead>\n<tbody>\n")?;
        for row in rows {
            self.push("<tr>")?;
            for index in 0..header.len() {
                let align = align.get(index).copied().unwrap_or(Align::None);
                self.push(&format!("<td{}>", style(align)))?;
                if let Some(cell) = row.get(index) {
                    self.inline(cell.trim(), 0)?;
                }
                self.push("</td>")?;
            }
            self.push("</tr>\n")?;
        }
        self.push("</tbody>\n</table>\n")
    }

    /// Render inline markup; unmatched delimiters remain literal text.
    fn inline(&mut self, text: &str, depth: usize) -> Result<(), Diagnostic> {
        if depth > MAX_INLINE_DEPTH {
            return self.text(text);
        }
        let bytes = text.as_bytes();
        let mut index = 0;
        let mut plain_start = 0;
        while index < bytes.len() {
            let byte = bytes[index];
            let consumed = match byte {
                b'\\' => {
                    if bytes
                        .get(index + 1)
                        .is_some_and(|next| next.is_ascii_punctuation())
                    {
                        self.text(&text[plain_start..index])?;
                        self.text(&text[index + 1..index + 2])?;
                        Some(index + 2)
                    } else {
                        None
                    }
                }
                b'`' => self.code_span(text, index, plain_start)?,
                b'*' | b'_' => self.emphasis(text, index, plain_start, depth)?,
                b'~' if text[index..].starts_with("~~") => {
                    self.strikethrough(text, index, plain_start, depth)?
                }
                b'[' if text[index..].starts_with("[^") => {
                    self.footnote_reference(text, index, plain_start)?
                }
                b'[' => self.link(text, index, plain_start, depth, false)?,
                b'!' if bytes.get(index + 1) == Some(&b'[') => {
                    self.link(text, index, plain_start, depth, true)?
                }
                b'<' => self.angle(text, index, plain_start)?,
                b'h' if text[index..].starts_with("http://")
                    || text[index..].starts_with("https://") =>
                {
                    self.bare_url(text, index, plain_start)?
                }
                _ => None,
            };
            match consumed {
                Some(next) => {
                    index = next;
                    plain_start = next;
                }
                None => {
                    // Advance by one whole character; every special byte above is ASCII.
                    index += text[index..].chars().next().map_or(1, char::len_utf8);
                }
            }
        }
        self.text(&text[plain_start..])
    }

    fn code_span(
        &mut self,
        text: &str,
        index: usize,
        plain_start: usize,
    ) -> Result<Option<usize>, Diagnostic> {
        let run = text[index..].bytes().take_while(|b| *b == b'`').count();
        let Some(close) = find_backtick_run(text, index + run, run) else {
            return Ok(None);
        };
        self.text(&text[plain_start..index])?;
        let mut content = &text[index + run..close];
        if content.len() >= 2
            && content.starts_with(' ')
            && content.ends_with(' ')
            && !content.trim().is_empty()
        {
            content = &content[1..content.len() - 1];
        }
        let reference = (self.options.reference)(content);
        if let Some(target) = reference.filter(|target| safe_destination(target)) {
            self.push("<a href=\"")?;
            self.text(&target)?;
            self.push("\"><code>")?;
            self.text(content)?;
            self.push("</code></a>")?;
        } else {
            self.push("<code>")?;
            self.text(content)?;
            self.push("</code>")?;
        }
        Ok(Some(close + run))
    }

    fn emphasis(
        &mut self,
        text: &str,
        index: usize,
        plain_start: usize,
        depth: usize,
    ) -> Result<Option<usize>, Diagnostic> {
        let marker = text.as_bytes()[index];
        let run = text[index..].bytes().take_while(|b| *b == marker).count();
        let width = if run >= 2 { 2 } else { 1 };
        let after = text[index + width..].chars().next();
        if after.is_none_or(char::is_whitespace) {
            return Ok(None);
        }
        let before = text[..index].chars().next_back();
        if marker == b'_' && before.is_some_and(char::is_alphanumeric) {
            return Ok(None);
        }
        let Some(close) = find_closer(text, index + width, marker, width) else {
            return Ok(None);
        };
        let tag = if width == 2 { "strong" } else { "em" };
        self.text(&text[plain_start..index])?;
        self.push(&format!("<{tag}>"))?;
        self.inline(&text[index + width..close], depth + 1)?;
        self.push(&format!("</{tag}>"))?;
        Ok(Some(close + width))
    }

    fn strikethrough(
        &mut self,
        text: &str,
        index: usize,
        plain_start: usize,
        depth: usize,
    ) -> Result<Option<usize>, Diagnostic> {
        if text[index + 2..]
            .chars()
            .next()
            .is_none_or(char::is_whitespace)
        {
            return Ok(None);
        }
        let Some(close) = find_closer(text, index + 2, b'~', 2) else {
            return Ok(None);
        };
        self.text(&text[plain_start..index])?;
        self.push("<del>")?;
        self.inline(&text[index + 2..close], depth + 1)?;
        self.push("</del>")?;
        Ok(Some(close + 2))
    }

    /// `[^id]` becomes a numbered superscript link when the note is defined.
    fn footnote_reference(
        &mut self,
        text: &str,
        index: usize,
        plain_start: usize,
    ) -> Result<Option<usize>, Diagnostic> {
        let Some(close) = text[index..].find(']').map(|at| index + at) else {
            return Ok(None);
        };
        let id = &text[index + 2..close];
        if id.is_empty() || id.contains(char::is_whitespace) {
            return Ok(None);
        }
        let key = normalize_label(id);
        if !self.definitions.footnotes.contains_key(&key) {
            return Ok(None);
        }
        let (number, first) = match self.footnote_order.iter().position(|seen| *seen == key) {
            Some(position) => (position + 1, false),
            None => {
                if self.footnote_order.len() >= 1000 {
                    return Err(limit("documentation footnote limit exceeded"));
                }
                self.footnote_order.push(key.clone());
                (self.footnote_order.len(), true)
            }
        };
        self.text(&text[plain_start..index])?;
        let prefix = self.options.id_prefix;
        if first {
            self.push("<sup class=\"footnote-ref\" id=\"")?;
            self.text(&format!("{prefix}fnref-{}", slug(&key)))?;
            self.push("\">")?;
        } else {
            self.push("<sup class=\"footnote-ref\">")?;
        }
        self.push("<a href=\"#")?;
        self.text(&format!("{prefix}fn-{}", slug(&key)))?;
        self.push(&format!("\">{number}</a></sup>"))?;
        Ok(Some(close + 1))
    }

    /// Render referenced footnotes as an ordered list with a back link per note.
    fn footnotes(&mut self) -> Result<(), Diagnostic> {
        if self.footnote_order.is_empty() {
            return Ok(());
        }
        let prefix = self.options.id_prefix;
        self.push("<section class=\"footnotes\">\n<ol>\n")?;
        let order = std::mem::take(&mut self.footnote_order);
        for (position, key) in order.iter().enumerate() {
            let body = self
                .definitions
                .footnotes
                .get(key)
                .cloned()
                .unwrap_or_default();
            let id = slug(key);
            self.push("<li id=\"")?;
            self.text(&format!("{prefix}fn-{id}"))?;
            self.push("\">")?;
            let start = self.out.len();
            self.blocks(&body, 1)?;
            let mut backref = String::from(" <a href=\"#");
            escape(&format!("{prefix}fnref-{id}"), &mut backref);
            backref.push_str(&format!(
                "\" class=\"footnote-backref\" aria-label=\"Back to reference {}\">↩</a>",
                position + 1
            ));
            if self.out[start..].ends_with("</p>\n") {
                let at = self.out.len() - "</p>\n".len();
                self.out.insert_str(at, &backref);
            } else {
                self.push("<p>")?;
                self.push(backref.trim_start())?;
                self.push("</p>\n")?;
            }
            self.push("</li>\n")?;
        }
        self.push("</ol>\n</section>\n")
    }

    fn link(
        &mut self,
        text: &str,
        index: usize,
        plain_start: usize,
        depth: usize,
        image: bool,
    ) -> Result<Option<usize>, Diagnostic> {
        let open = if image { index + 1 } else { index };
        let Some(close_bracket) = find_bracket(text, open) else {
            return Ok(None);
        };
        let label = &text[open + 1..close_bracket];
        let bytes = text.as_bytes();
        let (destination, title, end): (String, Option<String>, usize) =
            if bytes.get(close_bracket + 1) == Some(&b'(') {
                let Some(close_paren) = find_paren(text, close_bracket + 1) else {
                    return Ok(None);
                };
                let (destination, title) = split_destination(&text[close_bracket + 2..close_paren]);
                (
                    destination.to_string(),
                    title.map(str::to_string),
                    close_paren + 1,
                )
            } else {
                // `[text][label]`, `[text][]` or `[label]` resolve through definitions.
                let (reference, end) = if bytes.get(close_bracket + 1) == Some(&b'[') {
                    let Some(close) = text[close_bracket + 1..]
                        .find(']')
                        .map(|at| close_bracket + 1 + at)
                    else {
                        return Ok(None);
                    };
                    let inner = &text[close_bracket + 2..close];
                    (if inner.is_empty() { label } else { inner }, close + 1)
                } else {
                    (label, close_bracket + 1)
                };
                let Some((destination, title)) =
                    self.definitions.links.get(&normalize_label(reference))
                else {
                    return Ok(None);
                };
                (destination.clone(), title.clone(), end)
            };
        self.text(&text[plain_start..index])?;
        if !safe_destination(&destination) {
            self.inline(label, depth + 1)?;
            return Ok(Some(end));
        }
        let destination = self.rewrite_destination(&destination);
        if image {
            self.push("<img src=\"")?;
            self.text(&destination)?;
            self.push("\" alt=\"")?;
            self.text(&plain(label))?;
            self.push("\"")?;
        } else {
            self.push("<a href=\"")?;
            self.text(&destination)?;
            self.push("\"")?;
        }
        if let Some(title) = title {
            self.push(" title=\"")?;
            self.text(&title)?;
            self.push("\"")?;
        }
        if image {
            return self.push(">").map(|()| Some(end));
        }
        self.push(">")?;
        self.inline(label, depth + 1)?;
        self.push("</a>")?;
        Ok(Some(end))
    }

    /// Apply the caller's relative-link rewrite to the path part of a destination.
    fn rewrite_destination(&self, destination: &str) -> String {
        let (path, fragment) = destination
            .find('#')
            .map_or((destination, ""), |at| destination.split_at(at));
        match (self.options.link)(path) {
            Some(rewritten) if !path.is_empty() => {
                let candidate = format!("{rewritten}{fragment}");
                if safe_destination(&candidate) {
                    candidate
                } else {
                    destination.to_string()
                }
            }
            _ => destination.to_string(),
        }
    }

    /// Emit one line of raw HTML: allowlisted tags are re-serialized, text is escaped.
    fn raw_html(&mut self, line: &str) -> Result<(), Diagnostic> {
        let mut rest = line;
        while let Some(start) = rest.find('<') {
            self.text(&rest[..start])?;
            match rest[start..].find('>') {
                Some(end) if is_tag(&rest[start + 1..start + end]) => {
                    self.html_tag(&rest[start + 1..start + end])?;
                    rest = &rest[start + end + 1..];
                }
                _ => {
                    self.text("<")?;
                    rest = &rest[start + 1..];
                }
            }
        }
        self.text(rest)
    }

    /// Sanitize one tag and keep the open-element stack balanced.
    fn html_tag(&mut self, inner: &str) -> Result<(), Diagnostic> {
        let link = |target: &str| self.rewrite_destination(target);
        match sanitize_tag(inner, &link) {
            Tag::Open(name, serialized) => {
                if self.open_tags.len() >= 256 {
                    return Err(limit("documentation raw HTML nesting exceeds its limit"));
                }
                self.open_tags.push(name);
                self.push(&serialized)
            }
            Tag::Void(serialized) => self.push(&serialized),
            Tag::Close(name) => {
                // Close only elements this document opened; drop stray or mismatched closers.
                if let Some(position) = self.open_tags.iter().rposition(|open| *open == name) {
                    let closing: Vec<&'static str> = self.open_tags.drain(position..).collect();
                    for open in closing.iter().rev() {
                        self.push(&format!("</{open}>"))?;
                    }
                }
                Ok(())
            }
            Tag::Drop => Ok(()),
        }
    }

    fn close_open_tags(&mut self) -> Result<(), Diagnostic> {
        while let Some(open) = self.open_tags.pop() {
            self.push(&format!("</{open}>"))?;
        }
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.push("\n")?;
        }
        Ok(())
    }

    fn angle(
        &mut self,
        text: &str,
        index: usize,
        plain_start: usize,
    ) -> Result<Option<usize>, Diagnostic> {
        let Some(offset) = text[index..].find('>') else {
            return Ok(None);
        };
        let inner = &text[index + 1..index + offset];
        if inner.contains(['<', '\n']) {
            return Ok(None);
        }
        if is_autolink(inner) {
            self.text(&text[plain_start..index])?;
            let destination = if inner.contains('@') && !inner.contains(':') {
                format!("mailto:{inner}")
            } else {
                inner.to_string()
            };
            if safe_destination(&destination) {
                self.push("<a href=\"")?;
                self.text(&destination)?;
                self.push("\">")?;
                self.text(inner)?;
                self.push("</a>")?;
            } else {
                self.text(inner)?;
            }
            return Ok(Some(index + offset + 1));
        }
        if is_tag(inner) {
            self.text(&text[plain_start..index])?;
            self.html_tag(inner)?;
            return Ok(Some(index + offset + 1));
        }
        Ok(None)
    }

    fn bare_url(
        &mut self,
        text: &str,
        index: usize,
        plain_start: usize,
    ) -> Result<Option<usize>, Diagnostic> {
        if text[..index]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '"' | '(' | '\''))
        {
            return Ok(None);
        }
        let end = text[index..]
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\''))
            .map_or(text.len(), |offset| index + offset);
        let mut url = &text[index..end];
        while let Some(stripped) = url.strip_suffix(['.', ',', ';', ':', '!', '?', '*', '_']) {
            url = stripped;
        }
        while url.ends_with(')') && url.matches('(').count() < url.matches(')').count() {
            url = &url[..url.len() - 1];
        }
        if url.len() <= "https://".len() {
            return Ok(None);
        }
        self.text(&text[plain_start..index])?;
        self.push("<a href=\"")?;
        self.text(url)?;
        self.push("\">")?;
        self.text(url)?;
        self.push("</a>")?;
        Ok(Some(index + url.len()))
    }
}

/// Recognize one block starting at `index`; returns the block and the next unconsumed line.
fn parse_block<'a>(lines: &[&'a str], index: usize) -> (Option<Block<'a>>, usize) {
    let line = lines[index];
    if line.trim().is_empty() {
        return (None, index + 1);
    }
    if let Some((marker, width, info)) = fence_open(line) {
        let mut end = index + 1;
        let mut code = Vec::new();
        while end < lines.len() {
            let candidate = lines[end].trim_start();
            let run = candidate.bytes().take_while(|b| *b == marker as u8).count();
            if run >= width
                && candidate[run..].trim().is_empty()
                && lines[end].len() - candidate.len() < 4
            {
                return (
                    Some(Block::Fence {
                        language: info,
                        code,
                    }),
                    end + 1,
                );
            }
            code.push(lines[end]);
            end += 1;
        }
        return (
            Some(Block::Fence {
                language: info,
                code,
            }),
            end,
        );
    }
    if let Some((level, text)) = heading(line) {
        return (
            Some(Block::Heading(level, HeadingText::Borrowed(text))),
            index + 1,
        );
    }
    if thematic(line) {
        return (Some(Block::Rule), index + 1);
    }
    if quote(line).is_some() {
        let mut end = index;
        let mut inner = Vec::new();
        while end < lines.len() {
            if let Some(rest) = quote(lines[end]) {
                inner.push(rest.to_string());
            } else if !lines[end].trim().is_empty()
                && inner
                    .last()
                    .is_some_and(|last: &String| !last.trim().is_empty())
                && !starts_block(lines[end])
            {
                inner.push(lines[end].to_string());
            } else {
                break;
            }
            end += 1;
        }
        return (Some(Block::Quote(inner)), end);
    }
    if let Some(item) = list_item(line) {
        return parse_list(lines, index, item);
    }
    if let Some(table) = parse_table(lines, index) {
        return table;
    }
    if html_block_start(line) {
        let mut end = index;
        let mut raw = Vec::new();
        while end < lines.len() && !lines[end].trim().is_empty() {
            raw.push(lines[end]);
            end += 1;
        }
        return (Some(Block::Html(raw)), end);
    }
    let mut end = index + 1;
    let mut text = vec![line];
    while end < lines.len() {
        let next = lines[end];
        if let Some(level) = setext_underline(next) {
            let joined = text
                .iter()
                .map(|line| line.trim())
                .collect::<Vec<_>>()
                .join("\n");
            return (
                Some(Block::Heading(level, HeadingText::Owned(joined))),
                end + 1,
            );
        }
        if next.trim().is_empty()
            || (starts_block(next)
                && !list_item(next)
                    .is_some_and(|item| item.ordered && item.content_start_number != 1))
        {
            break;
        }
        text.push(next);
        end += 1;
    }
    (Some(Block::Paragraph(text)), end)
}

/// `===` underlines make an `h1`, `---` underlines an `h2`, when they follow paragraph text.
fn setext_underline(line: &str) -> Option<u8> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let body = line.trim();
    let marker = body.chars().next()?;
    if body.len() < 2 || !matches!(marker, '=' | '-') || !body.chars().all(|c| c == marker) {
        return None;
    }
    Some(if marker == '=' { 1 } else { 2 })
}

/// GitHub-style `[!NOTE]` markers on the first line of a block quote.
fn alert_kind(first: Option<&str>) -> Option<(&'static str, &'static str)> {
    let marker = first?.trim();
    let inner = marker.strip_prefix("[!")?.strip_suffix(']')?;
    ALERTS
        .iter()
        .find(|(class, _)| class.eq_ignore_ascii_case(inner))
        .copied()
}

/// `[ ] text` and `[x] text` at the start of a list item.
fn task_marker(first: &str) -> Option<(bool, &str)> {
    let checked = if let Some(rest) = first.strip_prefix("[ ] ") {
        return Some((false, rest));
    } else if let Some(rest) = first.strip_prefix("[x] ") {
        rest
    } else {
        first.strip_prefix("[X] ")?
    };
    Some((true, checked))
}

struct ListItem<'a> {
    ordered: bool,
    indent: usize,
    content_offset: usize,
    content: &'a str,
    content_start_number: usize,
}

fn list_item(line: &str) -> Option<ListItem<'_>> {
    let indent = line.bytes().take_while(|b| *b == b' ').count();
    if indent > 3 {
        // Deeper indentation continues an enclosing item, which the list parser dedents.
        return None;
    }
    let rest = &line[indent..];
    let (marker_len, ordered, number) = if rest.starts_with(['-', '*', '+']) {
        (1, false, 0)
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || digits > 9 || !rest[digits..].starts_with(['.', ')']) {
            return None;
        }
        (digits + 1, true, rest[..digits].parse().unwrap_or(0))
    };
    let after = &rest[marker_len..];
    if !after.is_empty() && !after.starts_with(' ') {
        return None;
    }
    let spaces = after.bytes().take_while(|b| *b == b' ').count();
    let spaces = if after.is_empty() {
        1
    } else {
        spaces.clamp(1, 4)
    };
    let content_offset = indent + marker_len + spaces;
    Some(ListItem {
        ordered,
        indent,
        content_offset,
        content: after.get(spaces..).unwrap_or(""),
        content_start_number: number,
    })
}

fn parse_list<'a>(
    lines: &[&'a str],
    index: usize,
    first: ListItem<'_>,
) -> (Option<Block<'a>>, usize) {
    let ordered = first.ordered;
    let indent = first.indent;
    let start = if ordered {
        first.content_start_number
    } else {
        1
    };
    let mut items: Vec<Vec<String>> = Vec::new();
    let mut end = index;
    // A list is loose when a blank line separates two items or two blocks of one item.
    let mut loose = false;
    while end < lines.len() {
        let Some(item) =
            list_item(lines[end]).filter(|item| item.ordered == ordered && item.indent == indent)
        else {
            break;
        };
        let offset = item.content_offset;
        let mut content = vec![item.content.to_string()];
        end += 1;
        let mut blank = false;
        while end < lines.len() {
            let line = lines[end];
            if line.trim().is_empty() {
                blank = true;
                content.push(String::new());
                end += 1;
                continue;
            }
            let leading = line.bytes().take_while(|b| *b == b' ').count();
            if leading >= offset {
                loose |= blank;
                content.push(line[offset..].to_string());
                end += 1;
                continue;
            }
            if !blank && !starts_block(line) {
                content.push(line.trim_start().to_string());
                end += 1;
                continue;
            }
            break;
        }
        let trailing_blank = content.last().is_some_and(|line| line.trim().is_empty());
        while content.last().is_some_and(|line| line.trim().is_empty()) {
            content.pop();
        }
        if trailing_blank
            && end < lines.len()
            && list_item(lines[end])
                .is_some_and(|next| next.ordered == ordered && next.indent == indent)
        {
            loose = true;
        }
        items.push(content);
    }
    (
        Some(Block::List {
            ordered,
            start,
            loose,
            items,
        }),
        end,
    )
}

fn parse_table<'a>(lines: &[&'a str], index: usize) -> Option<(Option<Block<'a>>, usize)> {
    let header = lines[index];
    let delimiter = *lines.get(index + 1)?;
    if !header.contains('|') || !delimiter.contains('|') {
        return None;
    }
    let cells = split_row(header);
    let marks = split_row(delimiter);
    if marks.len() != cells.len() || cells.is_empty() {
        return None;
    }
    let mut align = Vec::new();
    for mark in &marks {
        let mark = mark.trim();
        let inner = mark.trim_matches(':');
        if inner.is_empty() || !inner.bytes().all(|b| b == b'-') {
            return None;
        }
        align.push(match (mark.starts_with(':'), mark.ends_with(':')) {
            (true, true) => Align::Center,
            (true, false) => Align::Left,
            (false, true) => Align::Right,
            (false, false) => Align::None,
        });
    }
    let mut rows = Vec::new();
    let mut end = index + 2;
    while end < lines.len() && !lines[end].trim().is_empty() && lines[end].contains('|') {
        rows.push(split_row(lines[end]));
        end += 1;
    }
    Some((
        Some(Block::Table {
            header: cells,
            align,
            rows,
        }),
        end,
    ))
}

/// Split one pipe-table row, ignoring pipes inside code spans and escaped pipes.
fn split_row(line: &str) -> Vec<String> {
    let mut trimmed = line.trim();
    trimmed = trimmed.strip_prefix('|').unwrap_or(trimmed);
    trimmed = trimmed.strip_suffix('|').unwrap_or(trimmed);
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut code = 0;
    let mut chars = trimmed.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '`' => {
                let mut run = 1;
                while chars.peek() == Some(&'`') {
                    chars.next();
                    run += 1;
                }
                code = if code == 0 {
                    run
                } else if code == run {
                    0
                } else {
                    code
                };
                current.extend(std::iter::repeat_n('`', run));
            }
            '\\' if chars.peek() == Some(&'|') => {
                chars.next();
                current.push('|');
            }
            '|' if code == 0 => cells.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    cells.push(current);
    cells
}

/// Unwrap the leading paragraph of a tight list item, keeping any following blocks.
fn tighten(out: &mut String, start: usize) {
    let body = &out[start..];
    if !body.starts_with("<p>") {
        return;
    }
    let Some(close) = body.find("</p>\n") else {
        return;
    };
    let mut rebuilt = String::with_capacity(body.len());
    rebuilt.push_str(&body["<p>".len()..close]);
    let rest = &body[close + "</p>\n".len()..];
    if !rest.is_empty() {
        rebuilt.push('\n');
        rebuilt.push_str(rest);
    }
    out.truncate(start);
    out.push_str(&rebuilt);
}

fn fence_open(line: &str) -> Option<(char, usize, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let marker = rest.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let width = rest.chars().take_while(|c| *c == marker).count();
    if width < 3 {
        return None;
    }
    let info = rest[width..].trim();
    if marker == '`' && info.contains('`') {
        return None;
    }
    Some((marker, width, info))
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let level = rest.bytes().take_while(|b| *b == b'#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let after = &rest[level..];
    if !after.is_empty() && !after.starts_with(' ') {
        return None;
    }
    let mut text = after.trim();
    let closing = text.len() - text.trim_end_matches('#').len();
    if closing > 0 {
        let before = &text[..text.len() - closing];
        if before.is_empty() || before.ends_with(' ') {
            text = before.trim_end();
        }
    }
    Some((u8::try_from(level).unwrap_or(6), text))
}

/// Whether a line begins a non-paragraph block, ending lazy paragraph continuation.
fn starts_block(line: &str) -> bool {
    heading(line).is_some()
        || fence_open(line).is_some()
        || quote(line).is_some()
        || thematic(line)
        || list_item(line).is_some()
        || html_block_start(line)
}

fn thematic(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(marker) = trimmed
        .chars()
        .next()
        .filter(|c| matches!(c, '-' | '*' | '_'))
    else {
        return false;
    };
    trimmed.chars().filter(|c| *c == marker).count() >= 3
        && trimmed.chars().all(|c| c == marker || c == ' ')
}

fn quote(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let rest = trimmed.strip_prefix('>')?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

fn html_block_start(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('<')
        && trimmed[1..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, '/' | '!'))
        && trimmed.contains('>')
}

/// Recognize `tag`, `/tag`, `tag attr="v:x"` and `!--` shapes; autolinks are never tags.
pub(crate) fn is_tag(inner: &str) -> bool {
    if inner.contains('\n') || is_autolink(inner) {
        return false;
    }
    let body = inner.strip_prefix('/').unwrap_or(inner);
    if body.starts_with('!') {
        return true;
    }
    let name = body
        .split(|c: char| c.is_whitespace() || c == '/')
        .next()
        .unwrap_or("");
    !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn is_autolink(inner: &str) -> bool {
    if inner.contains(char::is_whitespace) {
        return false;
    }
    if let Some(colon) = inner.find(':') {
        let scheme = &inner[..colon];
        return (2..=32).contains(&scheme.len())
            && scheme
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'));
    }
    inner.contains('@') && inner.rsplit('@').next().is_some_and(|d| d.contains('.'))
}

/// Structural and phrasing elements that raw HTML may contribute to a page.
const ALLOWED_TAGS: &[&str] = &[
    "a",
    "abbr",
    "b",
    "blockquote",
    "br",
    "caption",
    "cite",
    "code",
    "col",
    "colgroup",
    "dd",
    "del",
    "details",
    "dfn",
    "div",
    "dl",
    "dt",
    "em",
    "figcaption",
    "figure",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
    "img",
    "ins",
    "kbd",
    "li",
    "mark",
    "ol",
    "p",
    "pre",
    "q",
    "s",
    "samp",
    "section",
    "small",
    "span",
    "strong",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "u",
    "ul",
    "var",
    "wbr",
];
const VOID_TAGS: &[&str] = &["br", "col", "hr", "img", "wbr"];
/// Attributes kept on any allowed element; `href` and `src` additionally pass the link policy.
const ALLOWED_ATTRIBUTES: &[&str] = &[
    "align",
    "alt",
    "aria-hidden",
    "aria-label",
    "class",
    "colspan",
    "dir",
    "height",
    "href",
    "id",
    "lang",
    "open",
    "reversed",
    "role",
    "rowspan",
    "src",
    "start",
    "title",
    "width",
];

enum Tag {
    Open(&'static str, String),
    Close(&'static str),
    Void(String),
    Drop,
}

/// Re-serialize one raw tag through the allowlists; unknown tags and attributes vanish.
fn sanitize_tag(inner: &str, link: &dyn Fn(&str) -> String) -> Tag {
    let inner = inner.trim();
    if inner.starts_with('!') || inner.starts_with('?') {
        return Tag::Drop;
    }
    let (closing, body) = match inner.strip_prefix('/') {
        Some(rest) => (true, rest.trim()),
        None => (false, inner),
    };
    let name_end = body
        .find(|c: char| c.is_whitespace() || c == '/')
        .unwrap_or(body.len());
    let lower = body[..name_end].to_ascii_lowercase();
    let Some(name) = ALLOWED_TAGS.iter().find(|tag| **tag == lower).copied() else {
        return Tag::Drop;
    };
    if closing {
        return if VOID_TAGS.contains(&name) {
            Tag::Drop
        } else {
            Tag::Close(name)
        };
    }
    let mut serialized = format!("<{name}");
    let mut rest = body[name_end..].trim_start();
    let mut count = 0;
    while !rest.is_empty() && rest != "/" {
        count += 1;
        if count > 32 {
            break;
        }
        let attribute_end = rest
            .find(|c: char| c.is_whitespace() || c == '=' || c == '/')
            .unwrap_or(rest.len());
        let attribute = rest[..attribute_end].to_ascii_lowercase();
        rest = rest[attribute_end..].trim_start();
        let value = if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            if let Some(quote) = after.chars().next().filter(|c| matches!(c, '"' | '\'')) {
                let closing = after[1..].find(quote).map_or(after.len(), |at| at + 1);
                let value = &after[1..closing];
                rest = after.get(closing + 1..).unwrap_or("").trim_start();
                Some(value)
            } else {
                let end = after
                    .find(|c: char| c.is_whitespace() || c == '>')
                    .unwrap_or(after.len());
                let value = &after[..end];
                rest = after[end..].trim_start();
                Some(value)
            }
        } else {
            None
        };
        if attribute.is_empty() {
            // A lone `/` or malformed token; skip one character so the scan always advances.
            rest = rest.get(1..).unwrap_or("").trim_start();
            continue;
        }
        if !ALLOWED_ATTRIBUTES.contains(&attribute.as_str()) {
            continue;
        }
        let value = value.map(decode_entities);
        match (attribute.as_str(), &value) {
            ("href" | "src", Some(target)) => {
                if !safe_destination(target) {
                    continue;
                }
                let target = if attribute == "href" {
                    link(target)
                } else {
                    target.clone()
                };
                serialized.push_str(&format!(" {attribute}=\""));
                escape(&target, &mut serialized);
                serialized.push('"');
            }
            ("href" | "src", None) => {}
            (_, Some(value)) => {
                serialized.push_str(&format!(" {attribute}=\""));
                escape(value, &mut serialized);
                serialized.push('"');
            }
            (_, None) => serialized.push_str(&format!(" {attribute}")),
        }
    }
    serialized.push('>');
    if VOID_TAGS.contains(&name) {
        Tag::Void(serialized)
    } else {
        Tag::Open(name, serialized)
    }
}

/// Decode the entities HTML attribute values commonly carry before re-escaping them.
fn decode_entities(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn find_backtick_run(text: &str, from: usize, run: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = from;
    while index < bytes.len() {
        if bytes[index] == b'`' {
            let width = bytes[index..].iter().take_while(|b| **b == b'`').count();
            if width == run {
                return Some(index);
            }
            index += width;
        } else {
            index += 1;
        }
    }
    None
}

/// Locate a closing emphasis run of exactly `width`, skipping code spans and requiring
/// non-whitespace before the closer.
fn find_closer(text: &str, from: usize, marker: u8, width: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = from;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'`' {
            let run = bytes[index..].iter().take_while(|b| **b == b'`').count();
            index = find_backtick_run(text, index + run, run).map_or(index + run, |c| c + run);
            continue;
        }
        if byte == marker {
            let run = bytes[index..].iter().take_while(|b| **b == marker).count();
            let before = text[..index].chars().next_back();
            let after = text[index + run..].chars().next();
            let boundary_ok = marker != b'_' || after.is_none_or(|c| !c.is_alphanumeric());
            if run == width
                && index > from
                && before.is_some_and(|c| !c.is_whitespace())
                && boundary_ok
            {
                return Some(index);
            }
            index += run;
            continue;
        }
        index += 1;
    }
    None
}

fn find_bracket(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0;
    let mut index = open;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 1,
            b'`' => {
                let run = bytes[index..].iter().take_while(|b| **b == b'`').count();
                index = find_backtick_run(text, index + run, run).map_or(index + run, |c| c + run);
                continue;
            }
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn find_paren(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0;
    let mut index = open;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 1,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            b'\n' => return None,
            _ => {}
        }
        index += 1;
    }
    None
}

/// Separate `destination "title"`; angle-bracketed destinations lose their brackets.
fn split_destination(text: &str) -> (&str, Option<&str>) {
    let text = text.trim();
    let (destination, rest) = if let Some(inner) = text.strip_prefix('<') {
        match inner.find('>') {
            Some(end) => (&inner[..end], inner[end + 1..].trim()),
            None => (text, ""),
        }
    } else {
        match text.find(char::is_whitespace) {
            Some(end) => (&text[..end], text[end..].trim()),
            None => (text, ""),
        }
    };
    let title = rest
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .or_else(|| rest.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')))
        .or_else(|| rest.strip_prefix('(').and_then(|r| r.strip_suffix(')')));
    (destination, title)
}

const KEYWORDS: &[&str] = &[
    "fn", "let", "const", "comptime", "if", "else", "true", "false", "and", "or", "not", "pub",
    "type", "match", "return", "for", "in", "while", "import", "with", "trait", "impl", "actor",
    "receive", "spawn", "where", "do", "defer", "as", "module", "break", "continue", "derive",
    "newtype", "send", "after", "foreign", "self",
];

/// Wrap Morrow tokens in classed spans; all text is escaped and no token is executed.
pub fn highlight_morrow(code: &str, out: &mut String) {
    let chars: Vec<char> = code.chars().collect();
    let mut index = 0;
    let span = |class: &str, text: &str, out: &mut String| {
        out.push_str("<span class=\"");
        out.push_str(class);
        out.push_str("\">");
        escape(text, out);
        out.push_str("</span>");
    };
    while index < chars.len() {
        let c = chars[index];
        let start = index;
        if c == '"' {
            let triple = chars[index..].starts_with(&['"', '"', '"']);
            index += if triple { 3 } else { 1 };
            loop {
                if index >= chars.len() {
                    break;
                }
                if chars[index] == '\\' {
                    index += 2;
                    continue;
                }
                if triple && chars[index..].starts_with(&['"', '"', '"']) {
                    index += 3;
                    break;
                }
                if !triple && (chars[index] == '"' || chars[index] == '\n') {
                    index += usize::from(chars[index] == '"');
                    break;
                }
                index += 1;
            }
            let text: String = chars[start..index.min(chars.len())].iter().collect();
            span("str", &text, out);
            continue;
        }
        if c == '#' {
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            let text: String = chars[start..index].iter().collect();
            let class = if text.trim_start_matches('#').trim_start().starts_with("=>") {
                "result"
            } else {
                "cmt"
            };
            span(class, &text, out);
            continue;
        }
        if c == '@' && chars.get(index + 1).is_some_and(|c| c.is_alphabetic()) {
            index += 1;
            while index < chars.len() && chars[index].is_alphanumeric() {
                index += 1;
            }
            let text: String = chars[start..index].iter().collect();
            span("attr", &text, out);
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            while index < chars.len() && (chars[index].is_alphanumeric() || chars[index] == '_') {
                index += 1;
            }
            let text: String = chars[start..index].iter().collect();
            let next = chars[index..].iter().find(|c| **c != ' ');
            if KEYWORDS.contains(&text.as_str()) {
                span("kw", &text, out);
            } else if c.is_uppercase() {
                span("ty", &text, out);
            } else if next == Some(&'(') {
                span("fn", &text, out);
            } else {
                escape(&text, out);
            }
            continue;
        }
        if c.is_ascii_digit() {
            while index < chars.len()
                && (chars[index].is_ascii_alphanumeric() || matches!(chars[index], '.' | '_'))
            {
                index += 1;
            }
            let text: String = chars[start..index].iter().collect();
            span("num", &text, out);
            continue;
        }
        index += 1;
        escape(&c.to_string(), out);
    }
}

fn limit(message: &str) -> Diagnostic {
    Diagnostic::new(Span::default(), message)
}
