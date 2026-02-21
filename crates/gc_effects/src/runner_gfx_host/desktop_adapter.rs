use std::cell::RefCell;
use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey};
use minifb::{Key, KeyRepeat, MouseMode, Window, WindowOptions};

pub(crate) const DESKTOP_ADAPTER_NAME: &str = "desktop-host";

thread_local! {
    static DESKTOP_SURFACES: RefCell<BTreeMap<String, DesktopSurface>> = const { RefCell::new(BTreeMap::new()) };
}

struct DesktopSurface {
    window: Window,
    last_mouse: Option<(i64, i64)>,
}

pub(crate) fn create_surface(surface: &str, width: i64, height: i64, title: &str) -> bool {
    let width = width.max(1).min(i64::from(i32::MAX)) as usize;
    let height = height.max(1).min(i64::from(i32::MAX)) as usize;
    let Ok(window) = Window::new(title, width, height, WindowOptions::default()) else {
        return false;
    };
    DESKTOP_SURFACES.with(|registry| {
        let mut registry = registry.borrow_mut();
        registry.insert(
            surface.to_string(),
            DesktopSurface {
                window,
                last_mouse: None,
            },
        );
    });
    true
}

pub(crate) fn query_surface_size(surface: &str) -> Option<(i64, i64)> {
    DESKTOP_SURFACES.with(|registry| {
        let mut registry = registry.borrow_mut();
        let surface = registry.get_mut(surface)?;
        if !surface.window.is_open() {
            return None;
        }
        let (w, h) = surface.window.get_size();
        Some((w as i64, h as i64))
    })
}

pub(crate) fn apply_title(surface: &str, title: &str) -> bool {
    DESKTOP_SURFACES.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(surface) = registry.get_mut(surface) else {
            return false;
        };
        if !surface.window.is_open() {
            return false;
        }
        surface.window.set_title(title);
        true
    })
}

pub(crate) fn apply_cursor_mode(surface: &str, mode: &str) -> bool {
    DESKTOP_SURFACES.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(surface) = registry.get_mut(surface) else {
            return false;
        };
        if !surface.window.is_open() {
            return false;
        }
        let visible = !matches!(mode, "hidden" | "grabbed" | "confined");
        surface.window.set_cursor_visibility(visible);
        true
    })
}

pub(crate) fn request_redraw(surface: &str) -> bool {
    DESKTOP_SURFACES.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(surface) = registry.get_mut(surface) else {
            return false;
        };
        if !surface.window.is_open() {
            return false;
        }
        surface.window.update();
        true
    })
}

pub(crate) fn poll_events(surface: &str, seq: &mut u64, max_events: usize) -> Vec<Term> {
    DESKTOP_SURFACES.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(surface_state) = registry.get_mut(surface) else {
            return Vec::new();
        };
        if !surface_state.window.is_open() {
            return Vec::new();
        }

        surface_state.window.update();
        let mut out = Vec::new();

        for key in surface_state.window.get_keys_pressed(KeyRepeat::No) {
            if out.len() >= max_events {
                break;
            }
            *seq = seq.saturating_add(1);
            out.push(map_term(vec![
                (":kind", Term::symbol(":key")),
                (":surface", Term::Str(surface.to_string())),
                (":seq", Term::Int((*seq as i64).into())),
                (":code", Term::Str(key_code_string(key))),
                (":state", Term::symbol(":press")),
                (":modifiers", key_modifiers(&surface_state.window)),
            ]));
        }

        if out.len() < max_events
            && let Some((mx, my)) = surface_state
                .window
                .get_mouse_pos(MouseMode::Discard)
                .map(|(x, y)| (x as i64, y as i64))
        {
            let moved = surface_state
                .last_mouse
                .map(|(lx, ly)| lx != mx || ly != my)
                .unwrap_or(true);
            if moved {
                surface_state.last_mouse = Some((mx, my));
                *seq = seq.saturating_add(1);
                out.push(map_term(vec![
                    (":kind", Term::symbol(":mouse")),
                    (":action", Term::symbol(":move")),
                    (":surface", Term::Str(surface.to_string())),
                    (":seq", Term::Int((*seq as i64).into())),
                    (":column", Term::Int(mx.into())),
                    (":row", Term::Int(my.into())),
                    (":modifiers", key_modifiers(&surface_state.window)),
                ]));
            }
        }

        out.truncate(max_events);
        out
    })
}

fn key_modifiers(window: &Window) -> Term {
    let mut mods = Vec::new();
    if window.is_key_down(Key::LeftShift) || window.is_key_down(Key::RightShift) {
        mods.push(Term::symbol(":shift"));
    }
    if window.is_key_down(Key::LeftCtrl) || window.is_key_down(Key::RightCtrl) {
        mods.push(Term::symbol(":control"));
    }
    if window.is_key_down(Key::LeftAlt) || window.is_key_down(Key::RightAlt) {
        mods.push(Term::symbol(":alt"));
    }
    Term::Vector(mods)
}

fn key_code_string(key: Key) -> String {
    format!("{key:?}").to_ascii_lowercase()
}

fn map_term(items: Vec<(&str, Term)>) -> Term {
    let mut map = BTreeMap::new();
    for (k, v) in items {
        map.insert(TermOrdKey(Term::symbol(k)), v);
    }
    Term::Map(map)
}
