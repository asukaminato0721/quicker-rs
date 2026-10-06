//! C# regular and verbatim string literals. JSON strings have different escapes.

pub(super) fn read(input: &str) -> Result<(String, usize), String> {
    let verbatim = input.starts_with("@\"");
    let start = if verbatim { 2 } else { 1 };
    let mut at = start;
    let mut units = Vec::new();
    while at < input.len() {
        let c = input[at..].chars().next().unwrap();
        at += c.len_utf8();
        if c == '"' {
            if verbatim && input[at..].starts_with('"') {
                at += 1;
                units.push(u16::from(b'"'));
                continue;
            }
            let value = String::from_utf16(&units)
                .map_err(|_| "String literal contains an unpaired UTF-16 surrogate")?;
            return Ok((value, at));
        }
        if !verbatim && matches!(c, '\r' | '\n' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            return Err("A regular string literal cannot contain a line break".into());
        }
        if !verbatim && c == '\\' {
            let escape = input[at..]
                .chars()
                .next()
                .ok_or("Incomplete string escape")?;
            at += escape.len_utf8();
            let escaped = match escape {
                '\'' => '\'',
                '"' => '"',
                '\\' => '\\',
                '0' => '\0',
                'a' => '\u{7}',
                'b' => '\u{8}',
                'f' => '\u{c}',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'v' => '\u{b}',
                'u' | 'U' | 'x' => {
                    let maximum = if escape == 'U' { 8 } else { 4 };
                    let count = input[at..]
                        .bytes()
                        .take(maximum)
                        .take_while(u8::is_ascii_hexdigit)
                        .count();
                    if count == 0 || (escape != 'x' && count != maximum) {
                        return Err("Invalid hexadecimal string escape".into());
                    }
                    let number = u32::from_str_radix(&input[at..at + count], 16).unwrap();
                    at += count;
                    if number <= 0xffff {
                        units.push(number as u16);
                    } else {
                        let c =
                            char::from_u32(number).ok_or("String escape exceeds Unicode range")?;
                        units.extend_from_slice(c.encode_utf16(&mut [0; 2]));
                    }
                    continue;
                }
                _ => return Err(format!("Unsupported string escape: \\{escape}")),
            };
            units.push(escaped as u16);
        } else {
            units.extend_from_slice(c.encode_utf16(&mut [0; 2]));
        }
    }
    Err("Unclosed string literal".into())
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use serde_json::json;

    #[test]
    fn csharp_strings_keep_backslashes_quotes_braces_and_line_breaks() {
        for (expr, expected) in [
            (r#"$= @"\""#, "\\"),
            (r#"$= @"C:\new\test.txt""#, "C:\\new\\test.txt"),
            (r#"$= @"a""b{missing}""#, "a\"b{missing}"),
            ("$= @\"a\r\n中\"", "a\r\n中"),
            (
                r#"$= "\0\a\b\f\n\r\t\v\'\"\\""#,
                "\0\u{7}\u{8}\u{c}\n\r\t\u{b}'\"\\",
            ),
            (r#"$= "\x4e2d\u6587\U0001F600""#, "中文😀"),
            (r#"$= "\ud83d\ude00""#, "😀"),
            (r#"$= "\x1" + "23""#, "\u{1}23"),
            (r#"$= "\x12345""#, "\u{1234}5"),
            (r#"$= "a" + @"\n" + "b""#, "a\\nb"),
            (r#"$= @"".Length == 0 ? "yes" : "no""#, "yes"),
        ] {
            assert_eq!(
                evaluate(expr, &HashMap::new()).unwrap(),
                json!(expected),
                "{expr}"
            );
        }
    }

    #[test]
    fn malformed_or_unsupported_literals_fail_without_reinterpretation() {
        for expr in [
            "$= @\"",
            "$= @\"a\"\"",
            "$= @ \"a\"",
            "$= $\"{x}\"",
            "$= 'x'",
            r#"$= "\q""#,
            r#"$= "\/""#,
            r#"$= "\u12""#,
            r#"$= "\U00110000""#,
            r#"$= "\x""#,
            r#"$= "\ud800""#,
            r#"$= "\udc00""#,
            r#"$= "\UFFFFFFFF""#,
            "$= \"a\nb\"",
            "$= \"a\u{2028}b\"",
            r#"$= @"a" @"b""#,
        ] {
            assert!(evaluate(expr, &HashMap::new()).is_err(), "{expr}");
        }
    }
}
