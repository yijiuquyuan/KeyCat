// The Windows host for the web scene in ../web. Mirrors the mac build's mac/App.swift and
// mac/AppMenu.swift: a transparent, click-through floating window reports the pointer to the page,
// forwards global keyboard and mouse input, and offers the same tray menu. The page is drawn by
// the system WebView2, so the app ships no browser of its own. tauri-plugin-updater handles updates.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod elevate;
#[cfg_attr(not(windows), allow(dead_code))]
mod hook;
#[cfg_attr(not(windows), allow(dead_code))]
mod keymap;
mod store;

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::webview::PageLoadEvent;
use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, RunEvent, Webview, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Wry,
};
use tauri_plugin_updater::{Update, UpdaterExt};

use autostart::Mode;
use hook::{Button, Input};
use store::Store;

const DESIGN: (f64, f64) = (360.0, 368.0); // mac/App.swift designSize at scale 1
const PANEL: &str = "panel";
const SETTINGS: &str = "settings";
const TRAY: &str = "tray";
const DEV: bool = cfg!(debug_assertions);
/// tauri.conf.json `identifier`, which names the app data folder in %APPDATA%.
const IDENTIFIER: &str = "com.ellobok.keycat";

// The web scene talks to its host through `window.webkit.messageHandlers.<name>.postMessage`,
// the WKWebView bridge the mac build uses. Recreate that shape on top of Tauri IPC so web/ stays
// untouched. Right-clicks belong to the tray-style menu, not WebView2's own.
const BRIDGE: &str = r#"(() => {
  const handler = channel => ({
    postMessage: message => window.__TAURI_INTERNALS__.invoke('host_message', { channel, message }).catch(() => {}),
  });
  window.webkit = { messageHandlers: { cat: handler('cat'), settings: handler('settings') } };
  window.addEventListener('contextmenu', event => event.preventDefault());
})();"#;

// MARK: State

#[derive(Clone, PartialEq)]
enum UpdateState {
    Idle,
    Checking,
    Downloading(String, u32),
    Ready(String),
    Current,
    Failed,
}

struct Drag {
    cursor: (f64, f64),
    origin: (i32, i32),
    last: (i32, i32),
}

struct Host {
    store: Store,
    page_ready: bool,
    hook_ready: bool,
    hit: String,
    petting: bool,
    drag: Option<Drag>,
    pending_delta: (f64, f64),
    last_pointer: Option<(f64, f64)>,
    last_cursor: (i32, i32),
    size_fixes: u32,
    key_events: u64,
    held: HashSet<u32>,
    autostart: Mode,
    update: UpdateState,
    downloaded: Option<(Update, Vec<u8>)>,
    /// The page has sent a message, so its script is running.
    page_answered: bool,
    /// The cat window could not be made, or its page never answered (see watch_page).
    page_failed: bool,
}

/// Settings with their defaults applied, the same keys and fallbacks as the mac build.
struct Settings {
    style: String,
    coat: String,
    pattern: String,
    eye: String,
    /// "auto" follows the coat (the cobby breeds read round-eyed); "almond" forces the slit pupil.
    eye_shape: String,
    shine: String,
    collar: String,
    blush: bool,
    whiskers: bool,
    privacy: bool,
    click_through: bool,
    run_as_admin: bool,
    sleep_after: f64,
    tune: Value,
    scale: f64,
}

impl Host {
    fn scale(&self) -> f64 {
        self.store.get("catScale").and_then(Value::as_f64).filter(|v| v.is_finite() && *v > 0.3).unwrap_or(0.9)
    }

    fn settings(&self) -> Settings {
        let text = |key: &str, fallback: &str| {
            self.store.get(key).and_then(Value::as_str).unwrap_or(fallback).to_string()
        };
        let number = |key: &str| self.store.get(key).and_then(Value::as_f64).filter(|v| v.is_finite());
        let coat = text("coat", "black");
        Settings {
            style: text("style", "2d"),
            pattern: text(&format!("pattern.{coat}"), "auto"),
            eye: text("eyeColor", "auto"),
            eye_shape: text("eyeShape", "auto"),
            shine: text("eyeShine", "classic"),
            collar: text("collar", "none"),
            blush: self.store.get("blush") == Some(&Value::Bool(true)),
            whiskers: self.store.get("whiskers") != Some(&Value::Bool(false)),
            privacy: self.store.get("privacy") == Some(&Value::Bool(true)),
            click_through: self.store.get("clickThrough") == Some(&Value::Bool(true)),
            run_as_admin: self.store.get("runAsAdmin") == Some(&Value::Bool(true)),
            sleep_after: number("sleepAfter").unwrap_or(60.0),
            tune: self.store.get("tune").filter(|v| v.is_object()).cloned().unwrap_or_else(|| json!({})),
            scale: self.scale(),
            coat,
        }
    }
}

fn host(app: &AppHandle) -> MutexGuard<'_, Host> {
    app.state::<Mutex<Host>>().inner().lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn panel(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(PANEL)
}

// MARK: Logging

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

fn data_dir() -> PathBuf {
    DATA_DIR.get().cloned().unwrap_or_else(std::env::temp_dir)
}

