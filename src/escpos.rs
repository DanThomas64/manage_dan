use crate::datatypes::{Project, Task};
use escpos::driver::FileDriver;
use escpos::printer::Printer as EscposPrinter;
use quick_error::quick_error;
use std::io::Error;

quick_error! {
    #[derive(Debug)]
    pub enum PrintError {
        Io(err: Error) {
            from()
            display("I/O error: {}", err)
        }
        Escpos(err: escpos::Error) {
            from()
            display("Printer error: {}", err)
        }
    }
}

pub struct Printer {
    printer: EscposPrinter<FileDriver>,
}

impl Printer {
    pub fn new(device_path: &str) -> Result<Self, PrintError> {
        let printer = EscposPrinter::new(device_path, None);
        Ok(Printer { printer })
    }

    pub fn init(&mut self) -> Result<(), PrintError> {
        self.printer.init()?;
        Ok(())
    }

    pub fn cut(&mut self) -> Result<(), PrintError> {
        self.printer.cut()?;
        Ok(())
    }

    pub fn text(&mut self, text: &str) -> Result<(), PrintError> {
        self.printer.raw(text.as_bytes())?;
        Ok(())
    }

    pub fn newline(&mut self) -> Result<(), PrintError> {
        self.printer.writeln("")?;
        Ok(())
    }

    pub fn align(&mut self, align: Align) -> Result<(), PrintError> {
        self.printer.justify(align)?;
        Ok(())
    }

    pub fn qrcode(&mut self, data: &str) -> Result<(), PrintError> {
        self.printer
            .qrcode(data, QrCodeModel::Model2, 5, QrCodeErrorCorrection::L)?;
        Ok(())
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

pub fn print_task(task: &Task, device_path: &str, web_url: &str) -> Result<(), PrintError> {
    let mut printer = Printer::new(device_path)?;
    printer.init()?;

    let task_url = format!("{}/projects/{}/tasks/{}", web_url, task.project_id, task.id);
    printer.align(Align::Center)?;
    printer.qrcode(&task_url)?;
    printer.newline()?;

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
