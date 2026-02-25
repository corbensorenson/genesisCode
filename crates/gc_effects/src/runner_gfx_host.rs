use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{SealId, Value};
use num_traits::ToPrimitive;

use crate::policy::OpPolicy;
use crate::runner_host_bridge::{BridgeError, call_host_bridge};

#[cfg(all(not(target_os = "wasi"), feature = "gfx-desktop-backend"))]
mod desktop_adapter;
#[path = "runner_gfx_host/helpers.rs"]
mod helpers;
#[cfg(not(target_os = "wasi"))]
mod terminal_adapter;
use helpers::*;

const FIRST_PARTY_BACKEND: &str = "first-party-runtime";
const HEADLESS_ADAPTER: &str = "headless-sim";
#[cfg(target_os = "wasi")]
const NOOP_ADAPTER: &str = "noop";

#[derive(Debug, Clone)]
struct SurfaceState {
    width: i64,
    height: i64,
    title: String,
    backend: String,
    adapter: String,
    cursor_mode: String,
    pending_redraws: u64,
    poll_seq: u64,
}

impl SurfaceState {
    fn new(width: i64, height: i64, title: String, backend: String, adapter: String) -> Self {
        Self {
            width,
            height,
            title,
            backend,
            adapter,
            cursor_mode: "normal".to_string(),
            pending_redraws: 0,
            poll_seq: 0,
        }
    }
}

#[derive(Debug, Clone)]
enum GfxFirstPartyProfile {
    Headless,
    Interactive,
    Desktop,
    Browser,
}

impl GfxFirstPartyProfile {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Headless => "headless",
            Self::Interactive => "interactive",
            Self::Desktop => "desktop",
            Self::Browser => "browser",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GfxHostRuntime {
    next_surface: u64,
    surfaces: BTreeMap<String, SurfaceState>,
    audio_queued: u64,
    master_gain: i64,
}

pub(crate) fn gfx_host_call(
    runtime: &mut GfxHostRuntime,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Option<Value> {
    if !is_gfx_host_op(op) {
        return None;
    }
    if !has_explicit_bridge_profile(pol) {
        return Some(Value::Data(first_party_gfx_response(
            runtime, op, payload, pol,
        )));
    }
    Some(match call_host_bridge("gfx", op, payload, pol) {
        Ok(resp) => Value::Data(resp),
        Err(err) => mk_error(error_tok, &err, Some(op)),
    })
}

fn has_explicit_bridge_profile(pol: Option<&OpPolicy>) -> bool {
    let Some(pol) = pol else {
        return false;
    };
    let has_nonempty_str = |key: &str| {
        pol.extra
            .get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty())
    };
    has_nonempty_str("bridge_cmd")
        || has_nonempty_str("wasi_bridge_response")
        || has_nonempty_str("wasi_bridge_response_file")
        || pol
            .extra
            .get("wasi_bridge_profile")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
}

fn first_party_profile(pol: Option<&OpPolicy>) -> GfxFirstPartyProfile {
    let profile = pol
        .and_then(|p| {
            p.extra
                .get("first_party_profile")
                .or_else(|| p.extra.get("gfx_first_party_profile"))
        })
        .and_then(|v| v.as_str())
        .unwrap_or("headless")
        .to_ascii_lowercase();
    match profile.as_str() {
        "interactive" => GfxFirstPartyProfile::Interactive,
        "desktop" => GfxFirstPartyProfile::Desktop,
        "browser" => GfxFirstPartyProfile::Browser,
        _ => GfxFirstPartyProfile::Headless,
    }
}

fn first_party_gfx_response(
    runtime: &mut GfxHostRuntime,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
) -> Term {
    let profile = first_party_profile(pol);
    match op {
        "gfx/window::create-surface" => first_party_create_surface(runtime, payload, &profile),
        "gfx/window::resize-surface" => first_party_resize_surface(runtime, payload),
        "gfx/window::set-title" => first_party_set_title(runtime, payload),
        "gfx/window::request-redraw" => first_party_request_redraw(runtime, payload),
        "gfx/window::surface-info" => first_party_surface_info(runtime, payload),
        "gfx/input::poll-events" => first_party_poll_events(runtime, payload, &profile),
        "gfx/input::set-cursor-mode" => first_party_set_cursor_mode(runtime, payload),
        "gfx/audio::set-master" => first_party_set_master(runtime, payload, &profile),
        "gfx/audio::enqueue" => first_party_enqueue(runtime, payload, &profile),
        _ => map_term(vec![
            (":ok", Term::Bool(false)),
            (
                ":error/code",
                Term::Str("gfx/first-party-unsupported-op".to_string()),
            ),
            (":error/op", Term::symbol(op)),
        ]),
    }
}

fn backend_for_profile(profile: &GfxFirstPartyProfile) -> &'static str {
    match profile {
        GfxFirstPartyProfile::Headless => FIRST_PARTY_BACKEND,
        GfxFirstPartyProfile::Interactive => interactive_adapter_name(),
        GfxFirstPartyProfile::Desktop => FIRST_PARTY_BACKEND,
        GfxFirstPartyProfile::Browser => "browser-first-party-runtime",
    }
}

