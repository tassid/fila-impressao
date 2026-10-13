#![windows_subsystem = "windows"]

mod autostart;
mod device;
mod force;
mod spooler;
mod status;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::image::Image;
use tauri::menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Rect, State, WindowEvent};

const PANEL: &str = "panel";
const TRAY: &str = "main";
const TRAY_REFRESH: Duration = Duration::from_secs(3);
// A click on the tray icon first blurs the panel (hiding it) and then arrives
// as a click; without this pause that click would reopen it straight away.
const REOPEN_GUARD: Duration = Duration::from_millis(250);
const MARGIN: f64 = 12.0;

#[derive(Default)]
struct AppState {
    selected: Mutex<String>,
    /// Keeps the panel open while the UAC prompt has the focus.
    pinned: AtomicBool,
    hidden_at: Mutex<Option<Instant>>,
}

#[derive(Serialize)]
struct JobView {
    id: u32,
    document: String,
    status: String,
    pages: u32,
    problem: bool,
}

#[derive(Serialize, Default)]
struct Snapshot {
    printers: Vec<String>,
    selected: String,
    /// `None` for queues with no physical device (PDF, XPS).
    connected: Option<bool>,
    status: String,
    problem: bool,
    printing: bool,
    jobs: Vec<JobView>,
    error: Option<String>,
}

fn pick_printer(printers: &[String]) -> String {
    let hp = printers.iter().find(|p| p.to_lowercase().contains("hp")).cloned();
    let default = spooler::default_printer().filter(|d| printers.contains(d));
    hp.or(default).or_else(|| printers.first().cloned()).unwrap_or_default()
}

fn take_snapshot(state: &AppState) -> Snapshot {
    let mut snapshot = Snapshot::default();
    match spooler::list_printers() {
        Ok(printers) => snapshot.printers = printers,
        Err(error) => {
            snapshot.status = "Spooler não responde".to_string();
            snapshot.problem = true;
            snapshot.error = Some(error.to_string());
            return snapshot;
        }
    }

    let mut selected = state.selected.lock().unwrap();
    if !snapshot.printers.contains(&selected) {
        *selected = pick_printer(&snapshot.printers);
    }
    snapshot.selected = selected.clone();
    drop(selected);
    if snapshot.selected.is_empty() {
        snapshot.status = "Nenhuma impressora instalada".to_string();
        return snapshot;
    }

    snapshot.connected = device::is_connected(&snapshot.selected);
    match spooler::printer_state(&snapshot.selected) {
        Ok(printer) => {
            snapshot.status = status::printer_status_text(printer.status, printer.attributes).to_string();
            snapshot.problem = status::printer_has_problem(printer.status, printer.attributes);
            snapshot.printing = status::printer_is_printing(printer.status);
        }
        Err(error) => {
            snapshot.status = "Erro ao ler a impressora".to_string();
            snapshot.problem = true;
            snapshot.error = Some(error.to_string());
        }
    }
    if snapshot.connected == Some(false) {
        snapshot.status = "Desconectada".to_string();
        snapshot.printing = false;
    }

    snapshot.jobs = spooler::list_jobs(&snapshot.selected)
        .unwrap_or_default()
        .into_iter()
        .map(|job| JobView {
            id: job.id,
            status: status::job_status_text(job.status, &job.status_text),
            problem: status::job_has_problem(job.status),
            document: job.document,
            pages: job.total_pages,
        })
        .collect();
    snapshot
}

fn selected_printer(state: &AppState) -> String {
    state.selected.lock().unwrap().clone()
}

#[tauri::command]
fn snapshot(state: State<AppState>) -> Snapshot {
    take_snapshot(&state)
}

#[tauri::command]
fn select_printer(name: String, state: State<AppState>) {
    *state.selected.lock().unwrap() = name;
}

#[tauri::command]
fn cancel_job(id: u32, state: State<AppState>) -> Result<(), String> {
    spooler::cancel_job(&selected_printer(&state), id).map_err(|e| format!("Não consegui cancelar: {e}"))
}

/// Returns how many jobs the spooler refused to cancel.
#[tauri::command]
fn clear_all(state: State<AppState>) -> Result<usize, String> {
    spooler::clear_all(&selected_printer(&state)).map_err(|e| format!("Não consegui limpar: {e}"))
}

