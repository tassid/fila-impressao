//! "Forçar": stop the spooler, delete its spool files, start it again.
//! Needs admin, so the app relaunches itself elevated with `FORCE_ARG`.

use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;
use std::{env, fs, io, mem};

pub const FORCE_ARG: &str = "--force";

const EXIT_OK: i32 = 0;
const EXIT_SPOOLER_DOWN: i32 = 2;
const EXIT_FILES_LEFT: i32 = 3;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const SEE_MASK_NOCLOSEPROCESS: u32 = 0x40;
const ERROR_CANCELLED: i32 = 1223;
const INFINITE: u32 = 0xFFFF_FFFF;

#[repr(C)]
struct ShellExecuteInfo {
    size: u32,
    mask: u32,
    hwnd: *mut c_void,
    verb: *const u16,
    file: *const u16,
    parameters: *const u16,
    directory: *const u16,
    show: i32,
    inst_app: *mut c_void,
    id_list: *mut c_void,
    class: *const u16,
    hkey_class: *mut c_void,
    hot_key: u32,
    icon_or_monitor: *mut c_void,
    process: *mut c_void,
}

#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteExW(info: *mut ShellExecuteInfo) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    fn GetExitCodeProcess(handle: *mut c_void, exit_code: *mut u32) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Asks for admin (UAC), runs the forced clean-up and waits for it.
/// Returns the message to show the user either way.
pub fn run_elevated() -> Result<String, String> {
    let exe = env::current_exe().map_err(|e| format!("Não achei o próprio executável: {e}"))?;
    let file = to_wide(&exe.to_string_lossy());
    let verb = to_wide("runas");
    let parameters = to_wide(FORCE_ARG);

    let mut info: ShellExecuteInfo = unsafe { mem::zeroed() };
    info.size = mem::size_of::<ShellExecuteInfo>() as u32;
    info.mask = SEE_MASK_NOCLOSEPROCESS;
    info.verb = verb.as_ptr();
    info.file = file.as_ptr();
    info.parameters = parameters.as_ptr();

    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = io::Error::last_os_error();
        return Err(if error.raw_os_error() == Some(ERROR_CANCELLED) {
            "Permissão de administrador negada. Nada foi alterado.".to_string()
        } else {
            format!("Não consegui pedir permissão de administrador: {error}")
        });
    }
    if info.process.is_null() {
        return Err("Não consegui acompanhar a limpeza forçada.".to_string());
    }

    let mut code = u32::MAX;
    unsafe {
        WaitForSingleObject(info.process, INFINITE);
        GetExitCodeProcess(info.process, &mut code);
        CloseHandle(info.process);
    }
    match code as i32 {
        EXIT_OK => Ok("Fila limpa. Spooler reiniciado.".to_string()),
        EXIT_FILES_LEFT => Err("Spooler reiniciado, mas alguns arquivos da fila não saíram.".to_string()),
        EXIT_SPOOLER_DOWN => Err("Spooler não voltou. Reinicie o PC ou rode como admin: net start spooler".to_string()),
        other => Err(format!("Limpeza forçada falhou (código {other}).")),
    }
}

fn run_hidden(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn spooler_is(state: &str) -> bool {
    run_hidden("sc", &["query", "spooler"]).is_some_and(|out| out.contains(state))
}

fn wait_for(state: &str) -> bool {
    for _ in 0..20 {
        if spooler_is(state) {
            return true;
        }
        sleep(Duration::from_millis(500));
    }
    false
}

/// Runs inside the elevated process. Whatever happens, tries to leave the spooler running.
pub fn elevated_main() -> i32 {
    run_hidden("net", &["stop", "spooler", "/y"]);
    let stopped = wait_for("STOPPED");

    let mut files_left = 0;
    if stopped {
        let root = env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let spool = PathBuf::from(root).join("System32").join("spool").join("PRINTERS");
        if let Ok(entries) = fs::read_dir(&spool) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && fs::remove_file(&path).is_err() {
                    files_left += 1;
                }
            }
        }
    }

    let mut running = false;
    for _ in 0..3 {
        run_hidden("net", &["start", "spooler"]);
        if wait_for("RUNNING") {
            running = true;
            break;
        }
    }

    if !running {
        EXIT_SPOOLER_DOWN
    } else if !stopped || files_left > 0 {
        EXIT_FILES_LEFT
    } else {
        EXIT_OK
    }
}
