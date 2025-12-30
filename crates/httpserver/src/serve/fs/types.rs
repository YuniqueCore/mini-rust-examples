use std::{
    collections::HashMap,
    fmt::{self, Display},
    path::Path,
};

#[derive(Clone, Debug)]
pub struct TypeMappings {
    map: HashMap<String, TypeAction>,
}

impl Default for TypeMappings {
    fn default() -> Self {
        let map = HashMap::from_iter([
            ("md".into(), TypeAction::Markdown),
            ("rs".into(), TypeAction::Code),
            ("toml".into(), TypeAction::Code),
            ]);
        Self { map }
    }
}

impl TypeMappings {
    pub fn default_value(&self) -> Option<String> {
        let mappings: Vec<String> = self
            .map
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        Some(mappings.join(";").to_string())
    }
}

#[derive(Clone, Debug)]
pub enum TypeAction {
    /// Render file as an HTML page with a `<pre><code>` block (optional highlighting).
    Code,
    /// Render Markdown as HTML.
    Markdown,
    /// Serve the raw file bytes with the given Content-Type.
    Raw { content_type: String },
}

impl Display for TypeAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeAction::Code => write!(f, "{}", "code"),
            TypeAction::Markdown => write!(f, "{}", "html"),
            TypeAction::Raw { content_type } => write!(f, "{}", content_type),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeSpecParseError {
    pub message: String,
}

impl fmt::Display for TypeSpecParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid -t spec: {}", self.message)
    }
}

impl std::error::Error for TypeSpecParseError {}

impl TypeMappings {
    pub fn parse_spec(spec: &str) -> Result<Self, TypeSpecParseError> {
        let mut mappings = TypeMappings::default();
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return Ok(mappings);
        }

        for part in trimmed.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }

            let (lhs, rhs) = part.split_once('=').ok_or_else(|| TypeSpecParseError {
                message: format!("missing '=' in segment: {part}"),
            })?;

            let lhs = lhs.trim();

            let exts_part = &lhs[..lhs.len()];
            let rhs = rhs.trim();
            if rhs.is_empty() {
                return Err(TypeSpecParseError {
                    message: format!("missing mapping value for: {lhs}"),
                });
            }

            for ext in exts_part.split('|') {
                let ext = normalize_ext(ext)?;
                let action = parse_action_for_ext(&ext, rhs)?;
                mappings.map.insert(ext, action);
            }
        }

        Ok(mappings)
    }

    pub fn action_for_path(&self, path: &Path) -> Option<TypeAction> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        self.map.get(&ext).cloned()
    }
}

fn normalize_ext(ext: &str) -> Result<String, TypeSpecParseError> {
    let ext = ext.trim();
    if ext.is_empty() {
        return Err(TypeSpecParseError {
            message: "empty extension in []".to_string(),
        });
    }
    let ext = ext.strip_prefix('.').unwrap_or(ext).to_ascii_lowercase();
    if ext
        .chars()
        .any(|c| c.is_whitespace() || c == '/' || c == '\\')
    {
        return Err(TypeSpecParseError {
            message: format!("invalid extension: {ext}"),
        });
    }
    Ok(ext)
}

fn parse_action_for_ext(ext: &str, rhs: &str) -> Result<TypeAction, TypeSpecParseError> {
    let lowered = rhs.trim().to_ascii_lowercase();
    match lowered.as_str() {
        "code" => Ok(TypeAction::Code),
        // "html" is treated as "render markdown" for .md, otherwise raw HTML.
        "html" | "markdown" | "md" => {
            if ext == "md" {
                Ok(TypeAction::Markdown)
            } else {
                Ok(TypeAction::Raw {
                    content_type: "text/html; charset=utf-8".to_string(),
                })
            }
        }
        "plain" | "text" => Ok(TypeAction::Raw {
            content_type: "text/plain; charset=utf-8".to_string(),
        }),
        other => {
            if other.contains('/') {
                Ok(TypeAction::Raw {
                    content_type: other.to_string(),
                })
            } else {
                Err(TypeSpecParseError {
                    message: format!(
                        "unsupported mapping value: {rhs} (use 'code', 'html', 'plain', or a mime like 'text/plain')"
                    ),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty() {
        let m = TypeMappings::parse_spec("").unwrap();
        assert!(m.map.is_empty());
    }

    #[test]
    fn test_parse_spec() {
        let m = TypeMappings::parse_spec("[rs|toml]=code;[md]=html;[txt]=text/plain").unwrap();
        assert!(matches!(m.map.get("rs"), Some(TypeAction::Code)));
        assert!(matches!(m.map.get("toml"), Some(TypeAction::Code)));
        assert!(matches!(m.map.get("md"), Some(TypeAction::Markdown)));
        match m.map.get("txt").unwrap() {
            TypeAction::Raw { content_type } => assert_eq!(content_type, "text/plain"),
            _ => panic!("expected Raw"),
        }
    }
}