fn adapter_for_profile(profile: &GfxFirstPartyProfile) -> &'static str {
    match profile {
        GfxFirstPartyProfile::Headless => HEADLESS_ADAPTER,
        GfxFirstPartyProfile::Interactive => interactive_adapter_name(),
        GfxFirstPartyProfile::Desktop => desktop_adapter_name(),
        GfxFirstPartyProfile::Browser => "browser-host",
    }
}

#[cfg(not(target_os = "wasi"))]
fn interactive_adapter_name() -> &'static str {
    terminal_adapter::TERMINAL_ADAPTER_NAME
}

#[cfg(target_os = "wasi")]
fn interactive_adapter_name() -> &'static str {
    NOOP_ADAPTER
}

#[cfg(all(not(target_os = "wasi"), feature = "gfx-desktop-backend"))]
fn desktop_adapter_name() -> &'static str {
    desktop_adapter::DESKTOP_ADAPTER_NAME
}

#[cfg(any(target_os = "wasi", not(feature = "gfx-desktop-backend")))]
fn desktop_adapter_name() -> &'static str {
    "desktop-host"
}

#[cfg(not(target_os = "wasi"))]
fn interactive_query_surface_size() -> Option<(i64, i64)> {
    terminal_adapter::query_surface_size()
}

#[cfg(target_os = "wasi")]
fn interactive_query_surface_size() -> Option<(i64, i64)> {
    None
}

#[cfg(not(target_os = "wasi"))]
fn interactive_apply_title(title: &str) -> bool {
    terminal_adapter::apply_title(title)
}

#[cfg(target_os = "wasi")]
fn interactive_apply_title(_title: &str) -> bool {
    false
}

#[cfg(not(target_os = "wasi"))]
fn interactive_apply_cursor_mode(mode: &str) -> bool {
    terminal_adapter::apply_cursor_mode(mode)
}

#[cfg(target_os = "wasi")]
fn interactive_apply_cursor_mode(_mode: &str) -> bool {
    false
}

#[cfg(not(target_os = "wasi"))]
fn interactive_enqueue_audio_bell() -> bool {
    terminal_adapter::enqueue_audio_bell()
}

#[cfg(target_os = "wasi")]
fn interactive_enqueue_audio_bell() -> bool {
    false
}

#[cfg(not(target_os = "wasi"))]
fn interactive_poll_events(surface: &str, seq: &mut u64, max_events: usize) -> Vec<Term> {
    terminal_adapter::poll_events(surface, seq, max_events)
}

#[cfg(target_os = "wasi")]
fn interactive_poll_events(_surface: &str, _seq: &mut u64, _max_events: usize) -> Vec<Term> {
    Vec::new()
}

