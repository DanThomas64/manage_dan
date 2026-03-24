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

/// Width (in characters) of the terminal receipt box.
/// The usable content area is `TERMINAL_WIDTH - 1` (one leading space inside the border).
pub const TERMINAL_WIDTH: usize = 48;

/// Wraps `text` at word boundaries so no line exceeds `width` characters.
///
/// Lines that already fit within `width` are passed through unchanged,
/// preserving any intentional spacing (e.g. separator lines, formatted rows).
/// A single word longer than `width` is placed on its own line as-is rather
/// than being split mid-character.
fn word_wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        // Fast path: line fits as-is — preserve it exactly.
        if line.chars().count() <= width {
            out.push(line.to_string());
            continue;
        }
        // Line is too long; break at word boundaries.
        let mut current = String::new();
        for word in line.split_whitespace() {
            if current.is_empty() {
                current.push_str(word);
            } else if current.len() + 1 + word.len() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                out.push(current.clone());
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Backend enum
// ---------------------------------------------------------------------------

enum PrinterBackend {
    Usb {
        printer: Mutex<escpos::printer::Printer<escpos::driver::NativeUsbDriver>>,
        /// Characters per line as reported by `PrinterOptions` at init time.
        chars_per_line: u8,
    },
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
    fn init_usb(vid: u16, pid: u16, chars_per_line: u8) -> PrinterLibResult<Self> {
        use escpos::driver::NativeUsbDriver;
        use escpos::printer::Printer;
        use escpos::printer_options::PrinterOptions;
        use escpos::utils::{PageCode, Protocol};

        let driver = NativeUsbDriver::open(vid, pid)?;

        // PC437 is the standard ESC/POS character page (ASCII + Latin supplement).
        let mut options = PrinterOptions::default();
        options.page_code(Some(PageCode::PC437));
        options.characters_per_line(chars_per_line);

        let mut printer = Printer::new(driver, Protocol::default(), Some(options));
        printer.init()?;
        printer.page_code(PageCode::PC437)?;

        Ok(PrinterManager {
            backend: PrinterBackend::Usb {
                printer: Mutex::new(printer),
                chars_per_line,
            },
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

    /// Returns the usable line width for the active backend.
    ///
    /// Callers should use this to format separator lines and right-align badges
    /// so that content spans the full receipt width on both USB and terminal.
    pub fn line_width(&self) -> usize {
        match &self.backend {
            PrinterBackend::Usb { chars_per_line, .. } => *chars_per_line as usize,
            // Terminal: one leading space is always added inside the box border.
            PrinterBackend::Terminal => TERMINAL_WIDTH - 1,
        }
    }

    pub fn execute_job(&self, job: PrintJob) -> PrinterLibResult {
        match &self.backend {
            PrinterBackend::Usb { printer, .. } => Self::execute_usb(printer, job),
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
        use escpos::utils::JustifyMode;

        let mut printer = mutex.lock().map_err(|e| {
            PrinterLibError::CannotInitialize(format!("Failed to lock printer mutex: {}", e))
        })?;

        info!("Executing USB print job (Title: {})", job.title);

        let width = printer.options().get_characters_per_line() as usize;

        printer.feeds(1)?;

        // Header — centred, bold + double-strike for extra weight on the title line.
        printer.justify(JustifyMode::CENTER)?;
        printer.bold(true)?;
        printer.double_strike(true)?;
        printer.writeln(&job.title)?;
        printer.double_strike(false)?;
        printer.bold(false)?;
        // Task name: slightly taller, underlined, and word-wrapped so it stands out.
        use escpos::utils::UnderlineMode;
        printer.size(1, 2)?;
        printer.underline(UnderlineMode::Single)?;
        for wrapped in word_wrap(&job.origin, width) {
            printer.writeln(&wrapped)?;
        }
        printer.underline(UnderlineMode::None)?;
        printer.size(1, 1)?;
        printer.writeln("")?;
        printer.justify(JustifyMode::LEFT)?;

        // Body — word-wrap each line so words are never split mid-way.
        for l in &job.lines {
            if l.is_empty() {
                printer.writeln("")?;
            } else {
                for wrapped in word_wrap(l, width) {
                    printer.writeln(&wrapped)?;
                }
            }
        }

        printer.feeds(2)?;
        printer.print_cut()?;

        Ok(())
    }

    // --- Terminal path ---

    fn execute_terminal(job: PrintJob) {
        info!("Executing terminal print job (Title: {})", job.title);

        let inner = TERMINAL_WIDTH;

        // Box-drawing chars
        let top    = format!("╔{}╗", "═".repeat(inner));
        let mid    = format!("╠{}╣", "═".repeat(inner));
        let bottom = format!("╚{}╝", "═".repeat(inner));
        let empty  = format!("║{}║", " ".repeat(inner));

        let pad = |s: &str| {
            let truncated: String = s.chars().take(inner - 1).collect();
            format!("║ {:<width$}║", truncated, width = inner - 1)
        };

        let content_width = inner - 1;

        println!();
        println!("{}", top);
        println!("{}", pad(&job.title));
        println!("{}", pad(&job.origin));
        // Blank line between the task title and the dividing border.
        println!("{}", empty);
        println!("{}", mid);
        for line in &job.lines {
            if line.is_empty() {
                println!("{}", empty);
            } else {
                for wrapped in word_wrap(line, content_width) {
                    println!("{}", pad(&wrapped));
                }
            }
        }
        println!("{}", bottom);
        println!();
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Returns the usable line width for the active printer backend.
///
/// Use this to format separator lines and right-align content so that output
/// spans the full receipt width regardless of whether the printer is USB or
/// terminal.  Panics if called before `init`.
pub fn line_width() -> usize {
    PrinterManager::get().line_width()
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
pub fn init(vid: u16, pid: u16, mode: &str, chars_per_line: u8) -> PrinterLibResult {
    info!("Initializing printer (mode: {}, chars_per_line: {})", mode, chars_per_line);

    let manager = if mode == "usb" {
        match PrinterManager::init_usb(vid, pid, chars_per_line) {
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
        let mgr = PrinterManager::init_terminal();
        let job = PrintJob::new(
            "test".into(),
            "TEST TICKET".into(),
            vec!["Line 1".into(), "Line 2".into()],
        );
        assert!(mgr.execute_job(job).is_ok());
    }
}
