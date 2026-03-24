//! USB Printer communication and job management subsystem.
//!
//! Supports two backends selected at runtime via config:
//!
//! - `"usb"`      — sends ESC/POS commands to a physical USB thermal printer.
//! - `"terminal"` — renders the job as ASCII art to stdout; no hardware needed.

pub mod printer_error;
pub mod printer_prelude;

use crate::printer_prelude::*;
use once_cell::sync::OnceCell;
use std::sync::Mutex;

/// Width (in characters) of the terminal receipt rendering.
/// Also used by callers to format separator lines.
pub const TERMINAL_WIDTH: usize = 48;

// ---------------------------------------------------------------------------
// Backend enum
// ---------------------------------------------------------------------------

enum PrinterBackend {
    Usb(Mutex<escpos::printer::Printer<escpos::driver::NativeUsbDriver>>),
    Terminal,
}

// ---------------------------------------------------------------------------
// PrinterManager
// ---------------------------------------------------------------------------

pub struct PrinterManager {
    backend: PrinterBackend,
}

static PRINTER_MANAGER: OnceCell<PrinterManager> = OnceCell::new();

impl PrinterManager {
    fn init_usb(vid: u16, pid: u16) -> PrinterLibResult<Self> {
        use escpos::driver::NativeUsbDriver;
        use escpos::printer::Printer;
        use escpos::utils::Protocol;

        let driver = NativeUsbDriver::open(vid, pid)?;
        let mut printer = Printer::new(driver, Protocol::default(), None);
        printer.init()?;

        Ok(PrinterManager {
            backend: PrinterBackend::Usb(Mutex::new(printer)),
        })
    }

    fn init_terminal() -> Self {
        PrinterManager {
            backend: PrinterBackend::Terminal,
        }
    }

    /// Returns the globally initialized manager.  Panics if called before `init`.
    pub fn get() -> &'static PrinterManager {
        PRINTER_MANAGER
            .get()
            .expect("PrinterManager is not initialized")
    }

    pub fn execute_job(&self, job: PrintJob) -> PrinterLibResult {
        match &self.backend {
            PrinterBackend::Usb(mutex) => Self::execute_usb(mutex, job),
            PrinterBackend::Terminal => {
                Self::execute_terminal(job);
                Ok(())
            }
        }
    }

    // --- USB path ---

    fn execute_usb(
        mutex: &Mutex<escpos::printer::Printer<escpos::driver::NativeUsbDriver>>,
        job: PrintJob,
    ) -> PrinterLibResult {
        use escpos::ui::line::*;

        let mut printer = mutex.lock().map_err(|e| {
            PrinterLibError::CannotInitialize(format!("Failed to lock printer mutex: {}", e))
        })?;

        info!("Executing USB print job (Title: {})", job.title);

        let line = LineBuilder::new().style(LineStyle::Custom("=-")).build();

        printer.feeds(1)?;
        printer.size(2, 2)?;
        printer.writeln(&job.title)?;
        printer.feeds(1)?;
        printer.size(1, 1)?;
        printer.writeln(&job.origin)?;
        printer.feeds(1)?;
        printer.draw_line(line)?;

        for l in &job.lines {
            printer.writeln(l)?;
        }

        printer.feeds(2)?;
        printer.print_cut()?;

        Ok(())
    }

    // --- Terminal path ---

    fn execute_terminal(job: PrintJob) {
        info!("Executing terminal print job (Title: {})", job.title);

        let inner = TERMINAL_WIDTH;
        let total = inner + 2; // borders

        // Box-drawing chars
        let top    = format!("╔{}╗", "═".repeat(inner));
        let mid    = format!("╠{}╣", "═".repeat(inner));
        let bottom = format!("╚{}╝", "═".repeat(inner));
        let empty  = format!("║{}║", " ".repeat(inner));

        let pad = |s: &str| {
            let truncated: String = s.chars().take(inner).collect();
            format!("║ {:<width$}║", truncated, width = inner - 1)
        };

        println!();
        println!("{}", top);
        println!("{}", pad(&job.title));
        println!("{}", pad(&job.origin));
        println!("{}", mid);
        for line in &job.lines {
            if line.is_empty() {
                println!("{}", empty);
            } else {
                // Wrap long lines at inner-1 chars
                let chars: Vec<char> = line.chars().collect();
                for chunk in chars.chunks(inner - 1) {
                    let s: String = chunk.iter().collect();
                    println!("{}", pad(&s));
                }
            }
        }
        println!("{}", bottom);
        println!();
    }
}

// ---------------------------------------------------------------------------
// PrintJob
// ---------------------------------------------------------------------------

pub struct PrintJob {
    pub origin: String,
    pub title: String,
    pub lines: Vec<String>,
}

impl PrintJob {
    pub fn new(origin: String, title: String, lines: Vec<String>) -> Self {
        PrintJob { origin, title, lines }
    }

    /// Executes the print job via the globally initialized backend.
    /// `_vid` and `_pid` are ignored (connection is managed globally).
    pub async fn execute(self, _vid: u16, _pid: u16) -> PrinterLibResult {
        PrinterManager::get().execute_job(self)
    }
}

// ---------------------------------------------------------------------------
// Public init
// ---------------------------------------------------------------------------

/// Initialises the printer subsystem.
///
/// `mode` should be `"usb"` (physical printer) or `"terminal"` (stdout rendering).
/// Any unrecognised value is treated as `"terminal"`.
pub fn init(vid: u16, pid: u16, mode: &str) -> PrinterLibResult {
    info!("Initializing printer (mode: {})", mode);

    let manager = if mode == "usb" {
        match PrinterManager::init_usb(vid, pid) {
            Ok(m) => {
                info!(
                    "USB printer initialized (VID: 0x{:04x}, PID: 0x{:04x})",
                    vid, pid
                );
                m
            }
            Err(e) => {
                error!(
                    "Failed to open USB printer (VID: 0x{:04x}, PID: 0x{:04x}): {}",
                    vid, pid, e
                );
                return Err(e);
            }
        }
    } else {
        info!("Printer running in terminal (dummy) mode — output goes to stdout");
        PrinterManager::init_terminal()
    };

    PRINTER_MANAGER
        .set(manager)
        .map_err(|_| PrinterLibError::CannotInitialize("Printer already initialized".to_string()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_mode_init_succeeds() {
        // Terminal mode never needs hardware, so this must always pass.
        // (We can't call init() here because OnceCell is already set in other
        // test runs, but we can exercise the manager constructor directly.)
        let mgr = PrinterManager::init_terminal();
        let job = PrintJob::new(
            "test".into(),
            "TEST TICKET".into(),
            vec!["Line 1".into(), "Line 2".into()],
        );
        assert!(mgr.execute_job(job).is_ok());
    }
}
