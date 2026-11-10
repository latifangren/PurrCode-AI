/// Strips single-line (`//`), multi-line (`/* */`) comments, and trailing commas from JSONC.
/// Preserves comment markers that appear inside string literals (e.g. "http://...").
pub fn clean_jsonc(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    let mut in_string = false;
    let mut escape_next = false;

    while i < len {
        let c = chars[i];

        if in_string {
            output.push(c);
            if escape_next {
                escape_next = false;
            } else if c == '\\' {
                escape_next = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if c == '"' {
            in_string = true;
            output.push(c);
            i += 1;
            continue;
        }

        // Single-line comment: //
        if c == '/' && i + 1 < len && chars[i + 1] == '/' {
            i += 2;
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Multi-line comment: /* ... */
        if c == '/' && i + 1 < len && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i = (i + 2).min(len);
            continue;
        }

        output.push(c);
        i += 1;
    }

    strip_trailing_commas(&output)
}

fn strip_trailing_commas(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut in_string = false;
    let mut escape = false;

    while i < len {
        let c = chars[i];

        if in_string {
            result.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if c == '"' {
            in_string = true;
            result.push(c);
            i += 1;
            continue;
        }

        if c == ',' {
            // Peek forward past whitespace to see if next non-whitespace is '}' or ']'
            let mut j = i + 1;
            while j < len && chars[j].is_whitespace() {
                j += 1;
            }
            if j < len && (chars[j] == '}' || chars[j] == ']') {
                // Skip the trailing comma
                i += 1;
                continue;
            }
        }

        result.push(c);
        i += 1;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_jsonc_comments() {
        let jsonc = r#"{
            // This is a comment
            "key": "value", /* inline comment */
            "url": "http://127.0.0.1:3425/v1" // url with // must survive
        }"#;
        let cleaned = clean_jsonc(jsonc);
        let parsed: serde_json::Value = serde_json::from_str(&cleaned).expect("should parse");
        assert_eq!(parsed["key"], "value");
        assert_eq!(parsed["url"], "http://127.0.0.1:3425/v1");
    }

    #[test]
    fn test_strip_trailing_commas() {
        let jsonc = r#"{
            "items": [1, 2, 3,],
            "obj": { "a": 1, },
        }"#;
        let cleaned = clean_jsonc(jsonc);
        let parsed: serde_json::Value = serde_json::from_str(&cleaned).expect("should parse");
        assert_eq!(parsed["items"][2], 3);
        assert_eq!(parsed["obj"]["a"], 1);
    }
}
