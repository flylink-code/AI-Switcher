//! Gemini often emits KaTeX/LaTeX in visible answers (`$10\ \mu\text{s}$`).
//! Claude Code does not render that math, so the raw source shows up as
//! garbled output. Unwrap common spans into plain text / unicode.

/// Convert Gemini-visible LaTeX into text Claude Code can display.
pub fn unwrap_gemini_latex(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    let display = replace_delimited(input, "$$", "$$");
    let paren = replace_delimited(&display, "\\(", "\\)");
    let bracket = replace_delimited(&paren, "\\[", "\\]");
    replace_dollar_math(&bracket)
}

/// Split so an incomplete `$...` / `\(` / `\[` span stays in `hold`.
pub fn split_safe_latex_prefix(input: &str) -> (String, String) {
    const MAX_HOLD: usize = 160;
    let Some(idx) = incomplete_math_start(input) else {
        return (input.to_string(), String::new());
    };
    if input.len().saturating_sub(idx) > MAX_HOLD {
        return (input.to_string(), String::new());
    }
    (input[..idx].to_string(), input[idx..].to_string())
}

fn incomplete_math_start(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut dollar: Option<usize> = None;
    let mut paren: Option<usize> = None;
    let mut bracket: Option<usize> = None;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'$' {
                // `$$` display: toggle using the same dollar slot.
                match dollar {
                    Some(_) => dollar = None,
                    None => dollar = Some(i),
                }
                i += 2;
                continue;
            }
            match dollar {
                Some(_) => dollar = None,
                None => dollar = Some(i),
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'(' => {
                    if paren.is_none() {
                        paren = Some(i);
                    }
                    i += 2;
                    continue;
                }
                b')' => {
                    paren = None;
                    i += 2;
                    continue;
                }
                b'[' => {
                    if bracket.is_none() {
                        bracket = Some(i);
                    }
                    i += 2;
                    continue;
                }
                b']' => {
                    bracket = None;
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        i += 1;
    }
    [dollar, paren, bracket].into_iter().flatten().min()
}

fn replace_delimited(input: &str, open: &str, close: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find(open) {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + open.len()..];
        if let Some(end) = after_open.find(close) {
            let inner = &after_open[..end];
            if looks_like_latex(inner) {
                out.push_str(&decode_latex_inner(inner));
            } else {
                out.push_str(open);
                out.push_str(inner);
                out.push_str(close);
            }
            rest = &after_open[end + close.len()..];
        } else {
            out.push_str(&rest[start..]);
            return out;
        }
    }
    out.push_str(rest);
    out
}