/// UTC time as ISO 8601, like JavaScript's `toISOString` without the milliseconds.
fn timestamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest / 60 % 60, rest % 60)
}

fn log(text: &str) {
    // Logging must never take the app down.
    let dir = data_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("log.txt");
    // Past 1 MB the log moves to log.old.txt, replacing the one before, so the two stay small.
    if fs::metadata(&path).is_ok_and(|meta| meta.len() > 1 << 20) {
        let _ = fs::rename(&path, dir.join("log.old.txt"));
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{} {text}", timestamp());
    }
}

// MARK: Page

fn js(app: &AppHandle, script: &str) {
    if !host(app).page_ready {
        return;
    }
    if let Some(panel) = panel(app) {
        let _ = panel.eval(script);
    }
}

fn push_options(app: &AppHandle) {
    let s = host(app).settings();
    let options = json!({
        "style": s.style,
        "coat": s.coat,
        "pattern": s.pattern,
        "eye": s.eye,
        "eyeShape": s.eye_shape,
        "shine": s.shine,
        "blush": s.blush,
        "whiskers": s.whiskers,
        "collar": s.collar,
        "privacy": s.privacy,
        "sleepAfter": if s.sleep_after > 0.0 { s.sleep_after } else { 1e9 },
        "tune": s.tune,
    });
    js(app, &format!("cat.options({options})"));
}

fn on_page_ready(app: &AppHandle) {
    let first = {
        let mut h = host(app);
        let first = !h.page_ready;
        h.page_ready = true;
        first
    };
    log("page loaded");
    push_options(app);
    if first {
        start_status_reports(app);
    }
}

// MARK: Window

fn window_size(scale: f64) -> LogicalSize<f64> {
    LogicalSize::new((DESIGN.0 * scale).round(), (DESIGN.1 * scale).round())
}

fn saved_origin(app: &AppHandle) -> Option<PhysicalPosition<i32>> {
    let (x, y) = {
        let h = host(app);
        (h.store.get("originX")?.as_f64()?, h.store.get("originY")?.as_f64()?)
    };
    let (x, y) = (x as i32, y as i32);
    // Only if an 80 px corner of the cat would still land on a connected screen.
    let visible = app.available_monitors().unwrap_or_default().iter().any(|monitor| {
        let (pos, size) = (monitor.position(), monitor.size());
        x < pos.x + size.width as i32 && pos.x < x + 80 && y < pos.y + size.height as i32 && pos.y < y + 80
    });
    visible.then(|| PhysicalPosition::new(x, y))
}

/// Bottom-right of the primary screen's work area (the taskbar is excluded), like the mac build.
fn default_origin(app: &AppHandle, size: LogicalSize<f64>) -> PhysicalPosition<i32> {
    let Some(monitor) = app.primary_monitor().ok().flatten() else {
        return PhysicalPosition::new(0, 0);
    };
    let area = monitor.work_area();
    let factor = monitor.scale_factor();
    PhysicalPosition::new(
        area.position.x + area.size.width as i32 - (size.width * factor).round() as i32 - (20.0 * factor).round() as i32,
        area.position.y + area.size.height as i32 - (size.height * factor).round() as i32,
    )
}

fn save_origin(app: &AppHandle, origin: (i32, i32)) {
    let mut h = host(app);
    h.store.set("originX", json!(origin.0));
    h.store.set("originY", json!(origin.1));
}

fn apply_scale(app: &AppHandle, value: f64, keep_origin: bool) {
    let scale = value.clamp(0.4, 2.0);
    host(app).store.set("catScale", json!(scale));
    let Some(panel) = panel(app) else { return };
    let size = window_size(scale);
    let origin = match keep_origin {
        true => panel.outer_position().ok(),
        false => None,
    }
    .or_else(|| saved_origin(app))
    .unwrap_or_else(|| default_origin(app, size));
    let _ = panel.set_size(size);
    let _ = panel.set_position(origin);
}

fn create_panel(app: &AppHandle) -> tauri::Result<()> {
    let scale = host(app).settings().scale;
    let size = window_size(scale);
    let panel = WebviewWindowBuilder::new(app, PANEL, WebviewUrl::App("index.html".into()))
        .title("KeyCat")
        .inner_size(size.width, size.height)
        .visible(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .focused(false)
        .focusable(false)
        .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Disabled)
        .initialization_script(BRIDGE)
        .on_page_load(|window, payload| {
            if payload.event() == PageLoadEvent::Finished {
                on_page_ready(window.app_handle());
            }
        })
        .build()?;
    let origin = saved_origin(app).unwrap_or_else(|| default_origin(app, size));
    panel.set_position(origin)?;
    panel.set_ignore_cursor_events(true)?;
    panel.show()?;
    log("panel created");
    watch_page(app);
    Ok(())
}

/// WebView2 can fail without an error reaching us: the window stays blank and the page never
/// loads or never runs. A page that has not sent a single message 30 s after its window was
/// made (it answers the frame count every 15 s, see start_status_reports) goes in the log, and
/// the menu says so until it answers.
fn watch_page(app: &AppHandle) {
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(30));
        let page_ready = {
            let mut h = host(&app);
            if h.page_answered {
                return;
            }
            h.page_failed = true;
            h.page_ready
        };
        log(&format!("page has not answered 30 s after start (load finished: {page_ready})"));
        refresh_menu(&app);
    });
}