#[cfg(all(not(target_os = "wasi"), feature = "gfx-desktop-backend"))]
fn desktop_create_surface(surface: &str, width: i64, height: i64, title: &str) -> bool {
    desktop_adapter::create_surface(surface, width, height, title)
}

#[cfg(any(target_os = "wasi", not(feature = "gfx-desktop-backend")))]
fn desktop_create_surface(_surface: &str, _width: i64, _height: i64, _title: &str) -> bool {
    false
}

#[cfg(all(not(target_os = "wasi"), feature = "gfx-desktop-backend"))]
fn desktop_query_surface_size(surface: &str) -> Option<(i64, i64)> {
    desktop_adapter::query_surface_size(surface)
}

#[cfg(any(target_os = "wasi", not(feature = "gfx-desktop-backend")))]
fn desktop_query_surface_size(_surface: &str) -> Option<(i64, i64)> {
    None
}

#[cfg(all(not(target_os = "wasi"), feature = "gfx-desktop-backend"))]
fn desktop_apply_title(surface: &str, title: &str) -> bool {
    desktop_adapter::apply_title(surface, title)
}

#[cfg(any(target_os = "wasi", not(feature = "gfx-desktop-backend")))]
fn desktop_apply_title(_surface: &str, _title: &str) -> bool {
    false
}

#[cfg(all(not(target_os = "wasi"), feature = "gfx-desktop-backend"))]
fn desktop_apply_cursor_mode(surface: &str, mode: &str) -> bool {
    desktop_adapter::apply_cursor_mode(surface, mode)
}

#[cfg(any(target_os = "wasi", not(feature = "gfx-desktop-backend")))]
fn desktop_apply_cursor_mode(_surface: &str, _mode: &str) -> bool {
    false
}

#[cfg(all(not(target_os = "wasi"), feature = "gfx-desktop-backend"))]
fn desktop_request_redraw(surface: &str) -> bool {
    desktop_adapter::request_redraw(surface)
}

#[cfg(any(target_os = "wasi", not(feature = "gfx-desktop-backend")))]
fn desktop_request_redraw(_surface: &str) -> bool {
    false
}

#[cfg(all(not(target_os = "wasi"), feature = "gfx-desktop-backend"))]
fn desktop_poll_events(surface: &str, seq: &mut u64, max_events: usize) -> Vec<Term> {
    desktop_adapter::poll_events(surface, seq, max_events)
}

#[cfg(any(target_os = "wasi", not(feature = "gfx-desktop-backend")))]
fn desktop_poll_events(_surface: &str, _seq: &mut u64, _max_events: usize) -> Vec<Term> {
    Vec::new()
}

