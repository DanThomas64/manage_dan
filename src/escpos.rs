use crate::datatypes::Task;
use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use escpos::driver::FileDriver;
use escpos::utils::*;
use escpos::printer::Printer;
use escpos::ui::line::*;
use std::path::Path;
use std::env;

pub fn print_task(task: &Task, device_path: &str) -> Result<()> {
    let path = Path::new(device_path);
    let driver = FileDriver::open(path)?;
    let mut printer = Printer::new(driver, Protocol::default(), None);

    let line = LineBuilder::new().style(LineStyle::Custom("=-")).build();
    let base_url = env::var("BASE_URL").context("API_URL environment variable not set")?;
    let task_url = format!("{base_url}/tasks/{}", task.id);

    printer.init()?
        .justify(JustifyMode::CENTER)?
        .qrcode_option(
                task_url.as_str(),
                QRCodeOption::new(QRCodeModel::Model1, 6, QRCodeCorrectionLevel::M),
        )?;
    print_header(&mut printer, "--- Task ---")?;
    printer.bold(true)?
        .writeln(&task.title)?
        .feed()?
        .draw_line(line)?
        .bold(false)?
        .reset_size()?
        .justify(JustifyMode::LEFT)?
        .feed()?
        .writeln("Description:")?
        .write(&task.description_as_text(42))?
        .feeds(2)?
        .write("Due Date:")?
        .feed()?;

    let due_date_str = match DateTime::parse_from_rfc3339(&task.due_date) {
        Ok(dt) => dt
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        Err(_) => task.due_date.clone(),
    };
    printer.write(&due_date_str)?.feeds(2)?;

    if let Some(labels) = &task.labels {
        if !labels.is_empty() {
            printer.writeln("Labels:")?;
            for label in labels {
                printer.writeln(&format!("- {}", label.title))?;
            }
        }
    }

    print_footer(&mut printer, "--- End Task ---")?;
    printer.print_cut()?;
    Ok(())
}
fn print_header(printer: &mut Printer<FileDriver>, print_type: &str) -> Result<()> {


    printer.feeds(2)?
        .size(2,2)?
        .write(print_type)?
        .feeds(2)?
        .feeds(2)?;
    Ok(())
}

fn print_footer(printer: &mut Printer<FileDriver>, print_type: &str) -> Result<()> {
    printer.justify(JustifyMode::CENTER)?
        .feeds(2)?
        .size(2,2)?
        .write(print_type)?
        .feeds(6)?;
    Ok(())
}
