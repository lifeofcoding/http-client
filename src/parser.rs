use std::fmt;

#[derive(Debug, Clone)]
pub struct ParsedRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

#[derive(Debug)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub fn parse(content: &str) -> Result<Vec<ParsedRequest>, ParseError> {
    let mut requests = Vec::new();
    let mut current: Option<ParsedRequest> = None;
    let mut in_body = false;
    let mut body_lines: Vec<String> = Vec::new();

    for raw_line in content.lines() {
        let line = raw_line.trim_end();

        if line.starts_with("###") {
            flush(&mut requests, &mut current, &mut in_body, &mut body_lines);
            continue;
        }

        if !in_body && line.trim_start().starts_with('#') {
            continue;
        }

        match current.as_mut() {
            None => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let (method, url) = split_request_line(trimmed)?;
                current = Some(ParsedRequest {
                    method,
                    url,
                    headers: Vec::new(),
                    body: None,
                });
            }
            Some(req) => {
                if in_body {
                    body_lines.push(line.to_string());
                } else if line.trim().is_empty() {
                    in_body = true;
                } else if let Some((name, value)) = line.split_once(':') {
                    req.headers
                        .push((name.trim().to_string(), value.trim().to_string()));
                } else {
                    return Err(ParseError(format!("invalid header line: `{line}`")));
                }
            }
        }
    }

    flush(&mut requests, &mut current, &mut in_body, &mut body_lines);
    Ok(requests)
}

fn flush(
    requests: &mut Vec<ParsedRequest>,
    current: &mut Option<ParsedRequest>,
    in_body: &mut bool,
    body_lines: &mut Vec<String>,
) {
    if let Some(mut req) = current.take() {
        if !body_lines.is_empty() {
            let body = body_lines.join("\n").trim().to_string();
            if !body.is_empty() {
                req.body = Some(body);
            }
        }
        requests.push(req);
    }
    *in_body = false;
    body_lines.clear();
}

fn split_request_line(line: &str) -> Result<(String, String), ParseError> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    match parts.as_slice() {
        [method, url] => Ok((method.to_uppercase(), url.to_string())),
        [method, url, version] if version.to_uppercase().starts_with("HTTP/") => {
            Ok((method.to_uppercase(), url.to_string()))
        }
        _ => Err(ParseError(format!(
            "invalid request line: `{line}` (expected `METHOD URL [HTTP/x.x]`)"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_request_like_example_http() {
        let input = [
            "POST https://example.com/api/cleanupVoices HTTP/1.1",
            "content-type: application/json",
            "Authorization: Bearer token=",
            "",
            "{",
            "  \"limit\": 300",
            "}",
        ]
        .join("\n");
        let requests = parse(&input).unwrap();
        assert_eq!(requests.len(), 1);
        let r = &requests[0];
        assert_eq!(r.method, "POST");
        assert_eq!(r.url, "https://example.com/api/cleanupVoices");
        assert_eq!(r.headers.len(), 2);
        assert_eq!(r.headers[0].0, "content-type");
        assert_eq!(r.body.as_deref(), Some("{\n  \"limit\": 300\n}"));
    }

    #[test]
    fn splits_multiple_requests_on_separator_and_skips_comments() {
        let input = [
            "# top comment",
            "GET https://a.dev/one HTTP/1.1",
            "Accept: text/plain",
            "",
            "",
            "###",
            "# another comment",
            "POST https://b.dev/two",
            "",
            "hello=world",
        ]
        .join("\n");
        let requests = parse(&input).unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].url, "https://a.dev/one");
        assert_eq!(requests[0].body, None);
        assert_eq!(requests[1].method, "POST");
        assert_eq!(requests[1].body.as_deref(), Some("hello=world"));
    }

    #[test]
    fn rejects_bad_request_line() {
        assert!(parse("not-a-request-line\n").is_err());
    }
}
