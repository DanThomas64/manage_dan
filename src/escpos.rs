use crate::datatypes::Task;
use anyhow::Result;
use escpos::driver::ConsoleDriver;
use escpos::printer::command::{Justification, Protocol};
use escpos::printer::Printer;

pub fn print_task(task: &Task, _device_path: &str) -> Result<()> {
    let driver = ConsoleDriver::new();
    let mut printer = Printer::new(driver, Protocol::default(), None)?;

    printer.init()?;
    printer.justify(Justification::Center)?;
    print_header(&mut printer, "Task")?;
    printer.writeln(task.title.as_bytes())?;
    printer.justify(Justification::Left)?;
    printer.feed(1)?;
    printer.write(b"Project ID: ")?;
    printer.write(task.project_id.to_string().as_bytes())?;
    printer.feed(2)?;
    printer.writeln(b"Description:")?;
    printer.write(task.description_as_text(42).as_bytes())?;
    printer.feed(2)?;

    if let Some(labels) = &task.labels {
        if !labels.is_empty() {
            printer.writeln(b"Labels:")?;
            for label in labels {
                printer.writeln(format!("- {}", label.title).as_bytes())?;
            }
        }
    }

    print_footer(&mut printer, "Task")?;
    printer.cut()?;
    Ok(())
}

fn print_header(printer: &mut Printer<ConsoleDriver>, print_type: &str) -> Result<()> {
    printer.feed(4)?;
    printer.write(print_type.as_bytes())?;
    printer.feed(4)?;
    Ok(())
}

fn print_footer(printer: &mut Printer<ConsoleDriver>, print_type: &str) -> Result<()> {
    printer.justify(Justification::Center)?;
    printer.feed(4)?;
    printer.write(print_type.as_bytes())?;
    printer.feed(9)?;
    Ok(())
}