#[tauri::command]
async fn force_clear(app: AppHandle) -> Result<String, String> {
    let was_pinned = app.state::<AppState>().pinned.swap(true, Ordering::SeqCst);
    let result = tauri::async_runtime::spawn_blocking(force::run_elevated).await;
    app.state::<AppState>().pinned.store(was_pinned, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(PANEL) {
        let _ = window.set_focus();
    }
    result.map_err(|e| format!("Limpeza forçada falhou: {e}"))?
}

#[tauri::command]
fn get_autostart() -> bool {
    autostart::is_enabled()
}

#[tauri::command]
fn set_autostart(enabled: bool) -> Result<(), String> {
    autostart::set_enabled(enabled).map_err(|e| format!("Não consegui alterar a inicialização: {e}"))
}

#[tauri::command]
fn hide_panel(app: AppHandle) {
    hide(&app);
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(PANEL) {
        let _ = window.hide();
        *app.state::<AppState>().hidden_at.lock().unwrap() = Some(Instant::now());
    }
}

/// Puts the panel next to the tray icon, or in the bottom-right corner when
/// there is no icon position to anchor to.
fn place(app: &AppHandle, anchor: Option<Rect>) {
    let Some(window) = app.get_webview_window(PANEL) else { return };
    let Ok(Some(monitor)) = window.primary_monitor() else { return };
    let Ok(size) = window.outer_size() else { return };
    let scale = monitor.scale_factor();
    let (width, height) = (f64::from(size.width), f64::from(size.height));
    let left = f64::from(monitor.position().x);
    let top = f64::from(monitor.position().y);
    let right = left + f64::from(monitor.size().width);
    let bottom = top + f64::from(monitor.size().height);
    let margin = MARGIN * scale;

    let (x, y) = match anchor {
        Some(rect) => {
            let icon = rect.position.to_physical::<f64>(scale);
            let icon_size = rect.size.to_physical::<f64>(scale);
            let above = icon.y - height - margin;
            let y = if above >= top { above } else { icon.y + icon_size.height + margin };
            (icon.x + icon_size.width / 2.0 - width / 2.0, y)
        }
        // Leaves room for a bottom taskbar.
        None => (right - width - margin, bottom - height - 60.0 * scale),
    };
    let x = x.clamp(left + margin, (right - width - margin).max(left + margin));
    let y = y.clamp(top + margin, (bottom - height - margin).max(top + margin));
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

fn show(app: &AppHandle, anchor: Option<Rect>) {
    let Some(window) = app.get_webview_window(PANEL) else { return };
    place(app, anchor);
    let _ = window.show();
    let _ = window.set_focus();
    let _ = app.emit("panel-shown", ());
}

fn toggle(app: &AppHandle, anchor: Option<Rect>) {
    let Some(window) = app.get_webview_window(PANEL) else { return };
    if window.is_visible().unwrap_or(false) {
        hide(app);
        return;
    }
    let just_hidden = app
        .state::<AppState>()
        .hidden_at
        .lock()
        .unwrap()
        .is_some_and(|at| at.elapsed() < REOPEN_GUARD);
    if !just_hidden {
        show(app, anchor);
    }
}

fn tray_tooltip(snapshot: &Snapshot) -> String {
    let queue = match snapshot.jobs.len() {
        0 => "fila vazia".to_string(),
        n => format!("{n} na fila"),
    };
    format!("Fila de Impressão · {queue}")
}

/// Keeps the tray icon and tooltip in step with the queue while the panel is closed.
fn watch_queue(app: AppHandle) {
    let idle = Image::from_bytes(include_bytes!("../icons/tray.png"));
    let busy = Image::from_bytes(include_bytes!("../icons/tray-busy.png"));
    let mut last = None;
    loop {
        let snapshot = take_snapshot(&app.state::<AppState>());
        let current = (snapshot.jobs.len(), tray_tooltip(&snapshot));
        if last.as_ref() != Some(&current) {
            if let Some(tray) = app.tray_by_id(TRAY) {
                let icon = if current.0 > 0 { &busy } else { &idle };
                let _ = tray.set_icon(icon.as_ref().ok().cloned());
                let _ = tray.set_tooltip(Some(&current.1));
            }
            last = Some(current);
        }
        thread::sleep(TRAY_REFRESH);
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItemBuilder::with_id("open", "Abrir").build(app)?;
    let clear = MenuItemBuilder::with_id("clear", "Limpar fila").build(app)?;
    let startup = CheckMenuItemBuilder::with_id("autostart", "Iniciar com o Windows")
        .checked(autostart::is_enabled())
        .build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Sair").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&open, &clear])
        .separator()
        .item(&startup)
        .separator()
        .item(&quit)
        .build()?;

    TrayIconBuilder::with_id(TRAY)
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .tooltip("Fila de Impressão")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => show(app, None),
            "clear" => {
                let _ = spooler::clear_all(&selected_printer(&app.state::<AppState>()));
            }
            "autostart" => {
                let wanted = startup.is_checked().unwrap_or(false);
                if autostart::set_enabled(wanted).is_err() {
                    let _ = startup.set_checked(!wanted);
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                toggle(tray.app_handle(), Some(rect));
            }
        })
        .build(app)?;
    Ok(())
}

fn main() {
    // Relaunched elevated by "Forçar": no window, just do the job.
    if std::env::args().any(|a| a == force::FORCE_ARG) {
        std::process::exit(force::elevated_main());
    }
    let start_hidden = std::env::args().any(|a| a == autostart::BACKGROUND_ARG);
    // Development aid: the panel stays open when it loses focus.
    let keep_open = std::env::args().any(|a| a == "--pin");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show(app, None)))
        .manage(AppState { pinned: AtomicBool::new(keep_open), ..AppState::default() })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            select_printer,
            cancel_job,
            clear_all,
            force_clear,
            get_autostart,
            set_autostart,
            hide_panel,
            quit
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            build_tray(&handle)?;

            if let Some(window) = app.get_webview_window(PANEL) {
                let app = handle.clone();
                window.on_window_event(move |event| {
                    let pinned = app.state::<AppState>().pinned.load(Ordering::SeqCst);
                    if matches!(event, WindowEvent::Focused(false)) && !pinned {
                        hide(&app);
                    }
                });
            }

            let watcher = handle.clone();
            thread::spawn(move || watch_queue(watcher));
            if !start_hidden {
                show(&handle, None);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Fila de Impressão");
}
