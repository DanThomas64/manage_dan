use crate::datatypes::Task;
use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use escpos::driver::NativeUsbDriver;
use escpos::utils::*;
use escpos::printer::Printer;
use escpos::ui::line::*;
use std::env;

pub fn print_task(task: &Task, vid: u16, pid: u16) -> Result<()> {
    let driver = NativeUsbDriver::open(vid, pid, None)?;
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

fn print_task_qrcode(printer: &mut Printer<NativeUsbDriver>, task: &Task) -> Result<()> {
    let base_url = env::var("BASE_URL").context("API_URL environment variable not set")?;
    let task_url = format!("{base_url}/tasks/{}", task.id);
    printer.justify(JustifyMode::CENTER)?
        .qrcode_option(
                task_url.as_str(),
                QRCodeOption::new(QRCodeModel::Model1, 6, QRCodeCorrectionLevel::M),
        )?;
    Ok(())
}

fn print_task_title(printer: &mut Printer<NativeUsbDriver>, task: &Task) -> Result<()> {
    let line = LineBuilder::new().style(LineStyle::Custom("=-")).build();
    printer
        .bold(true)?
        .writeln(&task.title)?
        .feed()?
        .draw_line(line)?
        .bold(false)?
        .reset_size()?
        .justify(JustifyMode::LEFT)?;
    Ok(())
}

fn print_task_description(printer: &mut Printer<NativeUsbDriver>, task: &Task) -> Result<()> {
    printer
        .feed()?
        .bold(true)?
        .writeln("Description:")?
        .bold(false)?
        .justify(JustifyMode::CENTER)?
        .write(&task.description_as_text(42))?
        .feed()?
        .justify(JustifyMode::LEFT)?;
    Ok(())
}

fn print_task_due_date(printer: &mut Printer<NativeUsbDriver>, task: &Task) -> Result<()> {
    printer
        .feed()?
        .bold(true)?
        .write("Due Date:")?
        .bold(false)?
        .feed()?
        .justify(JustifyMode::CENTER)?;
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
    printer.write(&due_date_str)?
        .feed()?
        .justify(JustifyMode::LEFT)?;
    Ok(())
}

fn print_task_labels(printer: &mut Printer<NativeUsbDriver>, task: &Task) -> Result<()> {
    if let Some(labels) = &task.labels {
        if !labels.is_empty() {
            printer
                .bold(true)?
                .writeln("Labels:")?
                .bold(false)?
                .justify(JustifyMode::CENTER)?;
            for label in labels {
                printer.writeln(&format!("- {}", label.title))?;
            }
        }
    }
    printer
        .feed()?
        .justify(JustifyMode::LEFT)?;
    Ok(())
}

fn print_task_printed_time(printer: &mut Printer<NativeUsbDriver>) -> Result<()> {
    let now = Local::now();
    let datetime_str = now.format("%Y-%m-%d %H:%M:%S").to_string();
    printer
        .feed()?
        .reset_size()?
        .bold(true)?
        .write("Printed at:")?
        .bold(false)?
        .feed()?
        .justify(JustifyMode::CENTER)?
        .write(&datetime_str)?
        .feed()?
        .justify(JustifyMode::LEFT)?;
    Ok(())
}

pub fn print_daily_summary(tasks: &[Task], vid: u16, pid: u16) -> Result<()> {
    let driver = NativeUsbDriver::open(vid, pid, None)?;
    let mut printer = Printer::new(driver, Protocol::default(), None);

    printer.init()?;
    print_header(&mut printer, "--- Daily Summary ---")?;
    print_summary_datetime(&mut printer)?;
    print_summary_tasks(&mut printer, tasks)?;
    print_footer(&mut printer, "--- End Summary ---")?;
    printer.print_cut()?;
    Ok(())
}

fn print_summary_datetime(printer: &mut Printer<NativeUsbDriver>) -> Result<()> {
    let now = Local::now();
    let datetime_str = now.format("%a %d-%b-%Y").to_string();
    printer
        .justify(JustifyMode::CENTER)?
        .writeln(&datetime_str)?
        .feed()?
        .justify(JustifyMode::LEFT)?;
    Ok(())
}

fn print_summary_tasks(printer: &mut Printer<NativeUsbDriver>, tasks: &[Task]) -> Result<()> {
    printer.justify(JustifyMode::CENTER)?
        .reset_size()?;
    if tasks.is_empty() {
        printer.writeln("No tasks for today.")?;
    } else {
        for task in tasks {
            printer.writeln(&format!("- {}", task.title))?.feed()?;
        }
    }
    Ok(())
}

fn print_header(printer: &mut Printer<NativeUsbDriver>, print_type: &str) -> Result<()> {
    printer.justify(JustifyMode::CENTER)?
        .feeds(2)?
        .size(2,2)?
        .write(print_type)?
        .feeds(3)?;
    Ok(())
}

fn print_footer(printer: &mut Printer<NativeUsbDriver>, print_type: &str) -> Result<()> {
    printer.justify(JustifyMode::CENTER)?
        .feed()?
        .size(2,2)?
        .write(print_type)?
        .feed()?;
    print_task_printed_time(printer)?;
        printer.feeds(6)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datatypes::{Label, Task};
    use std::env;
    use std::sync::{Mutex, OnceLock};

    fn test_mutex() -> &'static Mutex<()> {
        static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
        MUTEX.get_or_init(|| Mutex::new(()))
    }

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
        let _guard = test_mutex().lock().unwrap();
        // This test will attempt to print to a USB device specified by PRINTER_VID and PRINTER_PID.
        // Ensure these environment variables are set before running the test.

        let vid_str = env::var("PRINTER_VID")
            .context("PRINTER_VID environment variable not set for test")?;
        let pid_str = env::var("PRINTER_PID")
            .context("PRINTER_PID environment variable not set for test")?;
        let vid = u16::from_str_radix(vid_str.trim_start_matches("0x"), 16)
            .context("Failed to parse PRINTER_VID as a hexadecimal value")?;
        let pid = u16::from_str_radix(pid_str.trim_start_matches("0x"), 16)
            .context("Failed to parse PRINTER_PID as a hexadecimal value")?;

        let task = create_test_task();

        // Set required environment variables for the test
        env::set_var("BASE_URL", "http://example.com");

        let result = print_task(&task, vid, pid);

        // Clean up env var
        env::remove_var("BASE_URL");

        assert!(result.is_ok());

        Ok(())
    }

    #[test]
    fn test_print_daily_summary_example() -> Result<()> {
        let _guard = test_mutex().lock().unwrap();

        let vid_str = env::var("PRINTER_VID")
            .context("PRINTER_VID environment variable not set for test")?;
        let pid_str = env::var("PRINTER_PID")
            .context("PRINTER_PID environment variable not set for test")?;
        let vid = u16::from_str_radix(vid_str.trim_start_matches("0x"), 16)
            .context("Failed to parse PRINTER_VID as a hexadecimal value")?;
        let pid = u16::from_str_radix(pid_str.trim_start_matches("0x"), 16)
            .context("Failed to parse PRINTER_PID as a hexadecimal value")?;

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

        let result = print_daily_summary(&tasks, vid, pid);

        assert!(result.is_ok());
        Ok(())
    }
}
