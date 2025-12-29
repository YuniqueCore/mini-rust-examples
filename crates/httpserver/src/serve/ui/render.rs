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
.keyword{{color:#7dd3fc;font-weight:600}}\
.comment{{color:#94a3b8}}\
.literal{{color:#86efac}}\
.glyph{{color:#fbbf24}}\
.strong-identifier{{color:#e879f9;font-weight:600}}\
.special-identifier{{color:#c4b5fd}}\
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
.keyword{{color:#7dd3fc;font-weight:600}}\
.comment{{color:#94a3b8}}\
.literal{{color:#86efac}}\
.glyph{{color:#fbbf24}}\
.strong-identifier{{color:#e879f9;font-weight:600}}\
.special-identifier{{color:#c4b5fd}}\
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
    let lang = language.trim().to_ascii_lowercase();
    let mut out = String::with_capacity(source.len() + (source.len() / 4));
    match lang.as_str() {
        "rs" | "rust" => cmark_syntax::highlight::<cmark_syntax::languages::Rust>(source, &mut out),
        "js" | "javascript" | "ts" | "typescript" => {
            cmark_syntax::highlight::<cmark_syntax::languages::JavaScript>(source, &mut out)
        }
        "sh" | "bash" => cmark_syntax::highlight::<cmark_syntax::languages::Sh>(source, &mut out),
        "toml" => cmark_syntax::highlight::<cmark_syntax::languages::Toml>(source, &mut out),
        _ => out.push_str(&html_escape(source)),
    }
    out
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
