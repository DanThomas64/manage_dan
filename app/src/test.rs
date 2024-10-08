#[cfg(test)]
mod nogo_tests {
    use crate::nogo::*;

    #[test]
    fn update() {
        let mut systems = SystemsStatus::new();
        assert_eq!(systems.project, Status::Init);
        systems.update("log", Status::Degraded);
        assert_eq!(systems.log, Status::Degraded);
        systems.update("project", Status::Nogo);
        assert_eq!(systems.project, Status::Nogo);
        systems.update("db", Status::Nogo);
        assert_eq!(systems.db, Status::Nogo);
    }

    #[test]
    fn nogogo() {
        let mut systems = SystemsStatus::new();
        systems.init();
        let mut status = SystemsGoNogo::new();
        // Check init
        assert_eq!(status.gono, Status::Init);
        // Should update everhting to Go
        status.gonogo(systems);
        assert_eq!(status.gono, Status::Go);
        // Change systems to Degraded
        systems.update("log", Status::Degraded);
        status.gonogo(systems);
        assert_eq!(status.gono, Status::Degraded);
        // reset systems and try with Nogo system
        systems.init();
        systems.update("db", Status::Nogo);
        // reset status to init
        status.gono = Status::Init;
        status.gonogo(systems);
        assert_eq!(status.gono, Status::Nogo);
        // reset systems and try with unknown system
        systems.init();
        systems.update("project", Status::Unknown);
        // reset status to degraded
        status.gono = Status::Degraded;
        status.gonogo(systems);
        assert_eq!(status.gono, Status::Degraded);
        // reset systems and try with unknown system
        systems.init();
        systems.update("project", Status::Degraded);
        systems.update("tasks", Status::Init);
        // reset status to degraded
        status.gono = Status::Go;
        status.gonogo(systems);
        assert_eq!(status.gono, Status::Degraded);
    }
}
