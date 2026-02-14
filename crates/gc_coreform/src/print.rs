use std::collections::BTreeMap;

use crate::term::{Term, TermOrdKey};

const INDENT: usize = 2;
const MAX_WIDTH: usize = 100;

pub fn print_term(term: &Term) -> String {
    fmt_term(term, 0).join("\n")
}

pub fn print_module(forms: &[Term]) -> String {
    let mut out = String::new();
    for (i, f) in forms.iter().enumerate() {
        if i != 0 {
            out.push('\n');
        }
        out.push_str(&print_term(f));
        out.push('\n');
    }
    out
}

fn spaces(n: usize) -> String {
    " ".repeat(n)
}

fn is_atom(t: &Term) -> bool {
    matches!(
        t,
        Term::Nil | Term::Bool(_) | Term::Int(_) | Term::Str(_) | Term::Bytes(_) | Term::Symbol(_)
    )
}

fn atom_repr(t: &Term) -> Option<String> {
    match t {
        Term::Nil => Some("nil".to_string()),
        Term::Bool(true) => Some("true".to_string()),
        Term::Bool(false) => Some("false".to_string()),
        Term::Int(i) => Some(i.to_string()),
        Term::Str(s) => Some(format!("\"{}\"", escape_str(s))),
        Term::Bytes(b) => Some(format!("b\"{}\"", escape_bytes(b))),
        Term::Symbol(s) => Some(s.clone()),
        _ => None,
    }
}

fn single_line(t: &Term) -> Option<String> {
    if let Some(a) = atom_repr(t) {
        return Some(a);
    }

    match t {
        Term::Vector(xs) => {
            if xs.len() <= 3 && xs.iter().all(is_atom) {
                let mut s = String::from("[");
                for (i, x) in xs.iter().enumerate() {
                    if i != 0 {
                        s.push(' ');
                    }
                    s.push_str(&atom_repr(x)?);
                }
                s.push(']');
                Some(s)
            } else {
                None
            }
        }
        Term::Map(m) => {
            if m.is_empty() {
                return Some("{}".to_string());
            }
            if m.len() <= 2 && m.iter().all(|(k, v)| is_atom(&k.0) && is_atom(v)) {
                let mut s = String::from("{");
                for (i, (k, v)) in m.iter().enumerate() {
                    if i != 0 {
                        s.push(' ');
                    }
                    s.push_str(&atom_repr(&k.0)?);
                    s.push(' ');
                    s.push_str(&atom_repr(v)?);
                }
                s.push('}');
                Some(s)
            } else {
                None
            }
        }
        Term::Pair(_, _) => {
            let items = t.as_proper_list()?;
            let parts: Option<Vec<String>> = items.iter().map(|x| single_line(x)).collect();
            parts.and_then(|ps| {
                let mut s = String::from("(");
                for (i, p) in ps.iter().enumerate() {
                    if i != 0 {
                        s.push(' ');
                    }
                    s.push_str(p);
                }
                s.push(')');
                if s.len() <= MAX_WIDTH { Some(s) } else { None }
            })
        }
        _ => None,
    }
}

fn fmt_term(t: &Term, indent: usize) -> Vec<String> {
    if let Some(s) = single_line(t)
        && indent + s.len() <= MAX_WIDTH
    {
        return vec![format!("{}{}", spaces(indent), s)];
    }
    if let Some(a) = atom_repr(t) {
        return vec![format!("{}{}", spaces(indent), a)];
    }

    match t {
        Term::Vector(xs) => fmt_vector(xs, indent),
        Term::Map(m) => fmt_map(m, indent),
        Term::Pair(_, _) => fmt_list(t, indent),
        _ => vec![format!(
            "{}{}",
            spaces(indent),
            atom_repr(t).unwrap_or_else(|| "<unknown>".to_string())
        )],
    }
}