fn desktop_enqueue_audio_bell() -> bool {
    #[cfg(target_os = "macos")]
    {
        if std::process::Command::new("osascript")
            .arg("-e")
            .arg("beep")
            .status()
            .is_ok_and(|s| s.success())
        {
            return true;
        }
    }
    #[cfg(target_os = "windows")]
    {
        if std::process::Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg("[console]::beep(880,120)")
            .status()
            .is_ok_and(|s| s.success())
        {
            return true;
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for (cmd, args) in [
            ("canberra-gtk-play", vec!["-i", "bell"]),
            (
                "paplay",
                vec!["/usr/share/sounds/freedesktop/stereo/bell.oga"],
            ),
        ] {
            if std::process::Command::new(cmd)
                .args(args)
                .status()
                .is_ok_and(|s| s.success())
            {
                return true;
            }
        }
    }
    false
}

fn first_party_create_surface(
    runtime: &mut GfxHostRuntime,
    payload: &Term,
    profile: &GfxFirstPartyProfile,
) -> Term {
    let opts = payload_map(payload)
        .and_then(|m| m.get(&TermOrdKey(Term::symbol(":opts"))))
        .and_then(payload_map);
    let width = opts
        .and_then(|m| map_get_i64(m, ":width"))
        .unwrap_or(800_i64);
    let height = opts
        .and_then(|m| map_get_i64(m, ":height"))
        .unwrap_or(600_i64);
    let title = opts
        .and_then(|m| map_get_string(m, ":title"))
        .unwrap_or_else(|| "surface".to_string());
    runtime.next_surface = runtime.next_surface.saturating_add(1);
    let adapter = adapter_for_profile(profile).to_string();
    let sid = format!("surface-{}-{}", adapter, runtime.next_surface);
    let mut created = true;
    let (resolved_width, resolved_height) = match profile {
        GfxFirstPartyProfile::Interactive => {
            interactive_query_surface_size().unwrap_or((width, height))
        }
        GfxFirstPartyProfile::Desktop => {
            created = desktop_create_surface(&sid, width, height, &title);
            desktop_query_surface_size(&sid).unwrap_or((width, height))
        }
        GfxFirstPartyProfile::Browser => (width, height),
        GfxFirstPartyProfile::Headless => (width, height),
    };
    let backend = backend_for_profile(profile).to_string();
    let title_applied = match profile {
        GfxFirstPartyProfile::Interactive => interactive_apply_title(&title),
        GfxFirstPartyProfile::Desktop => desktop_apply_title(&sid, &title),
        GfxFirstPartyProfile::Browser => true,
        GfxFirstPartyProfile::Headless => true,
    };
    runtime.surfaces.insert(
        sid.clone(),
        SurfaceState::new(
            resolved_width,
            resolved_height,
            title.clone(),
            backend.clone(),
            adapter.clone(),
        ),
    );
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(backend)),
        (":adapter", Term::Str(adapter)),
        (":profile", Term::Str(profile.as_str().to_string())),
        (":surface", Term::Str(sid)),
        (":width", Term::Int(resolved_width.into())),
        (":height", Term::Int(resolved_height.into())),
        (":title", Term::Str(title)),
        (":title-applied", Term::Bool(title_applied)),
        (":created", Term::Bool(created)),
        (":pending-redraws", Term::Int(0_i64.into())),
    ])
}

fn first_party_resize_surface(runtime: &mut GfxHostRuntime, payload: &Term) -> Term {
    let Some(sid) = payload_surface_id(payload) else {
        return missing_surface_error("gfx/window::resize-surface");
    };
    let Some(surface) = runtime.surfaces.get_mut(&sid) else {
        return unknown_surface_error("gfx/window::resize-surface", &sid);
    };
    let size = payload_map(payload)
        .and_then(|m| m.get(&TermOrdKey(Term::symbol(":size"))))
        .and_then(payload_map);
    if let Some(w) = size.and_then(|m| map_get_i64(m, ":width")) {
        surface.width = w;
    }
    if let Some(h) = size.and_then(|m| map_get_i64(m, ":height")) {
        surface.height = h;
    }
    if surface.adapter == interactive_adapter_name() {
        if let Some((w, h)) = interactive_query_surface_size() {
            surface.width = w;
            surface.height = h;
        }
    } else if surface.adapter == desktop_adapter_name()
        && let Some((w, h)) = desktop_query_surface_size(&sid)
    {
        surface.width = w;
        surface.height = h;
    }
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(surface.backend.clone())),
        (":adapter", Term::Str(surface.adapter.clone())),
        (":surface", Term::Str(sid)),
        (":width", Term::Int(surface.width.into())),
        (":height", Term::Int(surface.height.into())),
    ])
}

