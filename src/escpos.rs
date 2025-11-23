use crate::datatypes::{Project, Task};
use std::fs::File;
use std::io::{Result, Write};

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
        self.writer.write_all(buf)
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

    printer.text(&format!("Owner: {}", project.owner.name))?;
    printer.newline()?;
    printer.newline()?;

    printer.text("Views:")?;
    printer.newline()?;
    for view in &project.views {
        printer.text(&format!("- {}", view.title))?;
        printer.newline()?;
    }

    printer.cut()?;
    Ok(())
}

pub fn print_task(task: &Task, device_path: &str) -> Result<()> {
    let mut printer = Printer::new(device_path)?;
    printer.init()?;

    printer.align(Align::Center)?;
    printer.text(&task.title)?;
    printer.newline()?;
    printer.align(Align::Left)?;
    printer.newline()?;

    printer.text("Project: ")?;
    printer.text(&task.project.title)?;
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
