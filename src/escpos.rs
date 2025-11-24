use crate::datatypes::Task;
use anyhow::Result;
use escpos::driver::FileDriver;
use escpos::utils::{JustifyMode, Protocol};
use escpos::printer::Printer;
use std::path::Path;

pub fn print_task(task: &Task, device_path: &str) -> Result<()> {
    let path = Path::new(device_path);
    let driver = FileDriver::open(path)?;
    let mut printer = Printer::new(driver, Protocol::default(), None);

    printer.init()?;
    printer.justify(JustifyMode::CENTER)?;
    print_header(&mut printer, "Task")?;
    printer.writeln(&task.title)?;
    printer.justify(JustifyMode::LEFT)?;
    printer.feed()?;
    printer.write("Project ID: ")?;
    printer.write(&task.project_id.to_string())?;
    printer.feeds(2)?;
    printer.writeln("Description:")?;
    printer.write(&task.description_as_text(42))?;
    printer.feeds(2)?;

    if let Some(labels) = &task.labels {
        if !labels.is_empty() {
            printer.writeln("Labels:")?;
            for label in labels {
                printer.writeln(&format!("- {}", label.title))?;
            }
        }
    }

    print_footer(&mut printer, "Task")?;
    printer.cut()?;
    Ok(())
}

fn print_header(printer: &mut Printer<FileDriver>, print_type: &str) -> Result<()> {
    printer.feeds(4)?;
    printer.write(print_type)?;
    printer.feeds(4)?;
    Ok(())
}

fn print_footer(printer: &mut Printer<FileDriver>, print_type: &str) -> Result<()> {
    printer.justify(JustifyMode::CENTER)?;
    printer.feeds(4)?;
    printer.write(print_type)?;
    printer.feeds(9)?;
    Ok(())
}