// MARK: Frame loop

fn tick(app: &AppHandle) {
    let Some(panel) = panel(app) else { return };
    if !host(app).page_ready {
        return;
    }
    let Ok(pointer) = app.cursor_position() else { return };
    let dragged = host(app).drag.as_mut().map(|drag| {
        drag.last = (
            drag.origin.0 + (pointer.x - drag.cursor.0).round() as i32,
            drag.origin.1 + (pointer.y - drag.cursor.1).round() as i32,
        );
        drag.last
    });
    let origin = match dragged {
        Some(origin) => {
            let _ = panel.set_position(PhysicalPosition::new(origin.0, origin.1));
            origin
        }
        None => match panel.outer_position() {
            Ok(pos) => (pos.x, pos.y),
            Err(_) => return,
        },
    };
    // The page works in CSS pixels; the pointer and the window are in physical pixels.
    let factor = panel.scale_factor().unwrap_or(1.0);
    keep_size(app, &panel, factor);
    let cursor = (
        ((pointer.x - f64::from(origin.0)) / factor).round() as i32,
        ((pointer.y - f64::from(origin.1)) / factor).round() as i32,
    );
    let mut calls = Vec::new();
    {
        let mut h = host(app);
        // The hook reports no pointer moves, so the movement is the pointer's change between
        // frames, in CSS pixels like the mac build's event deltas. It drives the mouse paw and
        // the pointer speed that makes the cat curious.
        if let Some(last) = h.last_pointer.replace((pointer.x, pointer.y)) {
            h.pending_delta.0 += (pointer.x - last.0) / factor;
            h.pending_delta.1 += (pointer.y - last.1) / factor;
        }
        if cursor != h.last_cursor {
            h.last_cursor = cursor;
            calls.push(format!("cat.cursor({},{})", cursor.0, cursor.1));
        }
        if h.pending_delta != (0.0, 0.0) {
            calls.push(format!("cat.mouseMove({:.1},{:.1})", h.pending_delta.0, h.pending_delta.1));
            h.pending_delta = (0.0, 0.0);
        }
    }
    if !calls.is_empty() {
        js(app, &calls.join(";"));
    }
}

/// Holds the panel to the size picked in the menu. Users have seen it creep larger while the
/// pointer moves over the cat or the cat is dragged, which makes the cat grow; log.txt notes
/// the first few corrections so the cause can be traced.
fn keep_size(app: &AppHandle, panel: &WebviewWindow, factor: f64) {
    let Ok(actual) = panel.inner_size() else { return };
    let want = window_size(host(app).scale());
    let off = |have: u32, logical: f64| (f64::from(have) - (logical * factor).round()).abs() > 1.0;
    if !off(actual.width, want.width) && !off(actual.height, want.height) {
        return;
    }
    let fixes = {
        let mut h = host(app);
        h.size_fixes += 1;
        h.size_fixes
    };
    if fixes <= 5 {
        log(&format!(
            "panel was {}x{}, restoring {}x{} (scale factor {factor})",
            actual.width,
            actual.height,
            (want.width * factor).round(),
            (want.height * factor).round()
        ));
    }
    let _ = panel.set_size(want);
}

