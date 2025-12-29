use crate::serve::url::html_escape;

pub fn render_code_page(title: &str, language: Option<&str>, source: &str) -> Vec<u8> {
    let lang = language.unwrap_or("");
    let escaped_title = html_escape(title);
    let highlighted = highlight_code_html(source, lang);

    let html = format!(
        "<!doctype html>\
<html><head><meta charset=\"utf-8\">\
<title>{title}</title>\
<style>\
body{{font-family:system-ui,-apple-system,Segoe UI,Roboto,sans-serif;padding:16px}}\
pre{{background:#0b1020;color:#e6e6e6;padding:12px;border-radius:8px;overflow:auto}}\
.kw{{color:#7dd3fc;font-weight:600}}\
.com{{color:#94a3b8}}\
.str{{color:#86efac}}\
.num{{color:#fbbf24}}\
</style>\
</head><body>\
<h1 style=\"font-size:16px;margin:0 0 12px 0\">{title}</h1>\
<pre><code class=\"language-{lang}\">{code}</code></pre>\
</body></html>",
        title = escaped_title,
        lang = html_escape(lang),
        code = highlighted
    );
    html.into_bytes()
}

pub fn render_markdown_page(title: &str, markdown: &str) -> Vec<u8> {
    let escaped_title = html_escape(title);
    let body = render_markdown_minimal(markdown);
    let html = format!(
        "<!doctype html>\
<html><head><meta charset=\"utf-8\">\
<title>{title}</title>\
<style>\
body{{font-family:system-ui,-apple-system,Segoe UI,Roboto,sans-serif;max-width:900px;margin:24px auto;padding:0 16px;line-height:1.5}}\
pre{{background:#0b1020;color:#e6e6e6;padding:12px;border-radius:8px;overflow:auto}}\
code{{font-family:ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace}}\
.kw{{color:#7dd3fc;font-weight:600}}\
.com{{color:#94a3b8}}\
.str{{color:#86efac}}\
.num{{color:#fbbf24}}\
a{{text-decoration:none}} a:hover{{text-decoration:underline}}\
</style>\
</head><body>\
{body}\
</body></html>",
        title = escaped_title,
        body = body
    );
    html.into_bytes()
}

fn highlight_code_html(source: &str, language: &str) -> String {
    let profile = language_profile(language);
    highlight_with_profile(source, &profile)
}

#[derive(Clone)]
struct LanguageProfile<'a> {
    keywords: &'a [&'a str],
    line_comments: &'a [&'a str],
    block_comments: &'a [(&'a str, &'a str)],
    string_quotes: &'a [char],
}

fn language_profile(language: &str) -> LanguageProfile<'static> {
    let lang = language.trim().to_ascii_lowercase();
    match lang.as_str() {
        "rs" | "rust" => LanguageProfile {
            keywords: RUST_KEYWORDS,
            line_comments: &["//"],
            block_comments: &[("/*", "*/")],
            string_quotes: &['"', '\''],
        },
        "go" => LanguageProfile {
            keywords: GO_KEYWORDS,
            line_comments: &["//"],
            block_comments: &[("/*", "*/")],
            string_quotes: &['"', '\'', '`'],
        },
        "js" | "ts" | "jsx" | "tsx" => LanguageProfile {
            keywords: JS_KEYWORDS,
            line_comments: &["//"],
            block_comments: &[("/*", "*/")],
            string_quotes: &['"', '\'', '`'],
        },
        "py" | "python" => LanguageProfile {
            keywords: PY_KEYWORDS,
            line_comments: &["#"],
            block_comments: &[],
            string_quotes: &['"', '\''],
        },
        "toml" | "yaml" | "yml" | "sh" | "bash" => LanguageProfile {
            keywords: &[],
            line_comments: &["#"],
            block_comments: &[],
            string_quotes: &['"', '\''],
        },
        "html" | "xml" => LanguageProfile {
            keywords: &[],
            line_comments: &[],
            block_comments: &[("<!--", "-->")],
            string_quotes: &['"', '\''],
        },
        "css" => LanguageProfile {
            keywords: &[],
            line_comments: &[],
            block_comments: &[("/*", "*/")],
            string_quotes: &['"', '\''],
        },
        _ => LanguageProfile {
            keywords: &[],
            line_comments: &["//", "#"],
            block_comments: &[("/*", "*/")],
            string_quotes: &['"', '\''],
        },
    }
}

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];

const GO_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
];

