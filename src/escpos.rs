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

    printer.init()?;
    print_task_qrcode(&mut printer, task)?;
    print_header(&mut printer, "--- Task ---")?;
    print_task_title(&mut printer, task)?;
    print_task_description(&mut printer, task)?;
    print_task_due_date(&mut printer, task)?;
    print_task_labels(&mut printer, task)?;
    print_footer(&mut printer, "--- End Task ---")?;
    printer.print_cut()?;
    Ok(())
}

fn print_task_qrcode(printer: &mut Printer<FileDriver>, task: &Task) -> Result<()> {
    let base_url = env::var("BASE_URL").context("API_URL environment variable not set")?;
    let task_url = format!("{base_url}/tasks/{}", task.id);
    printer.justify(JustifyMode::CENTER)?
        .qrcode_option(
                task_url.as_str(),
                QRCodeOption::new(QRCodeModel::Model1, 6, QRCodeCorrectionLevel::M),
        )?;
    Ok(())
}

fn print_task_title(printer: &mut Printer<FileDriver>, task: &Task) -> Result<()> {
    let line = LineBuilder::new().style(LineStyle::Custom("=-")).build();
    printer.bold(true)?
        .writeln(&task.title)?
        .feed()?
        .draw_line(line)?
        .bold(false)?
        .reset_size()?
        .justify(JustifyMode::LEFT)?;
    Ok(())
}

fn print_task_description(printer: &mut Printer<FileDriver>, task: &Task) -> Result<()> {
    printer
        .feed()?
        .writeln("Description:")?
        .write(&task.description_as_text(42))?
        .feeds(2)?;
    Ok(())
}

fn print_task_due_date(printer: &mut Printer<FileDriver>, task: &Task) -> Result<()> {
    printer.write("Due Date:")?.feed()?;
    let due_date_str = if task.due_date.starts_with("0001-01-01") {
        "No due date".to_string()
    } else {
        match DateTime::parse_from_rfc3339(&task.due_date) {
            Ok(dt) => dt
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string(),
            Err(_) => task.due_date.clone(),
        }
    };
    printer.write(&due_date_str)?.feeds(2)?;
    Ok(())
}

fn print_task_labels(printer: &mut Printer<FileDriver>, task: &Task) -> Result<()> {
    if let Some(labels) = &task.labels {
        if !labels.is_empty() {
            printer.writeln("Labels:")?;
            for label in labels {
                printer.writeln(&format!("- {}", label.title))?;
            }
        }
    }
    printer.feed()?;
    Ok(())
}

pub fn print_daily_summary(tasks: &[Task], device_path: &str) -> Result<()> {
    let path = Path::new(device_path);
    let driver = FileDriver::open(path)?;
    let mut printer = Printer::new(driver, Protocol::default(), None);

    printer.init()?;
    print_header(&mut printer, "--- Daily Summary ---")?;
    print_summary_datetime(&mut printer)?;
    print_summary_tasks(&mut printer, tasks)?;
    print_footer(&mut printer, "--- End Summary ---")?;
    printer.print_cut()?;
    Ok(())
}

fn print_summary_datetime(printer: &mut Printer<FileDriver>) -> Result<()> {
    let now = Local::now();
    let datetime_str = now.format("%Y-%m-%d %H:%M").to_string();
    printer
        .justify(JustifyMode::CENTER)?
        .writeln(&datetime_str)?
        .feed()?
        .justify(JustifyMode::LEFT)?;
    Ok(())
}

fn print_summary_tasks(printer: &mut Printer<FileDriver>, tasks: &[Task]) -> Result<()> {
    if tasks.is_empty() {
        printer.writeln("No tasks for today.")?;
    } else {
        for task in tasks {
            printer.writeln(&format!("- {}", task.title))?.feed()?;
        }
    }
    Ok(())
}

fn print_header(printer: &mut Printer<FileDriver>, print_type: &str) -> Result<()> {
    printer.feeds(2)?
        .size(2,2)?
        .write(print_type)?
        .feeds(3)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datatypes::{Label, Task};
    use std::env;

    fn create_test_task() -> Task {
        Task {
            id: 123,
            title: "Test Print Task".to_string(),
            description: "This is a <b>test</b> description for the <i>print function</i>."
                .to_string(),
            updated: "2025-11-24T10:00:00Z".to_string(),
            done: false,
            labels: Some(vec![
                Label {
                    title: "Urgent".to_string(),
                },
                Label {
                    title: "Test".to_string(),
                },
            ]),
            project_id: 1,
            due_date: "2025-11-25T18:00:00Z".to_string(),
            reminders: None,
        }
    }

    #[test]
    fn test_print_task_example() -> Result<()> {
        // This test generates an output file `test_print_output.bin` with ESC/POS commands.
        // You can send this file to a compatible thermal printer to see the output, e.g.,
        // on Linux/macOS: `lp test_print_output.bin`
        // Or `cat test_print_output.bin > /dev/usb/lp0`

        let task = create_test_task();
        let output_file = "/dev/usb/lp0";

        // Set required environment variables for the test
        env::set_var("BASE_URL", "http://example.com");

        let result = print_task(&task, output_file);

        // Clean up env var
        env::remove_var("BASE_URL");

        assert!(result.is_ok());

        Ok(())
    }

    #[test]
    fn test_print_daily_summary_example() -> Result<()> {
        let tasks = vec![
            create_test_task(),
            Task {
                id: 124,
                title: "Another Test Task".to_string(),
                description: String::new(),
                updated: String::new(),
                done: false,
                labels: None,
                project_id: 1,
                due_date: String::new(),
                reminders: None,
            },
        ];
        let output_file = "/dev/usb/lp0";

        let result = print_daily_summary(&tasks, output_file);

        assert!(result.is_ok());

        Ok(())
    }
}
