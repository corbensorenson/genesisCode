use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::io::Write;
use std::time::Duration;

use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use crossterm::execute;
use crossterm::terminal;
use gc_coreform::{Term, TermOrdKey};

pub(crate) const TERMINAL_ADAPTER_NAME: &str = "terminal-host";

pub(crate) fn query_surface_size() -> Option<(i64, i64)> {
    terminal::size()
        .ok()
        .map(|(w, h)| (i64::from(w.max(1)), i64::from(h.max(1))))
}

pub(crate) fn apply_title(title: &str) -> bool {
    let mut out = std::io::stderr();
    if !out.is_terminal() {
        return false;
    }
    execute!(out, terminal::SetTitle(title)).is_ok()
}

pub(crate) fn apply_cursor_mode(mode: &str) -> bool {
    let mut out = std::io::stderr();
    if !out.is_terminal() {
        return false;
    }
    match mode {
        "hidden" | "grabbed" | "confined" => execute!(out, cursor::Hide).is_ok(),
        "normal" | "visible" => execute!(out, cursor::Show).is_ok(),
        _ => false,
    }
}

pub(crate) fn enqueue_audio_bell() -> bool {
    let mut out = std::io::stderr();
    if !out.is_terminal() {
        return false;
    }
    out.write_all(b"\x07").and_then(|_| out.flush()).is_ok()
}

pub(crate) fn poll_events(surface: &str, seq: &mut u64, max_events: usize) -> Vec<Term> {
    let mut out = Vec::new();
    for _ in 0..max_events {
        let Ok(ready) = event::poll(Duration::from_millis(0)) else {
            break;
        };
        if !ready {
            break;
        }
        let Ok(ev) = event::read() else {
            break;
        };
        *seq = seq.saturating_add(1);
        out.push(event_to_term(surface, *seq, ev));
    }
    out
}

fn event_to_term(surface: &str, seq: u64, ev: Event) -> Term {
    let base = vec![
        (":surface", Term::Str(surface.to_string())),
        (":seq", Term::Int((seq as i64).into())),
    ];
    match ev {
        Event::Key(key) => map_term(
            [base, key_event_fields(key)]
                .into_iter()
                .flatten()
                .collect(),
        ),
        Event::Mouse(mouse) => map_term(
            [base, mouse_event_fields(mouse)]
                .into_iter()
                .flatten()
                .collect(),
        ),
        Event::Resize(width, height) => map_term(vec![
            (":kind", Term::symbol(":resize")),
            (":surface", Term::Str(surface.to_string())),
            (":seq", Term::Int((seq as i64).into())),
            (":width", Term::Int((i64::from(width)).into())),
            (":height", Term::Int((i64::from(height)).into())),
        ]),
        Event::FocusGained => map_term(vec![
            (":kind", Term::symbol(":focus-gained")),
            (":surface", Term::Str(surface.to_string())),
            (":seq", Term::Int((seq as i64).into())),
        ]),
        Event::FocusLost => map_term(vec![
            (":kind", Term::symbol(":focus-lost")),
            (":surface", Term::Str(surface.to_string())),
            (":seq", Term::Int((seq as i64).into())),
        ]),
        Event::Paste(text) => map_term(vec![
            (":kind", Term::symbol(":paste")),
            (":surface", Term::Str(surface.to_string())),
            (":seq", Term::Int((seq as i64).into())),
            (":text", Term::Str(text)),
        ]),
    }
}

fn key_event_fields(key: KeyEvent) -> Vec<(&'static str, Term)> {
    vec![
        (":kind", Term::symbol(":key")),
        (":code", Term::Str(key_code_string(key.code))),
        (":state", Term::symbol(key_kind_symbol(key.kind))),
        (
            ":modifiers",
            Term::Vector(
                key_modifiers(key.modifiers)
                    .into_iter()
                    .map(Term::symbol)
                    .collect(),
            ),
        ),
    ]
}

fn mouse_event_fields(mouse: MouseEvent) -> Vec<(&'static str, Term)> {
    vec![
        (":kind", Term::symbol(":mouse")),
        (":action", Term::symbol(mouse_kind_symbol(mouse.kind))),
        (
            ":modifiers",
            Term::Vector(
                key_modifiers(mouse.modifiers)
                    .into_iter()
                    .map(Term::symbol)
                    .collect(),
            ),
        ),
        (":column", Term::Int((i64::from(mouse.column)).into())),
        (":row", Term::Int((i64::from(mouse.row)).into())),
    ]
}

fn key_kind_symbol(kind: event::KeyEventKind) -> &'static str {
    match kind {
        event::KeyEventKind::Press => ":press",
        event::KeyEventKind::Repeat => ":repeat",
        event::KeyEventKind::Release => ":release",
    }
}

fn mouse_kind_symbol(kind: MouseEventKind) -> &'static str {
    match kind {
        MouseEventKind::Down(_) => ":down",
        MouseEventKind::Up(_) => ":up",
        MouseEventKind::Drag(_) => ":drag",
        MouseEventKind::Moved => ":move",
        MouseEventKind::ScrollDown => ":scroll-down",
        MouseEventKind::ScrollUp => ":scroll-up",
        MouseEventKind::ScrollLeft => ":scroll-left",
        MouseEventKind::ScrollRight => ":scroll-right",
    }
}

fn key_modifiers(modifiers: KeyModifiers) -> Vec<&'static str> {
    let mut out = Vec::new();
    if modifiers.contains(KeyModifiers::SHIFT) {
        out.push(":shift");
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        out.push(":control");
    }
    if modifiers.contains(KeyModifiers::ALT) {
        out.push(":alt");
    }
    if modifiers.contains(KeyModifiers::SUPER) {
        out.push(":super");
    }
    out
}

fn key_code_string(code: KeyCode) -> String {
    match code {
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "page-up".to_string(),
        KeyCode::PageDown => "page-down".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::BackTab => "back-tab".to_string(),
        KeyCode::Delete => "delete".to_string(),
        KeyCode::Insert => "insert".to_string(),
        KeyCode::F(n) => format!("f{n}"),
        KeyCode::Char(c) => format!("char:{c}"),
        KeyCode::Null => "null".to_string(),
        KeyCode::Esc => "escape".to_string(),
        _ => format!("{code:?}"),
    }
}

fn map_term(items: Vec<(&str, Term)>) -> Term {
    let mut map = BTreeMap::new();
    for (k, v) in items {
        map.insert(TermOrdKey(Term::symbol(k)), v);
    }
    Term::Map(map)
}