/// Feeds hook input and a ~60 fps tick to the main thread, where all the state lives. A tick is
/// skipped while the previous one is still queued (a menu is open, say) rather than piling up.
fn start_pump(app: &AppHandle) {
    let (sink, inputs) = mpsc::channel::<Input>();
    match hook::start(sink.clone()) {
        Ok(()) => {
            host(app).hook_ready = true;
            log("global input hook started");
        }
        Err(error) => log(&format!("global input hook failed: {error}")),
    }
    let app = app.clone();
    let queued = Arc::new(AtomicBool::new(false));
    thread::Builder::new()
        .name("pump".into())
        .spawn(move || {
            // Holding a sender keeps the channel open even if the hook never started.
            let _sink = sink;
            let frame = Duration::from_millis(16);
            let mut next = Instant::now() + frame;
            loop {
                match inputs.recv_timeout(next.saturating_duration_since(Instant::now())) {
                    Ok(input) => {
                        let app2 = app.clone();
                        let _ = app.run_on_main_thread(move || on_input(&app2, input));
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        next = Instant::now() + frame;
                        if !queued.swap(true, Ordering::AcqRel) {
                            let (app2, queued) = (app.clone(), queued.clone());
                            let _ = app.run_on_main_thread(move || {
                                queued.store(false, Ordering::Release);
                                tick(&app2);
                            });
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        })
        .expect("spawn pump thread");
}

// MARK: Input

fn pointer_in_panel(app: &AppHandle) -> Option<(WebviewWindow, (f64, f64))> {
    let panel = panel(app)?;
    let pointer = app.cursor_position().ok()?;
    let (pos, size) = (panel.outer_position().ok()?, panel.outer_size().ok()?);
    let inside = pointer.x >= f64::from(pos.x)
        && pointer.x < f64::from(pos.x) + f64::from(size.width)
        && pointer.y >= f64::from(pos.y)
        && pointer.y < f64::from(pos.y) + f64::from(size.height);
    inside.then_some((panel, (pointer.x, pointer.y)))
}

fn on_input(app: &AppHandle, input: Input) {
    match input {
        Input::Key { id, code, down } => {
            {
                let mut h = host(app);
                // Autorepeat: one paw press per physical press.
                if down && !h.held.insert(id) {
                    return;
                }
                if !down {
                    h.held.remove(&id);
                }
                h.key_events += 1;
            }
            if let Some(code) = code {
                js(app, &format!("cat.key('{code}',{down})"));
            }
        }
        Input::Button { button, down } => on_button(app, button, down),
        // Feed the wheel like a vertical mouse move, as the mac build does.
        Input::Wheel(notches) => host(app).pending_delta.1 += -notches * 0.4,
    }
}

fn on_button(app: &AppHandle, button: Button, down: bool) {
    if down {
        let (hit, click_through) = {
            let h = host(app);
            (h.hit.clone(), h.settings().click_through)
        };
        if hit != "none" && !click_through {
            if let Some((panel, pointer)) = pointer_in_panel(app) {
                match button {
                    Button::Right => show_context_menu(app, &panel),
                    Button::Left if hit == "pet" => start_pet(app, &panel, pointer),
                    Button::Left => start_drag(app, &panel, pointer),
                    Button::Other => {}
                }
            }
        }
    } else {
        stop_pet(app);
        stop_drag(app);
    }
    js(app, &format!("cat.mouseButton({down})"));
}

fn start_pet(app: &AppHandle, panel: &WebviewWindow, pointer: (f64, f64)) {
    {
        let mut h = host(app);
        if h.petting {
            return;
        }
        h.petting = true;
    }
    let left = panel.outer_position().map(|pos| f64::from(pos.x)).unwrap_or(0.0);
    let factor = panel.scale_factor().unwrap_or(1.0);
    js(app, &format!("cat.pet(true, {})", ((pointer.0 - left) / factor).round()));
}

fn stop_pet(app: &AppHandle) {
    if std::mem::take(&mut host(app).petting) {
        js(app, "cat.pet(false, 0)");
    }
}

fn start_drag(app: &AppHandle, panel: &WebviewWindow, pointer: (f64, f64)) {
    let Ok(pos) = panel.outer_position() else { return };
    {
        let mut h = host(app);
        if h.drag.is_some() {
            return;
        }
        h.drag = Some(Drag { cursor: pointer, origin: (pos.x, pos.y), last: (pos.x, pos.y) });
    }
    let _ = panel.set_ignore_cursor_events(false);
}

fn stop_drag(app: &AppHandle) {
    let Some(drag) = host(app).drag.take() else { return };
    save_origin(app, drag.last);
    update_click_target(app);
}

/// Only the cat itself takes clicks, and in click-through mode not even that: everything goes
/// to the window underneath, and the menu is reached from the tray.
fn update_click_target(app: &AppHandle) {
    let ignore = {
        let h = host(app);
        if h.drag.is_some() {
            return;
        }
        h.hit == "none" || h.settings().click_through
    };
    if let Some(panel) = panel(app) {
        let _ = panel.set_ignore_cursor_events(ignore);
    }
}

// MARK: Bridge

#[tauri::command]
fn host_message(app: AppHandle, webview: Webview, channel: String, message: Value) {
    match (webview.label(), channel.as_str()) {
        (PANEL, "cat") => on_cat_message(&app, &message),
        (SETTINGS, "settings") => on_settings_message(&app, &message),
        _ => {}
    }
}

fn on_cat_message(app: &AppHandle, message: &Value) {
    let recovered = {
        let mut h = host(app);
        h.page_answered = true;
        std::mem::take(&mut h.page_failed)
    };
    if recovered {
        log("page answered");
        refresh_menu(app);
    }
    if let Some(hit) = message.get("hit").and_then(Value::as_str) {
        host(app).hit = hit.to_string();
        update_click_target(app);
    } else if let Some(ticks) = message.get("ticks").and_then(Value::as_f64) {
        write_status(app, ticks);
    }
}

fn on_settings_message(app: &AppHandle, body: &Value) {
    if let Some(tune) = body.get("tune").and_then(Value::as_object) {
        {
            let mut h = host(app);
            let mut merged = h.settings().tune;
            if let Some(saved) = merged.as_object_mut() {
                saved.extend(tune.clone());
            }
            h.store.set("tune", merged);
        }
        push_options(app);
    } else if body.get("reset").and_then(Value::as_bool) == Some(true) {
        host(app).store.set("tune", json!({}));
        push_options(app);
    } else if let Some(test) = body.get("test").and_then(Value::as_str) {
        js(app, if test == "annoy" { "cat.annoyNow()" } else { "cat.curiousNow(3)" });
    }
}

// MARK: Menu

type Item = Box<dyn IsMenuItem<Wry>>;

fn check(app: &AppHandle, id: String, text: &str, checked: bool) -> tauri::Result<Item> {
    Ok(Box::new(CheckMenuItem::with_id(app, id, text, true, checked, None::<&str>)?))
}

fn action(app: &AppHandle, id: &str, text: &str) -> tauri::Result<Item> {
    Ok(Box::new(MenuItem::with_id(app, id, text, true, None::<&str>)?))
}

fn note(app: &AppHandle, text: &str) -> tauri::Result<Item> {
    Ok(Box::new(MenuItem::new(app, text, false, None::<&str>)?))
}

fn separator(app: &AppHandle) -> tauri::Result<Item> {
    Ok(Box::new(PredefinedMenuItem::separator(app)?))
}

fn submenu(app: &AppHandle, text: &str, items: Vec<Item>) -> tauri::Result<Item> {
    let refs: Vec<&dyn IsMenuItem<Wry>> = items.iter().map(|item| item.as_ref()).collect();
    Ok(Box::new(Submenu::with_items(app, text, true, &refs)?))
}

/// Radio-style items: each id saves `key = value` (the value as JSON) when clicked.
fn choice(app: &AppHandle, key: &str, current: &Value, options: &[(&str, Value)]) -> tauri::Result<Vec<Item>> {
    options
        .iter()
        .map(|(text, value)| check(app, format!("set:{key}={value}"), text, same(current, value)))
        .collect()
}

/// Settings written by the 1.0.x Electron build store whole numbers, so 60 has to match 60.0.
fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

fn toggle(app: &AppHandle, text: &str, key: &str, on: bool) -> tauri::Result<Item> {
    check(app, format!("set:{key}={}", !on), text, on)
}

fn update_item(app: &AppHandle) -> tauri::Result<Item> {
    let version = app.package_info().version.to_string();
    if DEV {
        return note(app, &format!("开发模式 v{version}"));
    }
    match host(app).update.clone() {
        UpdateState::Ready(new) => action(app, "update:install", &format!("重启并更新到 v{new}")),
        UpdateState::Downloading(_, percent) => note(app, &format!("正在下载更新 {percent}%")),
        UpdateState::Checking => note(app, "正在检查更新…"),
        UpdateState::Current => note(app, &format!("已是最新版本 v{version}")),
        UpdateState::Failed => action(app, "update:check", "检查更新失败，点击重试"),
        UpdateState::Idle => action(app, "update:check", "检查更新…"),
    }
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let (s, hook_ready, autostart, page_failed) = {
        let h = host(app);
        (h.settings(), h.hook_ready, h.autostart, h.page_failed)
    };
    let coat = s.coat.as_str();
    let mut items = vec![check(app, format!("set:style={}", json!(if s.style == "3d" { "2d" } else { "3d" })), "立体效果（3D）", s.style == "3d")?];
    items.push(submenu(
        app,
        "毛色",
        choice(app, "coat", &json!(coat), &[
            ("黑猫", json!("black")),
            ("奶牛猫", json!("tuxedo")),
            ("橘猫", json!("orange")),
            ("蓝灰猫", json!("grey")),
            ("白猫", json!("white")),
            ("三花猫", json!("calico")),
            ("暹罗猫", json!("siamese")),
            ("布偶猫", json!("ragdoll")),
            ("德文卷毛猫", json!("devon")),
            ("美短银虎斑", json!("silver")),
            ("狸花猫", json!("lihua")),
            ("金渐层", json!("golden")),
        ])?,
    )?);
    // Only coats whose stripes would show get the choice; black and white stay plain.
    if coat == "orange" || coat == "grey" {
        let striped = s.pattern == "tabby" || (s.pattern == "auto" && coat == "orange");
        let current = json!(if striped { "tabby" } else { "solid" });
        let options = [("纯色", json!("solid")), ("虎斑条纹", json!("tabby"))];
        items.push(submenu(app, "花纹", choice(app, &format!("pattern.{coat}"), &current, &options)?)?);
    }
    let eyes = choice(app, "eyeColor", &json!(s.eye), &[
        ("跟随毛色", json!("auto")),
        ("金色", json!("gold")),
        ("绿色", json!("green")),
        ("蓝色", json!("blue")),
        ("琥珀色", json!("amber")),
        ("鸳鸯眼（一蓝一金）", json!("odd")),
    ])?;
    let shine = choice(app, "eyeShine", &json!(s.shine), &[
        ("无", json!("none")),
        ("经典", json!("classic")),
        ("闪亮", json!("sparkle")),
    ])?;
    let collar = choice(app, "collar", &json!(s.collar), &[
        ("无", json!("none")),
        ("红色铃铛", json!("red")),
        ("蓝色铃铛", json!("blue")),
        ("粉色铃铛", json!("pink")),
        ("绿色铃铛", json!("green")),
    ])?;
    let eye_shape = choice(app, "eyeShape", &json!(s.eye_shape), &[
        ("跟随毛色", json!("auto")),
        ("杏仁眼（竖瞳）", json!("almond")),
        ("圆眼（圆瞳）", json!("round")),
    ])?;
    items.push(submenu(app, "外观", vec![
        submenu(app, "眼睛颜色", eyes)?,
        submenu(app, "眼睛款式", eye_shape)?,
        submenu(app, "眼睛高光", shine)?,
        submenu(app, "项圈", collar)?,
        toggle(app, "腮红", "blush", s.blush)?,
        toggle(app, "胡须", "whiskers", s.whiskers)?,
    ])?);
    // 特小 is exactly two thirds of 小, matching the mac menu.
    let sizes = [("特小", 0.7 * 2.0 / 3.0), ("小", 0.7), ("中", 0.9), ("大", 1.1), ("特大", 1.35)]
        .iter()
        .map(|(text, value)| check(app, format!("scale:{value}"), text, s.scale == *value))
        .collect::<tauri::Result<Vec<_>>>()?;
    items.push(submenu(app, "大小", sizes)?);
    items.push(submenu(
        app,
        "多久睡着",
        choice(app, "sleepAfter", &json!(s.sleep_after), &[
            ("30 秒", json!(30.0)),
            ("1 分钟", json!(60.0)),
            ("3 分钟", json!(180.0)),
            ("10 分钟", json!(600.0)),
            ("不睡", json!(0.0)),
        ])?,
    )?);
    items.push(toggle(app, "隐私模式（不显示键名）", "privacy", s.privacy)?);
    items.push(toggle(app, "鼠标穿透（点击直接落到下层窗口）", "clickThrough", s.click_through)?);
    items.push(action(app, "settings", "调节反应…")?);
    items.push(separator(app)?);
    if DEV {
        items.push(note(app, "开机自启（开发模式不可用）")?);
        items.push(note(app, "以管理员身份运行（开发模式不可用）")?);
    } else {
        items.push(check(app, "autostart".into(), "开机自启", autostart != Mode::Off)?);
        items.push(check(app, "admin".into(), "以管理员身份运行（游戏里也有按键反馈）", s.run_as_admin)?);
    }
    items.push(action(app, "home", "回到右下角")?);
    let visible = panel(app).and_then(|panel| panel.is_visible().ok()).unwrap_or(false);
    items.push(action(app, "visibility", if visible { "隐藏" } else { "显示" })?);
    items.push(separator(app)?);
    items.push(update_item(app)?);
    if !DEV && !hook_ready {
        items.push(note(app, "键盘监听未启动（详见 log.txt）")?);
    }
    if page_failed {
        items.push(note(app, "猫的页面未加载（详见 log.txt）")?);
    }
    items.push(separator(app)?);
    items.push(action(app, "quit", "退出 KeyCat")?);
    let refs: Vec<&dyn IsMenuItem<Wry>> = items.iter().map(|item| item.as_ref()).collect();
    Menu::with_items(app, &refs)
}

fn rebuild_menu(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY) else { return };
    match build_menu(app) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(error) => log(&format!("menu build failed: {error}")),
    }
}

/// Rebuilds the tray menu from any thread.
fn refresh_menu(app: &AppHandle) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || rebuild_menu(&app2));
}

