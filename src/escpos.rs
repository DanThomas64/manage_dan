use crate::datatypes::Task;
use anyhow::Result;
use escpos::driver::FileDriver;
use escpos::printer::Printer;
use escpos::utils::*;

pub fn print_task(task: &Task, device_path: &str) -> Result<()> {
    let driver = FileDriver::new(device_path)?;
    let mut printer = Printer::new(driver, None, None);

    printer.init()?;
    printer.justify(Justification::Center)?;
    print_header(&mut printer, "Task")?;
    printer.text(&task.title)?;
    printer.ln(1)?;
    printer.justify(Justification::Left)?;
    printer.ln(1)?;
    printer.text("Project ID: ")?;
    printer.text(&task.project_id.to_string())?;
    printer.ln(2)?;
    printer.text("Description:")?;
    printer.ln(1)?;
    printer.text(&task.description_as_text(42))?;
    printer.ln(2)?;

    if let Some(labels) = &task.labels {
        if !labels.is_empty() {
            printer.text("Labels:")?;
            printer.ln(1)?;
            for label in labels {
                printer.text(&format!("- {}", label.title))?;
                printer.ln(1)?;
            }
        }
    }

    print_footer(&mut printer, "Task")?;
    printer.cut()?;
    Ok(())
}

fn print_header(printer: &mut Printer<FileDriver>, print_type: &str) -> Result<()> {
    printer.ln(4)?;
    printer.text(print_type)?;
    printer.ln(4)?;
    Ok(())
}

fn print_footer(printer: &mut Printer<FileDriver>, print_type: &str) -> Result<()> {
    printer.justify(Justification::Center)?;
    printer.ln(4)?;
    printer.text(print_type)?;
    printer.ln(9)?;
    Ok(())
}
