//! Tells whether a printer's hardware is plugged in right now.
//! The spooler keeps reporting "ready" for an unplugged USB printer, so this
//! asks the device manager about the device behind the print queue instead.

use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

const CR_SUCCESS: u32 = 0;
// Only finds devices that are currently present.
const CM_LOCATE_DEVNODE_NORMAL: u32 = 0;

#[link(name = "cfgmgr32")]
extern "system" {
    fn CM_Locate_DevNodeW(device: *mut u32, device_id: *const u16, flags: u32) -> u32;
}

/// Software-only queues (PDF, XPS, OneNote) have no cable to unplug.
fn is_hardware_instance(instance_id: &str) -> bool {
    let id = instance_id.to_ascii_uppercase();
    !id.is_empty() && !id.starts_with("SWD\\") && !id.starts_with("ROOT\\")
}

fn is_present(instance_id: &str) -> bool {
    let wide: Vec<u16> = instance_id.encode_utf16().chain(std::iter::once(0)).collect();
    let mut device = 0;
    unsafe { CM_Locate_DevNodeW(&mut device, wide.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) == CR_SUCCESS }
}

/// `None` when the printer has no physical device to check.
pub fn is_connected(printer: &str) -> Option<bool> {
    let path = format!(r"SYSTEM\CurrentControlSet\Control\Print\Printers\{printer}\PnPData");
    let key = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(path).ok()?;
    let instance_id: String = key.get_value("DeviceInstanceId").ok()?;
    is_hardware_instance(&instance_id).then(|| is_present(&instance_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usb_printers_are_hardware_and_virtual_queues_are_not() {
        assert!(is_hardware_instance(r"USBPRINT\HPDESKJET_F4400_SERIES\8&14B70E07&0&USB001"));
        assert!(!is_hardware_instance(r"SWD\PRINTENUM\{0A6AB7E4-2A3F-4B0F-9E4E-1B1E0B6B5A11}"));
        assert!(!is_hardware_instance(r"root\PRINTQUEUE\0000"));
        assert!(!is_hardware_instance(""));
    }

    #[test]
    fn missing_device_is_not_present() {
        assert!(!is_present(r"USBPRINT\NAO_EXISTE\0&00000000&0&USB999"));
    }

    #[test]
    fn unknown_printer_has_nothing_to_check() {
        assert_eq!(is_connected("impressora que não existe"), None);
    }
}