fn show_context_menu(app: &AppHandle, panel: &WebviewWindow) {
    match build_menu(app) {
        Ok(menu) => {
            let _ = panel.popup_menu(&menu);
        }
        Err(error) => log(&format!("menu build failed: {error}")),
    }
}

/// Saves a look setting, sends it to the cat and redraws the menu so the checkmarks follow.
fn on_menu(app: &AppHandle, id: &str) {
    if let Some((key, raw)) = id.strip_prefix("set:").and_then(|rest| rest.split_once('=')) {
        if let Ok(value) = serde_json::from_str::<Value>(raw) {
            host(app).store.set(key, value);
            push_options(app);
            update_click_target(app);
        }
    } else if let Some(value) = id.strip_prefix("scale:").and_then(|raw| raw.parse::<f64>().ok()) {
        apply_scale(app, value, true);
    } else {
        match id {
            "settings" => open_settings(app),
            "autostart" => {
                let on = host(app).autostart == Mode::Off;
                set_autostart(app, if on { autostart_mode(app) } else { Mode::Off });
            }
            "admin" => toggle_admin(app),
            "home" => {
                {
                    let mut h = host(app);
                    h.store.remove("originX");
                    h.store.remove("originY");
                }
                let scale = host(app).settings().scale;
                apply_scale(app, scale, false);
            }
            "visibility" => {
                if let Some(panel) = panel(app) {
                    let _ = if panel.is_visible().unwrap_or(false) { panel.hide() } else { panel.show() };
                }
            }
            "update:check" => {
                tauri::async_runtime::spawn(check_for_updates(app.clone(), true));
            }
            "update:install" => install_update(app),
            "quit" => app.exit(0),
            _ => {}
        }
    }
    rebuild_menu(app);
}

