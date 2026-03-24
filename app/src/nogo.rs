//! System status tracking and Go/NoGo determination logic.
//!
//! This module defines the health status of individual subsystems and calculates
//! the overall operational status of the application.

use crate::prelude::*;
use tokio::time::{sleep, Duration};
use serde::{Serialize, Deserialize}; // Added serde imports

/// The status enum is used to store the possible status of a system
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)] // Added Serialize/Deserialize
pub enum Status {
    Init,
    Go,
    Nogo,
    Degraded,
    Unknown,
}

/// The actual store of the status of each system and the oversall status
#[derive(Debug, Clone, Copy, Serialize, Deserialize)] // Added Serialize/Deserialize
pub struct SystemsStatus {
    pub db: Status,
    pub log: Status,
    pub notes: Status,
    pub project: Status,
    pub printer: Status,
    pub todo: Status,
    pub shopping: Status,
}

/// Holds the overall Go/NoGo status of the application.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)] // Added Serialize/Deserialize
pub struct SystemsGoNogo {
    pub gono: Status,
}
impl SystemsGoNogo {
    /// Creates a new `SystemsGoNogo` instance initialized to `Status::Init`.
    pub fn new() -> SystemsGoNogo {
        SystemsGoNogo { gono: Status::Init }
    }
    
    /// Calculates the initial overall status based on system initialization results.
    pub fn calculate_initial_status(&mut self, systems: SystemsStatus) {
        *self = self.gonogo(systems);
        info!("Overall Status initialized: {:?}", self.gono);
    }

    /// Starts the monitoring loop in a background task.
    pub fn start_monitoring(self, systems: SystemsStatus) {
        // Spawn the monitoring loop and do NOT await it.
        tokio::spawn(async move {
            let _ = self.monitor(systems).await;
        });
    }

    /// Check the status of each system in the Status Struct and then update
    /// the overall status accordingly.
    // TODO: Create Error handling for this
    /// Determines the overall status based on the status of all individual systems.
    pub fn gonogo(&mut self, all_sys: SystemsStatus) -> SystemsGoNogo {
        all_sys.iter().fold(self.gono, |status: Status, (_, x): (&'static str, Status)| {
            let n_status: Status = match status {
                Status::Init => match x {
                    Status::Go => Status::Go,
                    Status::Nogo | Status::Degraded => Status::Degraded, // Degraded is treated as degraded from Init
                    _ => Status::Unknown,
                },
                Status::Go => match x {
                    Status::Go => Status::Go,
                    Status::Nogo | Status::Degraded => Status::Degraded,
                    _ => Status::Unknown,
                },
                Status::Nogo => match x {
                    _ => Status::Nogo,
                },
                Status::Degraded => match x {
                    Status::Nogo => Status::Nogo,
                    _ => Status::Degraded,
                },
                _ => Status::Unknown,
            };
            self.gono = n_status;
            n_status
        });
        *self
    }
    
    /// The actual monitoring process loop.
    pub async fn monitor(
        mut self,
        systems: SystemsStatus,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: Create Error handling for this
        loop {
            sleep(Duration::from_millis(500)).await;
            
            // Capture status before update
            let old_status = self.gono;
            
            // Update status
            let _ = self.gonogo(systems);
            
            if self.gono != old_status {
                // Log only if status has changed
                info!("Overall Status changed: {:?}", self.gono);
            }
        }
    }
}

impl SystemsStatus {
    /// Create a new system status struct with all init status values
    pub fn new() -> SystemsStatus {
        SystemsStatus {
            db: Status::Init,
            log: Status::Init,
            notes: Status::Init,
            project: Status::Init,
            printer: Status::Init,
            todo: Status::Init,
            shopping: Status::Init,
        }
    }
    // TODO: Create Error handling for this
    /// Initializes all subsystems and updates their status fields.
    pub fn init(&mut self) -> SystemsStatus {
        
        // 1. Initialize DB first, so the log table exists when logging starts.
        match db::init().map_err(|e| AppError::Db(e).print()).is_ok() {
            true => self.update("db", Status::Go),
            false => self.update("db", Status::Nogo),
        };

        // 2. Initialize log system, which relies on DB being ready for DB logging.
        // This must happen AFTER DB initialization to ensure the 'log' table exists.
        match log::init().map_err(|e| AppError::Log(e).print()).is_ok() {
            true => self.update("log", Status::Go),
            false => self.update("log", Status::Nogo),
        };
        
        // initialize notes
        match notes::init()
            .map_err(|e| AppError::Notes(e).print())
            .is_ok()
        {
            true => self.update("notes", Status::Go),
            false => self.update("notes", Status::Nogo),
        };
        // initialize project
        match project::init()
            .map_err(|e| AppError::Project(e).print())
            .is_ok()
        {
            true => self.update("project", Status::Go),
            false => self.update("project", Status::Nogo),
        };
        
        // 5. Initialize printer, passing configuration values
        let config = AppConfig::get();
        match printer::init(config.printer.vendor_id, config.printer.product_id, &config.printer.mode, config.printer.characters_per_line)
            .map_err(|e| AppError::Printer(e).print())
            .is_ok()
        {
            true => self.update("printer", Status::Go),
            false => self.update("printer", Status::Nogo),
        };
        
        // initialize todo (Vikunja backend)
        let vikunja_cfg = &AppConfig::get().vikunja;
        match todo::init(
            &vikunja_cfg.base_url,
            &vikunja_cfg.api_token,
            vikunja_cfg.project_id,
        )
        .map_err(|e| AppError::Todo(e).print())
        .is_ok()
        {
            true => self.update("todo", Status::Go),
            false => self.update("todo", Status::Nogo),
        };

        // initialize shopping
        match shopping::init()
            .map_err(|e| AppError::Shopping(e).print())
            .is_ok()
        {
            true => self.update("shopping", Status::Go),
            false => self.update("shopping", Status::Nogo),
        };

        *self
    }
    
    /// Returns an iterator over the system statuses.
    pub fn iter(&self) -> SystemsIter {
        SystemsIter {
            systems: *self,
            index: 0,
        }
    }
    // TODO: Create Error handling for this
    /// Updates the status of a specific subsystem by name.
    pub fn update(&mut self, val: &str, status: Status) -> Self {
        match val {
            "db" => self.db = status,
            "log" => self.log = status,
            "notes" => self.notes = status,
            "project" => self.project = status,
            "printer" => self.printer = status,
            "todo" => self.todo = status,
            "shopping" => self.shopping = status,
            _ => _ = Status::Unknown,
        }
        *self
    }
}

/// An iterator over the fields of `SystemsStatus`.
pub struct SystemsIter {
    systems: SystemsStatus,
    index: usize,
}

impl Iterator for SystemsIter {
    type Item = (&'static str, Status);

    fn next(&mut self) -> Option<Self::Item> {
        let result = match self.index {
            0 => Some(("db", self.systems.db)),
            1 => Some(("log", self.systems.log)),
            2 => Some(("notes", self.systems.notes)),
            3 => Some(("project", self.systems.project)),
            4 => Some(("printer", self.systems.printer)),
            5 => Some(("todo", self.systems.todo)),
            6 => Some(("shopping", self.systems.shopping)),
            _ => None,
        };
        self.index += 1;
        result
    }
}