fn first_party_set_title(runtime: &mut GfxHostRuntime, payload: &Term) -> Term {
    let Some(sid) = payload_surface_id(payload) else {
        return missing_surface_error("gfx/window::set-title");
    };
    let Some(surface) = runtime.surfaces.get_mut(&sid) else {
        return unknown_surface_error("gfx/window::set-title", &sid);
    };
    let mut applied = true;
    if let Some(title) = payload_map(payload).and_then(|m| map_get_string(m, ":title")) {
        if surface.adapter == interactive_adapter_name() {
            applied = interactive_apply_title(&title);
        } else if surface.adapter == desktop_adapter_name() {
            applied = desktop_apply_title(&sid, &title);
        }
        surface.title = title;
    }
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(surface.backend.clone())),
        (":adapter", Term::Str(surface.adapter.clone())),
        (":surface", Term::Str(sid)),
        (":title", Term::Str(surface.title.clone())),
        (":title-applied", Term::Bool(applied)),
    ])
}

fn first_party_request_redraw(runtime: &mut GfxHostRuntime, payload: &Term) -> Term {
    let Some(sid) = payload_surface_id(payload) else {
        return missing_surface_error("gfx/window::request-redraw");
    };
    let Some(surface) = runtime.surfaces.get_mut(&sid) else {
        return unknown_surface_error("gfx/window::request-redraw", &sid);
    };
    surface.pending_redraws = surface.pending_redraws.saturating_add(1);
    let applied = if surface.adapter == desktop_adapter_name() {
        desktop_request_redraw(&sid)
    } else {
        true
    };
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(surface.backend.clone())),
        (":adapter", Term::Str(surface.adapter.clone())),
        (":surface", Term::Str(sid)),
        (":redraw-applied", Term::Bool(applied)),
        (
            ":pending-redraws",
            Term::Int((surface.pending_redraws as i64).into()),
        ),
    ])
}

fn first_party_surface_info(runtime: &mut GfxHostRuntime, payload: &Term) -> Term {
    let Some(sid) = payload_surface_id(payload) else {
        return missing_surface_error("gfx/window::surface-info");
    };
    let Some(surface) = runtime.surfaces.get_mut(&sid) else {
        return unknown_surface_error("gfx/window::surface-info", &sid);
    };
    if surface.adapter == interactive_adapter_name() {
        if let Some((w, h)) = interactive_query_surface_size() {
            surface.width = w;
            surface.height = h;
        }
    } else if surface.adapter == desktop_adapter_name()
        && let Some((w, h)) = desktop_query_surface_size(&sid)
    {
        surface.width = w;
        surface.height = h;
    }
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(surface.backend.clone())),
        (":adapter", Term::Str(surface.adapter.clone())),
        (":surface", Term::Str(sid)),
        (":width", Term::Int(surface.width.into())),
        (":height", Term::Int(surface.height.into())),
        (":title", Term::Str(surface.title.clone())),
        (
            ":pending-redraws",
            Term::Int((surface.pending_redraws as i64).into()),
        ),
        (":cursor-mode", Term::Str(surface.cursor_mode.clone())),
    ])
}

