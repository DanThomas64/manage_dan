use crate::datatypes::{Project, Task};
use quick_error::quick_error;
use std::fs::File;
use std::io::{Error, Write};

quick_error! {
    #[derive(Debug)]
    pub enum PrintError {
        Io(err: Error) {
            from()
            display("I/O error: {}", err)
            cause(err)
        }
    }
}

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
    pub fn new(device_path: &str) -> Result<Self, PrintError> {
        let file = File::create(device_path)?;
        Ok(Printer { writer: file })
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<(), PrintError> {
        self.writer.write_all(buf)?;
        Ok(())
    }

    pub fn init(&mut self) -> Result<(), PrintError> {
        self.write_all(INIT)
    }

    pub fn cut(&mut self) -> Result<(), PrintError> {
        self.write_all(CUT)
    }

    pub fn text(&mut self, text: &str) -> Result<(), PrintError> {
        self.write_all(text.as_bytes())
    }

    pub fn newline(&mut self) -> Result<(), PrintError> {
        self.write_all(LF)
    }

    pub fn align(&mut self, align: Align) -> Result<(), PrintError> {
        let align_byte = match align {
            Align::Left => 0,
            Align::Center => 1,
            Align::Right => 2,
        };
        self.write_all(&[ESC, b'a', align_byte])
    }
}

pub fn print_project(project: &Project, device_path: &str) -> Result<(), PrintError> {
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

pub fn print_task(task: &Task, device_path: &str) -> Result<(), PrintError> {
    let mut printer = Printer::new(device_path)?;
    printer.init()?;

    printer.align(Align::Center)?;
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

    if !task.assignees.is_empty() {
        printer.text("Assignees:")?;
        printer.newline()?;
        for assignee in &task.assignees {
            printer.text(&format!("- {}", assignee.name))?;
            printer.newline()?;
        }
        printer.newline()?;
    }

    if !task.labels.is_empty() {
        printer.text("Labels:")?;
        printer.newline()?;
        for label in &task.labels {
            printer.text(&format!("- {}", label.title))?;
            printer.newline()?;
        }
    }

    printer.cut()?;
    Ok(())
}
