use crate::prelude::*;

/// The status enum is used to store the possible status of a system
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    Init,
    Go,
    Nogo,
    Degraded,
    Unknown,
}

/// The actual store of the status of each system and the oversall status
#[derive(Debug, Clone, Copy)]
pub struct SystemsStatus {
    pub db: Status,
    pub log: Status,
    pub notes: Status,
    pub project: Status,
    pub printer: Status,
    pub todo: Status,
}

#[derive(Debug, Clone, Copy)]
pub struct SystemsGoNogo {
    pub gono: Status,
}
impl SystemsGoNogo {
    pub fn new() -> SystemsGoNogo {
        SystemsGoNogo { gono: Status::Init }
    }
    // TODO: Create Error handling for this
    pub async fn init(&mut self, systems: SystemsStatus) {
        // Calculate initial status based on system initialization results
        *self = self.gonogo(systems);
        
        // Log initial state
        info!("Overall Status initialized: {:?}", self.gono);
        
        // Start monitoring loop
        let _ = self.monitor(systems).await;
    }
    /// Check the status of each system in the Status Struct and then update
    /// the overall status accordingly.
    // TODO: Create Error handling for this
    pub fn gonogo(&mut self, all_sys: SystemsStatus) -> SystemsGoNogo {
        all_sys.iter().fold(self.gono, |status: Status, x: Status| {
            let n_status: Status = match status {
                Status::Init => match x {
                    Status::Go => Status::Go,
                    Status::Nogo => Status::Nogo,
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
                    _ => Status::Degraded,
                },
                _ => Status::Unknown,
            };
            self.gono = n_status;
            n_status
        });
        *self
    }
    /// A monitoring process to make check the GoNogo Struct
    pub async fn monitor(
        mut self,
        systems: SystemsStatus,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: Create Error handling for this
        let _ = tokio::spawn(async move {
            loop {
                let _ = sleep(Duration::from_millis(500)).await;
                
                // Capture status before update
                let old_status = self.gono;
                
                // Update status
                let _ = self.gonogo(systems);
                
                if self.gono != old_status {
                    // Log only if status has changed
                    info!("Overall Status changed: {:?}", self.gono);
                }
            }
        })
        .await;
        Ok(())
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
        }
    }
    // TODO: Create Error handling for this
    pub fn init(&mut self) -> SystemsStatus {
        
        // 1. Initialize DB first, so the log table exists when logging starts.
        match db::init().map_err(|e| AppError::Db(e).print()).is_ok() {
            true => self.update("db", Status::Go),
            false => self.update("db", Status::Nogo),
        };

        // 2. Initialize log system, which relies on DB being ready for DB logging.
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
        match printer::init(config.printer.vendor_id, config.printer.product_id)
            .map_err(|e| AppError::Printer(e).print())
            .is_ok()
        {
            true => self.update("printer", Status::Go),
            false => self.update("printer", Status::Nogo),
        };
        
        // initialize todo
        match todo::init().map_err(|e| AppError::Todo(e).print()).is_ok() {
            true => self.update("todo", Status::Go),
            false => self.update("todo", Status::Nogo),
        };
        *self
    }
    fn iter(&self) -> SystemsIter {
        SystemsIter {
            systems: *self,
            index: 0,
        }
    }
    // TODO: Create Error handling for this
    pub fn update(&mut self, val: &str, status: Status) -> Self {
        match val {
            "db" => self.db = status,
            "log" => self.log = status,
            "notes" => self.notes = status,
            "project" => self.project = status,
            "printer" => self.printer = status,
            "todo" => self.todo = status,
            _ => _ = Status::Unknown,
        }
        *self
    }
}

struct SystemsIter {
    systems: SystemsStatus,
    index: usize,
}

impl Iterator for SystemsIter {
    type Item = Status;

    fn next(&mut self) -> Option<Self::Item> {
        let result = match self.index {
            0 => Some(self.systems.db),
            1 => Some(self.systems.log),
            2 => Some(self.systems.notes),
            3 => Some(self.systems.project),
            4 => Some(self.systems.printer),
            5 => Some(self.systems.todo),
            _ => None,
        };
        self.index += 1;
        result
    }
}