fn first_party_poll_events(
    runtime: &mut GfxHostRuntime,
    payload: &Term,
    profile: &GfxFirstPartyProfile,
) -> Term {
    let Some(sid) = payload_surface_id(payload) else {
        return missing_surface_error("gfx/input::poll-events");
    };
    let Some(surface) = runtime.surfaces.get_mut(&sid) else {
        return unknown_surface_error("gfx/input::poll-events", &sid);
    };

    let max_events = payload_map(payload)
        .and_then(|m| map_get_i64(m, ":max-events"))
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(64_usize);

    let mut events = Vec::new();
    if matches!(profile, GfxFirstPartyProfile::Interactive) {
        if surface.pending_redraws > 0 {
            surface.pending_redraws -= 1;
            surface.poll_seq = surface.poll_seq.saturating_add(1);
            events.push(map_term(vec![
                (":kind", Term::symbol(":redraw")),
                (":surface", Term::Str(sid.clone())),
                (":seq", Term::Int((surface.poll_seq as i64).into())),
            ]));
        }
        let remaining_slots = max_events.saturating_sub(events.len());
        if remaining_slots > 0 {
            events.extend(interactive_poll_events(
                &sid,
                &mut surface.poll_seq,
                remaining_slots,
            ));
        }
    } else if matches!(
        profile,
        GfxFirstPartyProfile::Desktop | GfxFirstPartyProfile::Browser
    ) {
        if surface.pending_redraws > 0 {
            surface.pending_redraws -= 1;
            surface.poll_seq = surface.poll_seq.saturating_add(1);
            events.push(map_term(vec![
                (":kind", Term::symbol(":redraw")),
                (":surface", Term::Str(sid.clone())),
                (":seq", Term::Int((surface.poll_seq as i64).into())),
            ]));
        }
        let remaining_slots = max_events.saturating_sub(events.len());
        if matches!(profile, GfxFirstPartyProfile::Desktop) && remaining_slots > 0 {
            events.extend(desktop_poll_events(
                &sid,
                &mut surface.poll_seq,
                remaining_slots,
            ));
        }
    }

    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(surface.backend.clone())),
        (":adapter", Term::Str(surface.adapter.clone())),
        (":surface", Term::Str(sid)),
        (":events", Term::Vector(events)),
    ])
}

fn first_party_set_cursor_mode(runtime: &mut GfxHostRuntime, payload: &Term) -> Term {
    let Some(sid) = payload_surface_id(payload) else {
        return missing_surface_error("gfx/input::set-cursor-mode");
    };
    let Some(surface) = runtime.surfaces.get_mut(&sid) else {
        return unknown_surface_error("gfx/input::set-cursor-mode", &sid);
    };
    let mut applied = true;
    if let Some(mode) = payload_map(payload).and_then(|m| map_get_string(m, ":mode")) {
        if surface.adapter == interactive_adapter_name() {
            applied = interactive_apply_cursor_mode(&mode);
        } else if surface.adapter == desktop_adapter_name() {
            applied = desktop_apply_cursor_mode(&sid, &mode);
        }
        surface.cursor_mode = mode;
    }
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str(surface.backend.clone())),
        (":adapter", Term::Str(surface.adapter.clone())),
        (":surface", Term::Str(sid)),
        (":mode", Term::Str(surface.cursor_mode.clone())),
        (":applied", Term::Bool(applied)),
    ])
}

fn first_party_set_master(
    runtime: &mut GfxHostRuntime,
    payload: &Term,
    profile: &GfxFirstPartyProfile,
) -> Term {
    let gain = payload_map(payload)
        .and_then(|m| map_get_i64(m, ":gain"))
        .unwrap_or(1_i64);
    runtime.master_gain = gain;
    map_term(vec![
        (":ok", Term::Bool(true)),
        (
            ":backend",
            Term::Str(backend_for_profile(profile).to_string()),
        ),
        (
            ":adapter",
            Term::Str(adapter_for_profile(profile).to_string()),
        ),
        (":gain", Term::Int(runtime.master_gain.into())),
    ])
}

fn first_party_enqueue(
    runtime: &mut GfxHostRuntime,
    _payload: &Term,
    profile: &GfxFirstPartyProfile,
) -> Term {
    runtime.audio_queued = runtime.audio_queued.saturating_add(1);
    let bell_applied = match profile {
        GfxFirstPartyProfile::Interactive => interactive_enqueue_audio_bell(),
        GfxFirstPartyProfile::Desktop => desktop_enqueue_audio_bell(),
        GfxFirstPartyProfile::Browser => false,
        GfxFirstPartyProfile::Headless => false,
    };
    map_term(vec![
        (":ok", Term::Bool(true)),
        (
            ":backend",
            Term::Str(backend_for_profile(profile).to_string()),
        ),
        (
            ":adapter",
            Term::Str(adapter_for_profile(profile).to_string()),
        ),
        (":queued", Term::Int((runtime.audio_queued as i64).into())),
        (":bell-applied", Term::Bool(bell_applied)),
    ])
}
