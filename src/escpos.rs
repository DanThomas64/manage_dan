use crate::datatypes::Task;
use anyhow::Result;
use escpos::printer::Printer;
use escpos::utils::*;
use escpos::{driver::*};
use std::path::Path;

pub fn print_task(task: &Task, device_path: &str) -> Result<()> {
    let driver = FileDriver::open(Path::new(device_path))?;
    let mut printer = Printer::new(driver, Protocol::default(), None);

    printer.init()?;
    printer.justify(JustifyMode::CENTER)?;
    print_header(&mut printer, "Task")?;
    printer.writeln(task.title.as_str())?;
    printer.justify(JustifyMode::LEFT)?;
    printer.feed()?;
    printer.write("Project ID: ")?;
    printer.write(task.project_id.to_string().as_str())?;
    printer.feed()?;
    printer.writeln("Description:")?;
    printer.write(task.description_as_text(42).as_str())?;
    printer.feed()?;

    if let Some(labels) = &task.labels {
        if !labels.is_empty() {
            printer.writeln("Labels:")?;
            for label in labels {
                printer.writeln(format!("- {}", label.title).as_str())?;
            }
        }
    }

    print_footer(&mut printer, "Task")?;
    printer.cut()?;
    Ok(())
}

fn print_header(printer: &mut Printer<FileDriver>, print_type: &str) -> Result<()> {
    printer.feed()?;
    printer.write(print_type)?;
    printer.feed()?;
    Ok(())
}

fn print_footer(printer: &mut Printer<FileDriver>, print_type: &str) -> Result<()> {
    printer.justify(JustifyMode::CENTER)?;
    printer.feed()?;
    printer.write(print_type)?;
    printer.feed()?;
    Ok(())
}