// MARK: Administrator mode

/// How start at login has to work for the way KeyCat runs now.
fn autostart_mode(app: &AppHandle) -> Mode {
    if host(app).settings().run_as_admin && elevate::is_elevated() {
        Mode::Task
    } else {
        Mode::RunKey
    }
}

fn set_autostart(app: &AppHandle, mode: Mode) {
    match autostart::set(mode) {
        Ok(()) => host(app).autostart = mode,
        Err(error) => log(&format!("autostart change to {mode:?} failed: {error}")),
    }
}

/// Moves an enabled start at login over to the kind that matches the administrator setting.
/// Without elevation the task can be neither made nor removed, so that waits for an elevated run.
fn sync_autostart(app: &AppHandle) {
    let current = host(app).autostart;
    let want = autostart_mode(app);
    if DEV || current == Mode::Off {
        return;
    }
    // Moved or reinstalled elsewhere: point the task at this copy, which a normal start then
    // hands over to without a UAC prompt (see prepare_launch).
    if current == Mode::Task && want == Mode::Task && !autostart::task_runs_this() {
        return set_autostart(app, Mode::Task);
    }
    if current == want {
        return;
    }
    if want == Mode::Task || elevate::is_elevated() {
        set_autostart(app, want);
    }
}

/// Turning it on restarts KeyCat elevated behind the UAC prompt; turning it off takes effect the
/// next time KeyCat starts, since an elevated program cannot drop its rights.
fn toggle_admin(app: &AppHandle) {
    let on = !host(app).settings().run_as_admin;
    host(app).store.set("runAsAdmin", json!(on));
    if on && !elevate::is_elevated() {
        match elevate::relaunch(&format!("--after {}", std::process::id())) {
            Ok(()) => {
                log("restarting as administrator");
                return app.exit(0);
            }
            Err(error) => {
                log(&format!("administrator restart declined: {error}"));
                host(app).store.set("runAsAdmin", json!(false));
            }
        }
    }
    sync_autostart(app);
}

