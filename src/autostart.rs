//! "Iniciar com o Windows": a per-user Run entry, no admin needed.

use std::{env, io};

use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
use winreg::RegKey;

/// Passed on login so the app starts in the tray without opening the panel.
pub const BACKGROUND_ARG: &str = "--background";

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "FilaDeImpressao";

pub fn is_enabled() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|key| key.get_value::<String, _>(VALUE))
        .is_ok()
}

pub fn set_enabled(enabled: bool) -> io::Result<()> {
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)?;
    if enabled {
        let exe = env::current_exe()?;
        key.set_value(VALUE, &format!("\"{}\" {BACKGROUND_ARG}", exe.display()))
    } else {
        match key.delete_value(VALUE) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}
