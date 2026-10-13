//! Safe wrappers over the Windows print spooler API (winspool.drv).

use std::ffi::c_void;
use std::io;
use std::ptr;

type Handle = *mut c_void;

const PRINTER_ENUM_LOCAL: u32 = 0x2;
const PRINTER_ENUM_CONNECTIONS: u32 = 0x4;
const JOB_CONTROL_DELETE: u32 = 5;
const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
const MAX_JOBS: u32 = 1000;

#[repr(C)]
struct PrinterInfo4 {
    printer_name: *const u16,
    server_name: *const u16,
    attributes: u32,
}

#[repr(C)]
struct PrinterInfo2 {
    // pServerName .. pSecurityDescriptor: not needed here.
    pointers: [*const c_void; 13],
    attributes: u32,
    priority: u32,
    default_priority: u32,
    start_time: u32,
    until_time: u32,
    status: u32,
    jobs: u32,
    average_ppm: u32,
}

#[repr(C)]
struct JobInfo1 {
    job_id: u32,
    printer_name: *const u16,
    machine_name: *const u16,
    user_name: *const u16,
    document: *const u16,
    datatype: *const u16,
    status_text: *const u16,
    status: u32,
    priority: u32,
    position: u32,
    total_pages: u32,
    pages_printed: u32,
    submitted: [u16; 8],
}

#[link(name = "winspool")]
extern "system" {
    fn EnumPrintersW(
        flags: u32,
        name: *const u16,
        level: u32,
        buffer: *mut u8,
        size: u32,
        needed: *mut u32,
        returned: *mut u32,
    ) -> i32;
    fn GetDefaultPrinterW(buffer: *mut u16, size: *mut u32) -> i32;
    fn OpenPrinterW(name: *const u16, handle: *mut Handle, defaults: *const c_void) -> i32;
    fn ClosePrinter(handle: Handle) -> i32;
    fn GetPrinterW(handle: Handle, level: u32, buffer: *mut u8, size: u32, needed: *mut u32) -> i32;
    fn EnumJobsW(
        handle: Handle,
        first: u32,
        count: u32,
        level: u32,
        buffer: *mut u8,
        size: u32,
        needed: *mut u32,
        returned: *mut u32,
    ) -> i32;
    fn SetJobW(handle: Handle, job_id: u32, level: u32, job: *mut u8, command: u32) -> i32;
}

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub id: u32,
    pub document: String,
    pub status: u32,
    pub status_text: String,
    pub total_pages: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrinterState {
    pub status: u32,
    pub attributes: u32,
}

fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Reads a NUL-terminated UTF-16 string owned by a spooler buffer.
unsafe fn from_wide(text: *const u16) -> String {
    if text.is_null() {
        return String::new();
    }
    let mut len = 0;
    while *text.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(text, len))
}

/// Runs a "tell me the size, then fill the buffer" spooler call.
/// Returns the buffer (u64 so the structs inside are aligned) and the item count.
fn fill_buffer(mut call: impl FnMut(*mut u8, u32, &mut u32, &mut u32) -> i32) -> io::Result<(Vec<u64>, u32)> {
    let mut needed = 0;
    let mut returned = 0;
    if call(ptr::null_mut(), 0, &mut needed, &mut returned) != 0 {
        return Ok((Vec::new(), 0));
    }
    // The queue can grow between the two calls, so retry with the new size.
    for _ in 0..4 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER) {
            return Err(error);
        }
        let mut buffer = vec![0u64; needed as usize / 8 + 1];
        let size = (buffer.len() * 8) as u32;
        if call(buffer.as_mut_ptr().cast(), size, &mut needed, &mut returned) != 0 {
            return Ok((buffer, returned));
        }
    }
    Err(io::Error::last_os_error())
}

struct Printer(Handle);

