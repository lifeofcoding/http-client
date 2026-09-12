use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug)]
pub struct Secrets {
    vars: HashMap<String, String>,
    masked: Vec<String>,
}

#[derive(Debug, Default)]
pub struct SubstError {
    pub unresolved: Vec<String>,
    pub malformed: bool,
}

impl Secrets {
    pub fn from_maps(base: HashMap<String, String>, overrides: HashMap<String, String>) -> Self {
        let mut vars = base;
        vars.extend(overrides);
        Secrets {
            vars,
            masked: Vec::new(),
        }
    }

    pub fn load(env_files: &[PathBuf]) -> Result<Self, String> {
        let mut overrides: HashMap<String, String> = HashMap::new();
        for path in env_files {
            let content = fs::read_to_string(path)
                .map_err(|e| format!("cannot read env file {}: {e}", path.display()))?;
            let parsed =
                parse_env_file(&content).map_err(|e| format!("{}: {e}", path.display()))?;
            overrides.extend(parsed);
        }
        Ok(Secrets::from_maps(std::env::vars().collect(), overrides))
    }

    pub fn substitute(&mut self, text: &str) -> Result<String, SubstError> {
        let mut out = String::with_capacity(text.len());
        let mut err = SubstError::default();
        let mut rest = text;

        while let Some(open) = rest.find("{{") {
            out.push_str(&rest[..open]);
            let after = &rest[open + 2..];
            match after.find("}}") {
                None => {
                    err.malformed = true;
                    out.push_str("{{");
                    out.push_str(after);
                    rest = "";
                }
                Some(close) => {
                    let name = after[..close].trim();
                    if name.is_empty() {
                        err.malformed = true;
                        out.push_str("{{}}");
                    } else if let Some(value) = self.vars.get(name) {
                        if !self.masked.iter().any(|m| m == value) {
                            self.masked.push(value.clone());
                        }
                        out.push_str(value);
                    } else {
                        if !err.unresolved.iter().any(|n| n.as_str() == name) {
                            err.unresolved.push(name.to_string());
                        }
                        out.push_str("{{");
                        out.push_str(&after[..close + 2]);
                    }
                    rest = &after[close + 2..];
                }
            }
        }
        out.push_str(rest);

        if err.malformed || !err.unresolved.is_empty() {
            Err(err)
        } else {
            Ok(out)
        }
    }

    pub fn masked_values(&self) -> &[String] {
        &self.masked
    }
}

pub fn redact(text: &str, masked: &[String]) -> String {
    if masked.is_empty() {
        return text.to_string();
    }
    let mut values: Vec<&String> = masked.iter().collect();
    values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    let mut out = text.to_string();
    for value in values {
        if !value.is_empty() {
            out = out.replace(value.as_str(), "[redacted]");
        }
    }
    out
}

fn parse_env_file(content: &str) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    for (lineno, raw) in content.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line
            .strip_prefix("export ")
            .map(str::trim_start)
            .unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!(
                "line {}: expected `KEY=value`, got `{line}`",
                lineno + 1
            ));
        };
        let key = key.trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!("line {}: invalid key `{key}`", lineno + 1));
        }
        let value = strip_quotes(value.trim());
        map.insert(key.to_string(), value.to_string());
    }
    Ok(map)
}

fn strip_quotes(value: &str) -> &str {
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(pairs: &[(&str, &str)]) -> Secrets {
        Secrets::from_maps(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            HashMap::new(),
        )
    }

    fn err_of(text: &str, s: &mut Secrets) -> SubstError {
        s.substitute(text)
            .expect_err("expected substitution failure")
    }

    #[test]
    fn env_file_parsing() {
        let content = [
            "# comment",
            "",
            "export PLAIN=value",
            "QUOTED=\"double quoted\"",
            "SINGLE='single'",
            "KEY=value=with=equals",
            "URL=https://x.dev/api?key=1",
            "EMPTY=",
        ]
        .join("\n");
        let map = parse_env_file(&content).unwrap();
        assert_eq!(map["PLAIN"], "value");
        assert_eq!(map["QUOTED"], "double quoted");
        assert_eq!(map["SINGLE"], "single");
        assert_eq!(map["KEY"], "value=with=equals");
        assert_eq!(map["URL"], "https://x.dev/api?key=1");
        assert_eq!(map["EMPTY"], "");
    }

    #[test]
    fn env_file_rejects_bad_lines() {
        assert!(parse_env_file("no equals sign").is_err());
        assert!(parse_env_file("= novalue").is_err());
        assert!(parse_env_file("BAD-KEY=1").is_err());
    }

    #[test]
    fn file_overrides_process_env() {
        let mut base = HashMap::new();
        base.insert("TOKEN".to_string(), "from-env".to_string());
        let mut overrides = HashMap::new();
        overrides.insert("TOKEN".to_string(), "from-file".to_string());
        let mut secrets = Secrets::from_maps(base, overrides);
        assert_eq!(secrets.substitute("{{TOKEN}}").unwrap(), "from-file");
    }

    #[test]
    fn substitutes_across_text_and_records_masked() {
        let mut secrets = make(&[("TOKEN", "abc123"), ("HOST", "api.dev")]);
        assert_eq!(
            secrets
                .substitute("Bearer {{TOKEN}} @ {{HOST}}/{{TOKEN}}")
                .unwrap(),
            "Bearer abc123 @ api.dev/abc123"
        );
        assert_eq!(
            secrets.masked_values().to_vec(),
            vec!["abc123".to_string(), "api.dev".to_string()]
        );
    }

    #[test]
    fn unused_variables_are_not_masked() {
        let mut secrets = make(&[("USED", "abc123"), ("UNUSED", "zzzz")]);
        let _ = secrets.substitute("{{USED}}").unwrap();
        assert_eq!(secrets.masked_values().to_vec(), vec!["abc123".to_string()]);
    }

    #[test]
    fn leaves_plain_text_untouched() {
        let mut secrets = make(&[]);
        assert_eq!(
            secrets.substitute("no vars }} here { only").unwrap(),
            "no vars }} here { only"
        );
    }

    #[test]
    fn trims_variable_names() {
        let mut secrets = make(&[("TOKEN", "abc123")]);
        assert_eq!(secrets.substitute("{{ TOKEN }}").unwrap(), "abc123");
    }

    #[test]
    fn unknown_variables_are_reported() {
        let mut secrets = make(&[("KNOWN", "v")]);
        let err = err_of(
            "{{KNOWN}} {{MISSING_A}} {{MISSING_B}} {{MISSING_A}}",
            &mut secrets,
        );
        assert_eq!(err.unresolved, ["MISSING_A", "MISSING_B"]);
        assert!(!err.malformed);
    }

    #[test]
    fn unterminated_and_empty_placeholders_are_malformed() {
        let mut secrets = make(&[]);
        assert!(err_of("hello {{world", &mut secrets).malformed);
        assert!(err_of("hello {{}}", &mut secrets).malformed);
    }

    #[test]
    fn redaction_masks_all_occurrences() {
        assert_eq!(
            redact("Bearer abc123 in abc123", &["abc123".to_string()]),
            "Bearer [redacted] in [redacted]"
        );
    }

    #[test]
    fn redaction_handles_overlapping_values() {
        assert_eq!(
            redact("x abcdef y", &["abc".to_string(), "abcdef".to_string()]),
            "x [redacted] y"
        );
    }

    #[test]
    fn redaction_without_values_is_identity() {
        assert_eq!(redact("text", &[]), "text");
    }
}
