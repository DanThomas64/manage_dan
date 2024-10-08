use crate::prelude::*;

/// The status enum is used to store the possible status of a system
#[derive(Debug, Clone, Copy)]
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
    pub tasks: Status,
    pub todo: Status,
    pub overall: Status,
}

impl SystemsStatus {
    /// Create a new system status struct with all init status values
    pub async fn init() -> AppResult {
        let mut systems = SystemsStatus {
            db: Status::Init,
            log: Status::Init,
            notes: Status::Init,
            project: Status::Init,
            tasks: Status::Init,
            todo: Status::Init,
            overall: Status::Go,
        };
        match db::init().map_err(|e| AppError::Db(e).print()).is_ok() {
            true => systems.update("db", Status::Go),
            false => systems.update("db", Status::Nogo),
        };
        // initialize log
        match log::init().map_err(|e| AppError::Log(e).print()).is_ok() {
            true => systems.update("log", Status::Go),
            false => systems.update("log", Status::Nogo),
        };
        // initialize notes
        match notes::init()
            .map_err(|e| AppError::Notes(e).print())
            .is_ok()
        {
            true => systems.update("notes", Status::Go),
            false => systems.update("notes", Status::Nogo),
        };
        // initialize project
        match project::init()
            .map_err(|e| AppError::Project(e).print())
            .is_ok()
        {
            true => systems.update("project", Status::Go),
            false => systems.update("project", Status::Nogo),
        };
        // initialize tasks
        match tasks::init()
            .map_err(|e| AppError::Tasks(e).print())
            .is_ok()
        {
            true => systems.update("tasks", Status::Go),
            false => systems.update("tasks", Status::Nogo),
        };
        // initialize todo
        match todo::init().map_err(|e| AppError::Todo(e).print()).is_ok() {
            true => systems.update("todo", Status::Go),
            false => systems.update("todo", Status::Nogo),
        };
        let _ = systems.monitor().await;
        Ok(())
    }
    /// Check the status of each system in the Status Struct and then update
    /// the overall status accordingly.
    fn gonogo(&self) -> Status {
        self.iter().fold(self.overall, |status: Status, x: Status| {
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
                    Status::Go => Status::Degraded,
                    _ => Status::Unknown,
                },
                Status::Degraded => match x {
                    _ => Status::Degraded,
                },
                _ => Status::Unknown,
            };
            n_status
        })
    }
    fn iter(&self) -> SystemsIter {
        SystemsIter {
            systems: *self,
            index: 0,
        }
    }
    pub fn update(&mut self, val: &str, status: Status) -> Self {
        match val {
            "db" => self.db = status,
            "log" => self.log = status,
            "notes" => self.notes = status,
            "project" => self.project = status,
            "tasks" => self.tasks = status,
            "todo" => self.todo = status,
            "overall" => self.overall = status,
            _ => self.overall = Status::Unknown,
        }
        *self
    }
    /// A monitoring process to make check the SystemStatus Struct
    pub async fn monitor(mut self) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: This needs to handle errors
        let _ = tokio::spawn(async move {
            loop {
                let _ = sleep(Duration::from_millis(500)).await;
                let status = self.gonogo();
                self.overall = status;
                info!("Overall Status: {:?}", status);
            }
        })
        .await;
        Ok(())
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
            4 => Some(self.systems.tasks),
            5 => Some(self.systems.todo),
            6 => Some(self.systems.overall),
            _ => None,
        };
        self.index += 1;
        result
    }
}