impl Printer {
    fn open(name: &str) -> io::Result<Self> {
        let wide = to_wide(name);
        let mut handle = ptr::null_mut();
        if unsafe { OpenPrinterW(wide.as_ptr(), &mut handle, ptr::null()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(handle))
    }
}

impl Drop for Printer {
    fn drop(&mut self) {
        unsafe { ClosePrinter(self.0) };
    }
}

pub fn list_printers() -> io::Result<Vec<String>> {
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let (buffer, count) = fill_buffer(|buf, size, needed, returned| unsafe {
        EnumPrintersW(flags, ptr::null(), 4, buf, size, needed, returned)
    })?;
    let items = buffer.as_ptr().cast::<PrinterInfo4>();
    Ok((0..count as usize)
        .map(|i| unsafe { from_wide((*items.add(i)).printer_name) })
        .collect())
}

pub fn default_printer() -> Option<String> {
    let mut buffer = [0u16; 512];
    let mut size = buffer.len() as u32;
    if unsafe { GetDefaultPrinterW(buffer.as_mut_ptr(), &mut size) } == 0 {
        return None;
    }
    Some(unsafe { from_wide(buffer.as_ptr()) })
}

pub fn printer_state(name: &str) -> io::Result<PrinterState> {
    let printer = Printer::open(name)?;
    let (buffer, _) = fill_buffer(|buf, size, needed, _| unsafe {
        GetPrinterW(printer.0, 2, buf, size, needed)
    })?;
    if buffer.is_empty() {
        return Err(io::Error::other("spooler não retornou dados da impressora"));
    }
    let info = unsafe { &*buffer.as_ptr().cast::<PrinterInfo2>() };
    Ok(PrinterState { status: info.status, attributes: info.attributes })
}

pub fn list_jobs(name: &str) -> io::Result<Vec<Job>> {
    let printer = Printer::open(name)?;
    let (buffer, count) = fill_buffer(|buf, size, needed, returned| unsafe {
        EnumJobsW(printer.0, 0, MAX_JOBS, 1, buf, size, needed, returned)
    })?;
    let items = buffer.as_ptr().cast::<JobInfo1>();
    Ok((0..count as usize)
        .map(|i| unsafe {
            let job = &*items.add(i);
            Job {
                id: job.job_id,
                document: from_wide(job.document),
                status: job.status,
                status_text: from_wide(job.status_text),
                total_pages: job.total_pages,
            }
        })
        .collect())
}

pub fn cancel_job(name: &str, job_id: u32) -> io::Result<()> {
    let printer = Printer::open(name)?;
    if unsafe { SetJobW(printer.0, job_id, 0, ptr::null_mut(), JOB_CONTROL_DELETE) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Cancels every job in the queue. Returns how many the spooler refused.
pub fn clear_all(name: &str) -> io::Result<usize> {
    let jobs = list_jobs(name)?;
    Ok(jobs.iter().filter(|job| cancel_job(name, job.id).is_err()).count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    const JOB_CONTROL_PAUSE: u32 = 1;
    // A paused job here never reaches the port, so nothing is printed or saved.
    const TEST_PRINTER: &str = "Microsoft Print to PDF";

    #[repr(C)]
    struct DocInfo1 {
        doc_name: *const u16,
        output_file: *const u16,
        datatype: *const u16,
    }

    #[link(name = "winspool")]
    extern "system" {
        fn StartDocPrinterW(handle: Handle, level: u32, doc_info: *const DocInfo1) -> u32;
        fn WritePrinter(handle: Handle, data: *const c_void, size: u32, written: *mut u32) -> i32;
        fn EndDocPrinter(handle: Handle) -> i32;
    }

    fn add_paused_job(document: &str) -> u32 {
        let printer = Printer::open(TEST_PRINTER).expect("abrir impressora de teste");
        let doc_name = to_wide(document);
        let datatype = to_wide("RAW");
        let info = DocInfo1 {
            doc_name: doc_name.as_ptr(),
            output_file: ptr::null(),
            datatype: datatype.as_ptr(),
        };
        let id = unsafe { StartDocPrinterW(printer.0, 1, &info) };
        assert_ne!(id, 0, "StartDocPrinter: {}", io::Error::last_os_error());
        let paused = unsafe { SetJobW(printer.0, id, 0, ptr::null_mut(), JOB_CONTROL_PAUSE) };
        assert_ne!(paused, 0, "pausar job: {}", io::Error::last_os_error());
        let data = b"teste";
        let mut written = 0;
        unsafe {
            WritePrinter(printer.0, data.as_ptr().cast(), data.len() as u32, &mut written);
            EndDocPrinter(printer.0);
        }
        id
    }

    fn wait_until_gone(id: u32) -> bool {
        for _ in 0..25 {
            if !list_jobs(TEST_PRINTER).unwrap().iter().any(|job| job.id == id) {
                return true;
            }
            sleep(Duration::from_millis(200));
        }
        false
    }

    #[test]
    fn lists_installed_printers() {
        let printers = list_printers().unwrap();
        assert!(printers.iter().any(|p| p == TEST_PRINTER), "{printers:?}");
        assert!(printer_state(TEST_PRINTER).is_ok());
    }

    #[test]
    fn unknown_printer_is_an_error() {
        assert!(list_jobs("impressora que não existe").is_err());
    }

    #[test]
    #[ignore = "cria jobs reais (pausados) na fila da Microsoft Print to PDF"]
    fn cancels_one_job_then_clears_the_rest() {
        let first = add_paused_job("fila-impressao teste 1");
        let second = add_paused_job("fila-impressao teste 2");

        let jobs = list_jobs(TEST_PRINTER).unwrap();
        let listed = jobs.iter().find(|job| job.id == first).expect("job na fila");
        assert_eq!(listed.document, "fila-impressao teste 1");

        cancel_job(TEST_PRINTER, first).unwrap();
        assert!(wait_until_gone(first), "job cancelado continua na fila");
        assert!(list_jobs(TEST_PRINTER).unwrap().iter().any(|job| job.id == second));

        assert_eq!(clear_all(TEST_PRINTER).unwrap(), 0);
        assert!(wait_until_gone(second), "limpar tudo deixou job na fila");
    }
}
