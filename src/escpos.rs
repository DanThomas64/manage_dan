use crate::datatypes::Task;
use anyhow::Result;
use escpos::driver::ConsoleDriver;
use escpos::utils::{JustifyMode, Protocol};
use escpos::printer::Printer;

pub fn print_task(task: &Task, _device_path: &str) -> Result<()> {
    let driver = ConsoleDriver::open(false);
    let mut printer = Printer::new(driver, Protocol::default(), None);

    printer.init()?;
    printer.justify(JustifyMode::Center)?;
    print_header(&mut printer, "Task")?;
    printer.writeln(&task.title)?;
    printer.justify(JustifyMode::Left)?;
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

fn print_header(printer: &mut Printer<ConsoleDriver>, print_type: &str) -> Result<()> {
    printer.feeds(4)?;
    printer.write(print_type)?;
    printer.feeds(4)?;
    Ok(())
}

fn print_footer(printer: &mut Printer<ConsoleDriver>, print_type: &str) -> Result<()> {
    printer.justify(JustifyMode::Center)?;
    printer.feeds(4)?;
    printer.write(print_type)?;
    printer.feeds(9)?;
    Ok(())
}