/// Before anything else starts: a restart waits for the copy that launched it to quit (or
/// single-instance would close it), and with the administrator setting on, a normal start hands
/// over to an elevated one: through the login task when there is one, which asks nothing,
/// otherwise behind the UAC prompt. Returns false when this process should quit at once.
fn prepare_launch() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if let Some(pid) = args.iter().position(|arg| arg == "--after").and_then(|i| args.get(i + 1)?.parse().ok()) {
        elevate::wait_for_exit(pid);
        return true;
    }
    if DEV || elevate::is_elevated() {
        return true;
    }
    let Some(dir) = std::env::var_os("APPDATA") else { return true };
    let store = Store::open(PathBuf::from(dir).join(IDENTIFIER).join("settings.json"));
    // Declined, KeyCat still runs, just without key presses from elevated windows.
    store.get("runAsAdmin") != Some(&Value::Bool(true)) || (!autostart::run_task() && elevate::relaunch("").is_err())
}

// MARK: Settings window

fn open_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(SETTINGS) {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let height = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| f64::from(monitor.work_area().size.height) / monitor.scale_factor() - 60.0)
        .unwrap_or(860.0)
        .min(860.0);
    let built = WebviewWindowBuilder::new(app, SETTINGS, WebviewUrl::App("settings.html".into()))
        .title("KeyCat 调节")
        .inner_size(420.0, height)
        .resizable(false)
        .maximizable(false)
        .center()
        .initialization_script(BRIDGE)
        .on_page_load(|window, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let tune = host(window.app_handle()).settings().tune;
                let _ = window.eval(format!("initTune({tune})"));
            }
        })
        .build();
    if let Err(error) = built {
        log(&format!("settings window failed: {error}"));
    }
}

// MARK: Status & updates

fn start_status_reports(app: &AppHandle) {
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(2));
        loop {
            // The page counts frames into cat.state.ticks; reset it, then read it a second later.
            let app2 = app.clone();
            let _ = app.run_on_main_thread(move || js(&app2, "cat.state.ticks = 0"));
            thread::sleep(Duration::from_secs(1));
            let app2 = app.clone();
            let _ = app.run_on_main_thread(move || {
                js(&app2, "window.webkit.messageHandlers.cat.postMessage({ ticks: cat.state.ticks })")
            });
            thread::sleep(Duration::from_secs(14));
        }
    });
}

fn write_status(app: &AppHandle, ticks: f64) {
    let (hook_ready, key_events) = {
        let h = host(app);
        (h.hook_ready, h.key_events)
    };
    let line = format!(
        "{} keyboard={} keyEvents={key_events} fps={}\n",
        timestamp(),
        if hook_ready { "active" } else { "missing" },
        ticks.round()
    );
    let _ = fs::write(data_dir().join("status.txt"), line);
}

fn set_update(app: &AppHandle, state: UpdateState) {
    let changed = {
        let mut h = host(app);
        let changed = h.update != state;
        h.update = state;
        changed
    };
    if changed {
        refresh_menu(app);
    }
}

