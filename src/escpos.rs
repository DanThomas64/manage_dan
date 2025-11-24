use crate::datatypes::{Project, Task};
use anyhow::Result;
use std::fs::File;
use std::io::Write;

use escpos::utils::*;

// ESC/POS Commands
const ESC: u8 = 0x1B;
const GS: u8 = 0x1D;

const INIT: &[u8] = &[ESC, b'@'];
const CUT: &[u8] = &[GS, b'V', 0];
const LF: &[u8] = &[0x0A];

pub enum Align {
    Left,
    Center,
    Right,
}

pub struct Printer {
    writer: File,
}

impl Printer {
    pub fn new(device_path: &str) -> Result<Self> {
        let file = File::create(device_path)?;
        Ok(Printer { writer: file })
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        self.writer.write_all(buf)?;
        Ok(())
    }

    pub fn init(&mut self) -> Result<()> {
        self.write_all(INIT)
    }

    pub fn cut(&mut self) -> Result<()> {
        self.write_all(CUT)
    }

    pub fn text(&mut self, text: &str) -> Result<()> {
        self.write_all(text.as_bytes())
    }

    pub fn newline(&mut self) -> Result<()> {
        self.write_all(LF)
    }

    pub fn align(&mut self, align: Align) -> Result<()> {
        let align_byte = match align {
            Align::Left => 0,
            Align::Center => 1,
            Align::Right => 2,
        };
        self.write_all(&[ESC, b'a', align_byte])
    }
}

pub fn print_project(project: &Project, device_path: &str) -> Result<()> {
    let mut printer = Printer::new(device_path)?;
    printer.init()?;

    printer.align(Align::Center)?;
    printer.text(&project.title)?;
    printer.newline()?;
    printer.align(Align::Left)?;
    printer.newline()?;

    printer.text("Description:")?;
    printer.newline()?;
    printer.text(&project.description)?;
    printer.newline()?;
    printer.newline()?;

    printer.cut()?;
    Ok(())
}

pub fn print_task(task: &Task, device_path: &str) -> Result<()> {
    let mut printer = Printer::new(device_path)?;
    printer.init()?;

    printer.align(Align::Center)?;
    print_header(&mut printer, "Task")?;
    printer.text(&task.title)?;
    printer.newline()?;
    printer.align(Align::Left)?;
    printer.newline()?;

    printer.text("Project ID: ")?;
    printer.text(&task.project_id.to_string())?;
    printer.newline()?;
    printer.newline()?;

    printer.text("Description:")?;
    printer.newline()?;
    printer.text(&task.description)?;
    printer.newline()?;
    printer.newline()?;

    if let Some(labels) = &task.labels {
        if !labels.is_empty() {
            printer.text("Labels:")?;
            printer.newline()?;
            for label in labels {
                printer.text(&format!("- {}", label.title))?;
                printer.newline()?;
            }
        }
    }

    print_footer(&mut printer, "Task")?;

    printer.cut()?;
    Ok(())
}

fn print_header(printer: &mut Printer, print_type: &str) -> Result<()> {
    for _ in 1..5 {
        printer.newline()?;
    }
    printer.text(print_type)?;
    for _ in 1..5 {
        printer.newline()?;
    }
    Ok(())
}

fn print_footer(printer: &mut Printer, print_type: &str) -> Result<()> {
    printer.align(Align::Center)?;
    for _ in 1..5 {
        printer.newline()?;
    }
    printer.text(print_type)?;
    for _ in 1..10 {
        printer.newline()?;
    }
    Ok(())
}