fn replace_dollar_math(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && (i + 1 >= bytes.len() || bytes[i + 1] != b'$') {
            if let Some(end) = find_closing_dollar(bytes, i + 1) {
                let inner = &input[i + 1..end];
                if looks_like_latex(inner) {
                    out.push_str(&decode_latex_inner(inner));
                    i = end + 1;
                    continue;
                }
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn find_closing_dollar(bytes: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == b'$' && (i + 1 >= bytes.len() || bytes[i + 1] != b'$') {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn looks_like_latex(inner: &str) -> bool {
    let trimmed = inner.trim();
    if trimmed.is_empty() || trimmed.len() > 160 {
        return false;
    }
    trimmed.contains('\\')
        || trimmed.contains("_{")
        || trimmed.contains("^{")
        || (trimmed.contains('^') && trimmed.chars().any(|c| c.is_ascii_alphabetic()))
}

fn decode_latex_inner(inner: &str) -> String {
    let mut s = replace_cmd_braces(inner, "text");
    s = replace_cmd_braces(&s, "mathrm");
    s = replace_cmd_braces(&s, "mathbf");
    s = replace_cmd_braces(&s, "textrm");
    s = replace_cmd_braces(&s, "mathit");
    for (from, to) in LATEX_SYMBOLS {
        s = s.replace(from, to);
    }
    s = replace_subsup(&s, '_');
    s = replace_subsup(&s, '^');
    s = s.replace("\\ ", " ");
    s = s.replace("\\{", "{");
    s = s.replace("\\}", "}");
    s = s.replace("\\%", "%");
    s = s.replace("\\#", "#");
    s = s.replace("\\&", "&");
    s = s.replace("\\$", "$");
    s = collapse_spaces(&s);
    s
}

const LATEX_SYMBOLS: &[(&str, &str)] = &[
    ("\\rightarrow", "→"),
    ("\\Rightarrow", "⇒"),
    ("\\leftarrow", "←"),
    ("\\times", "×"),
    ("\\cdot", "·"),
    ("\\approx", "≈"),
    ("\\leq", "≤"),
    ("\\geq", "≥"),
    ("\\neq", "≠"),
    ("\\infty", "∞"),
    ("\\circ", "°"),
    ("\\degree", "°"),
    ("\\sim", "~"),
    ("\\to", "→"),
    ("\\mu", "μ"),
    ("\\alpha", "α"),
    ("\\beta", "β"),
    ("\\gamma", "γ"),
    ("\\Delta", "Δ"),
    ("\\Omega", "Ω"),
    ("\\omega", "ω"),
    ("\\pi", "π"),
    ("\\tau", "τ"),
    ("\\,", " "),
    ("\\;", " "),
    ("\\:", " "),
    ("\\!", ""),
];

fn replace_cmd_braces(input: &str, cmd: &str) -> String {
    let needle = format!("\\{cmd}{{");
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find(&needle) {
        out.push_str(&rest[..start]);
        let after = &rest[start + needle.len()..];
        if let Some(end) = after.find('}') {
            out.push_str(&after[..end]);
            rest = &after[end + 1..];
        } else {
            out.push_str(&rest[start..]);
            return out;
        }
    }
    out.push_str(rest);
    out
}

fn replace_subsup(input: &str, marker: char) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == marker {
            if chars.peek() == Some(&'{') {
                chars.next();
                let mut inner = String::new();
                for next in chars.by_ref() {
                    if next == '}' {
                        break;
                    }
                    inner.push(next);
                }
                out.push(marker);
                out.push_str(&inner);
                continue;
            }
        }
        out.push(ch);
    }
    out
}

fn collapse_spaces(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_space = false;
    for ch in input.chars() {
        if ch == ' ' {
            if prev_space {
                continue;
            }
            prev_space = true;
            out.push(' ');
        } else {
            prev_space = false;
            out.push(ch);
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwraps_session_timing_latex() {
        let raw = "下发 0x10 后加入 $10\\ \\mu\\text{s}$ 的 $t_{WB}$ 建立延时，并将超时时间放宽至 $500\\text{ ms}$。";
        let out = unwrap_gemini_latex(raw);
        assert_eq!(
            out,
            "下发 0x10 后加入 10 μs 的 t_WB 建立延时，并将超时时间放宽至 500 ms。"
        );
        assert_eq!(
            unwrap_gemini_latex("$3.5\\text{ ms} \\sim 10\\text{ ms}$"),
            "3.5 ms ~ 10 ms"
        );
        assert_eq!(unwrap_gemini_latex("$0\\text{xE0}$"), "0xE0");
        assert_eq!(unwrap_gemini_latex("标准 $0x05 \\rightarrow$ 地址"), "标准 0x05 → 地址");
    }

    #[test]
    fn leaves_plain_chinese_untouched() {
        let raw = "将超时时间放宽至 500 毫秒，并在地址 0x10 后加入建立延时。";
        assert_eq!(unwrap_gemini_latex(raw), raw);
    }

    #[test]
    fn leaves_shell_and_currency_alone() {
        assert_eq!(unwrap_gemini_latex("export $HOME"), "export $HOME");
        assert_eq!(unwrap_gemini_latex("costs $10"), "costs $10");
    }

    #[test]
    fn split_holds_incomplete_dollar_span() {
        let (emit, hold) = split_safe_latex_prefix("延时 $10\\ \\mu");
        assert_eq!(emit, "延时 ");
        assert_eq!(hold, "$10\\ \\mu");
        let (emit, hold) = split_safe_latex_prefix("Hello");
        assert_eq!(emit, "Hello");
        assert!(hold.is_empty());
    }
}