/// The check gives up after 10 s, so a network that cannot reach GitHub (common in mainland
/// China) fails fast instead of hanging in 正在检查更新…. The download gets longer, a few MB on
/// a slow line.
const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const UPDATE_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Checks for a new version and downloads it. `manual` is a click on 检查更新…: only then does a
/// failure show in the menu. The timed checks fail quietly and are retried sooner (see
/// start_update_checks). Returns false if the check or the download failed.
async fn check_for_updates(app: AppHandle, manual: bool) -> bool {
    if DEV {
        return true;
    }
    {
        let mut h = host(&app);
        if matches!(h.update, UpdateState::Checking | UpdateState::Downloading(..) | UpdateState::Ready(_)) {
            return true;
        }
        h.update = UpdateState::Checking;
    }
    refresh_menu(&app);
    let failed = if manual { UpdateState::Failed } else { UpdateState::Idle };
    let found = match app.updater_builder().timeout(UPDATE_CHECK_TIMEOUT).build() {
        Ok(updater) => updater.check().await,
        Err(error) => Err(error),
    };
    let mut update = match found {
        Ok(Some(update)) => update,
        Ok(None) => {
            set_update(&app, UpdateState::Current);
            return true;
        }
        Err(error) => {
            log(&format!("update check failed: {error}"));
            set_update(&app, failed);
            return false;
        }
    };
    update.timeout = Some(UPDATE_DOWNLOAD_TIMEOUT);
    let version = update.version.clone();
    log(&format!("update {version} available"));
    set_update(&app, UpdateState::Downloading(version.clone(), 0));
    let (progress_app, progress_version) = (app.clone(), version.clone());
    let mut received = 0u64;
    let downloaded = update
        .download(
            move |chunk, total| {
                received += chunk as u64;
                if let Some(total) = total.filter(|total| *total > 0) {
                    let percent = (received * 100 / total).min(100) as u32;
                    set_update(&progress_app, UpdateState::Downloading(progress_version.clone(), percent));
                }
            },
            || {},
        )
        .await;
    match downloaded {
        Ok(bytes) => {
            log(&format!("update {version} downloaded"));
            host(&app).downloaded = Some((update, bytes));
            set_update(&app, UpdateState::Ready(version.clone()));
            if let Some(tray) = app.tray_by_id(TRAY) {
                let _ = tray.set_tooltip(Some(format!("KeyCat：v{version} 已下载，可从菜单重启更新")));
            }
            true
        }
        Err(error) => {
            log(&format!("update download failed: {error}"));
            set_update(&app, failed);
            false
        }
    }
}

/// Runs the downloaded installer, which closes this process and starts the new version.
fn install_update(app: &AppHandle) {
    let Some((update, bytes)) = host(app).downloaded.take() else { return };
    log(&format!("installing update {}", update.version));
    if let Err(error) = update.install(bytes) {
        log(&format!("update install failed: {error}"));
        set_update(app, UpdateState::Failed);
    }
}

fn start_update_checks(app: &AppHandle) {
    let app = app.clone();
    thread::spawn(move || {
        const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
        thread::sleep(Duration::from_secs(15));
        // After a failure the next try comes in 30 min, then 1 h, 2 h, 4 h, and from then on
        // every 6 h as usual, so a short outage is caught up soon and a blocked network is not
        // asked over and over.
        let mut retry = Duration::from_secs(30 * 60);
        loop {
            let wait = if tauri::async_runtime::block_on(check_for_updates(app.clone(), false)) {
                retry = Duration::from_secs(30 * 60);
                EVERY
            } else {
                let wait = retry;
                retry = (retry * 2).min(EVERY);
                log(&format!("next update check in {} min", wait.as_secs() / 60));
                wait
            };
            thread::sleep(wait);
        }
    });
}

// MARK: Lifecycle

fn setup(app: &AppHandle) -> tauri::Result<()> {
    let dir = app.path().app_data_dir()?;
    let _ = DATA_DIR.set(dir.clone());
    app.manage(Mutex::new(Host {
        store: Store::open(dir.join("settings.json")),
        page_ready: false,
        hook_ready: false,
        hit: "none".into(),
        petting: false,
        drag: None,
        pending_delta: (0.0, 0.0),
        last_pointer: None,
        last_cursor: (-9999, -9999),
        size_fixes: 0,
        key_events: 0,
        held: HashSet::new(),
        autostart: if DEV { Mode::Off } else { autostart::current() },
        update: UpdateState::Idle,
        downloaded: None,
        page_answered: false,
        page_failed: false,
    }));
    // Without its window KeyCat still starts, so the tray menu can say what went wrong.
    if let Err(error) = create_panel(app) {
        log(&format!("panel failed: {error}"));
        host(app).page_failed = true;
    }
    TrayIconBuilder::with_id(TRAY)
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .tooltip("KeyCat")
        .show_menu_on_left_click(true)
        .build(app)?;
    start_pump(app);
    // Built after the hook came up, so a failed hook shows in the menu.
    rebuild_menu(app);
    start_update_checks(app);
    sync_autostart(app);
    log(&format!("started v{} elevated={}", app.package_info().version, elevate::is_elevated()));
    Ok(())
}

fn main() {
    if !prepare_launch() {
        return;
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(panel) = panel(app) {
                let _ = panel.show();
            }
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![host_message])
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .setup(|app| Ok(setup(app.handle())?))
        .build(tauri::generate_context!())
        .expect("failed to start KeyCat")
        .run(|_app, event| {
            // The app lives in the tray; closing the settings window must not quit it.
            if let RunEvent::ExitRequested { api, code: None, .. } = event {
                api.prevent_exit();
            }
        });
}