const JS_KEYWORDS: &[&str] = &[
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "null",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

const PY_KEYWORDS: &[&str] = &[
    "and", "as", "assert", "break", "class", "continue", "def", "del", "elif", "else", "except",
    "False", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "None",
    "nonlocal", "not", "or", "pass", "raise", "return", "True", "try", "while", "with", "yield",
];

fn highlight_with_profile(source: &str, profile: &LanguageProfile<'_>) -> String {
    let mut out = String::with_capacity(source.len());
    let mut i = 0usize;
    let mut word_start: Option<usize> = None;

    while i < source.len() {
        let rest = &source[i..];

        if let Some((_, end)) = profile
            .block_comments
            .iter()
            .find(|(s, _e)| rest.starts_with(*s))
            .copied()
        {
            flush_word(source, &mut out, &mut word_start, i, profile.keywords);
            if let Some(close_pos) = rest.find(end) {
                let end_idx = i + close_pos + end.len();
                out.push_str("<span class=\"com\">");
                out.push_str(&html_escape(&source[i..end_idx]));
                out.push_str("</span>");
                i = end_idx;
                continue;
            }
            out.push_str("<span class=\"com\">");
            out.push_str(&html_escape(rest));
            out.push_str("</span>");
            break;
        }

        if profile
            .line_comments
            .iter()
            .any(|s| !s.is_empty() && rest.starts_with(*s))
        {
            flush_word(source, &mut out, &mut word_start, i, profile.keywords);
            let end_idx = match rest.find('\n') {
                Some(pos) => i + pos + 1,
                None => source.len(),
            };
            out.push_str("<span class=\"com\">");
            out.push_str(&html_escape(&source[i..end_idx]));
            out.push_str("</span>");
            i = end_idx;
            continue;
        }

        let ch = rest.chars().next().unwrap_or('\0');
        if profile.string_quotes.contains(&ch) {
            flush_word(source, &mut out, &mut word_start, i, profile.keywords);
            let end_idx = consume_string(source, i, ch);
            out.push_str("<span class=\"str\">");
            out.push_str(&html_escape(&source[i..end_idx]));
            out.push_str("</span>");
            i = end_idx;
            continue;
        }

        if is_word_char(ch) {
            if word_start.is_none() {
                word_start = Some(i);
            }
            i += ch.len_utf8();
            continue;
        }

        flush_word(source, &mut out, &mut word_start, i, profile.keywords);
        out.push_str(&html_escape(&ch.to_string()));
        i += ch.len_utf8();
    }

    flush_word(
        source,
        &mut out,
        &mut word_start,
        source.len(),
        profile.keywords,
    );
    out
}

fn consume_string(source: &str, start: usize, quote: char) -> usize {
    let mut i = start + quote.len_utf8();
    while i < source.len() {
        let rest = &source[i..];
        let ch = rest.chars().next().unwrap_or('\0');
        if ch == '\\' {
            i += ch.len_utf8();
            if i < source.len() {
                let next = source[i..].chars().next().unwrap_or('\0');
                i += next.len_utf8();
            }
            continue;
        }
        i += ch.len_utf8();
        if ch == quote {
            break;
        }
    }
    i
}

fn is_word_char(c: char) -> bool {
    matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '_')
}

fn flush_word(
    source: &str,
    out: &mut String,
    word_start: &mut Option<usize>,
    word_end: usize,
    keywords: &[&str],
) {
    let Some(start) = word_start.take() else {
        return;
    };
    let word = &source[start..word_end];

    if keywords.contains(&word) {
        out.push_str("<span class=\"kw\">");
        out.push_str(&html_escape(word));
        out.push_str("</span>");
    } else if word.chars().all(|c| c.is_ascii_digit()) {
        out.push_str("<span class=\"num\">");
        out.push_str(&html_escape(word));
        out.push_str("</span>");
    } else {
        out.push_str(&html_escape(word));
    }
}

fn render_markdown_minimal(md: &str) -> String {
    // Minimal, safe markdown rendering:
    // - headings (#..######)
    // - unordered list (- )
    // - fenced code blocks (```lang)
    // - paragraphs
    // - inline code (`code`)
    let mut out = String::new();
    let mut in_code = false;
    let mut in_list = false;
    let mut code_lang = String::new();
    let mut code_buf = String::new();

    for raw_line in md.lines() {
        let line = raw_line.trim_end_matches('\r');

        if let Some(fence) = line.strip_prefix("```") {
            if in_list {
                out.push_str("</ul>");
                in_list = false;
            }
            if in_code {
                let highlighted = highlight_code_html(&code_buf, &code_lang);
                out.push_str(&format!(
                    "<pre><code class=\"language-{}\">{}</code></pre>",
                    html_escape(&code_lang),
                    highlighted
                ));
                code_buf.clear();
                code_lang.clear();
                in_code = false;
            } else {
                in_code = true;
                code_lang = fence.trim().to_string();
                code_buf.clear();
            }
            continue;
        }

        if in_code {
            code_buf.push_str(line);
            code_buf.push('\n');
            continue;
        }

        if line.is_empty() {
            if in_list {
                out.push_str("</ul>");
                in_list = false;
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("- ") {
            if !in_list {
                out.push_str("<ul>");
                in_list = true;
            }
            out.push_str("<li>");
            out.push_str(&render_inline_code(rest));
            out.push_str("</li>");
            continue;
        }

        if in_list {
            out.push_str("</ul>");
            in_list = false;
        }

        let heading_level = line.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&heading_level) && line.chars().nth(heading_level) == Some(' ') {
            let text = line[heading_level + 1..].trim();
            out.push_str(&format!(
                "<h{lvl}>{text}</h{lvl}>",
                lvl = heading_level,
                text = render_inline_code(text)
            ));
            continue;
        }

        out.push_str("<p>");
        out.push_str(&render_inline_code(line));
        out.push_str("</p>");
    }

    if in_list {
        out.push_str("</ul>");
    }
    if in_code {
        let highlighted = highlight_code_html(&code_buf, &code_lang);
        out.push_str(&format!(
            "<pre><code class=\"language-{}\">{}</code></pre>",
            html_escape(&code_lang),
            highlighted
        ));
    }
    out
}

fn render_inline_code(text: &str) -> String {
    // naive inline code rendering: split by backticks and alternate
    let mut out = String::new();
    let mut in_code = false;
    for part in text.split('`') {
        if in_code {
            out.push_str("<code>");
            out.push_str(&html_escape(part));
            out.push_str("</code>");
        } else {
            out.push_str(&html_escape(part));
        }
        in_code = !in_code;
    }
    out
}