fn fmt_list(t: &Term, indent: usize) -> Vec<String> {
    let Some(items) = t.as_proper_list() else {
        // Dotted pairs aren't expected in v0.2 surface; print a readable debug-ish form.
        return vec![format!("{}(pair <improper>)", spaces(indent))];
    };

    // If it fits, single-line it.
    let maybe = {
        let parts: Option<Vec<String>> = items.iter().map(|x| single_line(x)).collect();
        parts.and_then(|ps| {
            let mut s = String::from("(");
            for (i, p) in ps.iter().enumerate() {
                if i != 0 {
                    s.push(' ');
                }
                s.push_str(p);
            }
            s.push(')');
            if indent + s.len() <= MAX_WIDTH {
                Some(s)
            } else {
                None
            }
        })
    };
    if let Some(s) = maybe {
        return vec![format!("{}{}", spaces(indent), s)];
    }

    let mut out: Vec<String> = Vec::new();
    if items.is_empty() {
        out.push(format!("{}()", spaces(indent)));
        return out;
    }

    // Multi-line: if the head can't be single-lined, put the opening paren on its own line
    // and render all elements one-per-line. This avoids emitting placeholders like "<form>".
    let Some(head0) = single_line(items[0]) else {
        out.push(format!("{}(", spaces(indent)));
        for (idx, item) in items.iter().enumerate() {
            let mut lines = fmt_term(item, indent + INDENT);
            // Ensure the element's first line starts at indent+INDENT even if it was single-line.
            if !lines.is_empty() && !lines[0].starts_with(&spaces(indent + INDENT)) {
                lines[0] = format!("{}{}", spaces(indent + INDENT), lines[0].trim_start());
            }
            out.append(&mut lines);
            if idx + 1 == items.len() {
                out.last_mut().unwrap().push(')');
            }
        }
        return out;
    };

    // Otherwise, keep 1-2 leading forms on the first line if they are single-line and fit.
    let mut head_count = 1usize;
    let mut first = format!("{}({}", spaces(indent), head0);

    if items.len() >= 2
        && let Some(h1) = single_line(items[1])
        && first.len() + 1 + h1.len() <= MAX_WIDTH
    {
        first.push(' ');
        first.push_str(&h1);
        head_count = 2;
    }
    out.push(first);

    if items.len() == head_count {
        // Close paren on the only line.
        let last = out.last_mut().unwrap();
        last.push(')');
        return out;
    }

    for (idx, item) in items.iter().enumerate().skip(head_count) {
        let mut lines = fmt_term(item, indent + INDENT);
        // Ensure the element's first line starts at indent+INDENT even if it was single-line.
        if !lines.is_empty() && !lines[0].starts_with(&spaces(indent + INDENT)) {
            lines[0] = format!("{}{}", spaces(indent + INDENT), lines[0].trim_start());
        }
        out.append(&mut lines);

        if idx + 1 == items.len() {
            // Close list on last line.
            out.last_mut().unwrap().push(')');
        }
    }

    out
}

fn fmt_vector(xs: &[Term], indent: usize) -> Vec<String> {
    if xs.is_empty() {
        return vec![format!("{}[]", spaces(indent))];
    }

    let mut out = Vec::new();
    out.push(format!("{}[", spaces(indent)));
    for (i, x) in xs.iter().enumerate() {
        let mut lines = fmt_term(x, indent + INDENT);
        out.append(&mut lines);
        if i + 1 == xs.len() {
            out.last_mut().unwrap().push(']');
        }
    }
    out
}

fn fmt_map(m: &BTreeMap<TermOrdKey, Term>, indent: usize) -> Vec<String> {
    if m.is_empty() {
        return vec![format!("{}{{}}", spaces(indent))];
    }

    let mut out = Vec::new();
    out.push(format!("{}{{", spaces(indent)));
    let pairs: Vec<(&Term, &Term)> = m.iter().map(|(k, v)| (&k.0, v)).collect();
    for (i, (k, v)) in pairs.iter().enumerate() {
        let ks = single_line(k);
        let vs = single_line(v);
        if let (Some(ks), Some(vs)) = (ks, vs) {
            let line = format!("{}{} {}", spaces(indent + INDENT), ks, vs);
            if line.len() <= MAX_WIDTH {
                out.push(line);
            } else {
                out.extend(fmt_term(k, indent + INDENT));
                out.extend(fmt_term(v, indent + INDENT * 2));
            }
        } else {
            out.extend(fmt_term(k, indent + INDENT));
            out.extend(fmt_term(v, indent + INDENT * 2));
        }

        if i + 1 == pairs.len() {
            out.last_mut().unwrap().push('}');
        }
    }
    out
}

fn escape_str(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn escape_bytes(b: &[u8]) -> String {
    let mut out = String::new();
    for &x in b {
        match x {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7E => out.push(x as char),
            _ => out.push_str(&format!("\\x{:02X}", x)),
        }
    }
    out
}
