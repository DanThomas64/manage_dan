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

    /// A fully-healthy baseline: every subsystem `Go`. Used instead of
    /// `SystemsStatus::init()`, which would run every subsystem's *real*
    /// `init()` (DB, printer, Vikunja/nb, ...) and requires `AppConfig` to
    /// already be loaded — neither of which this pure-logic unit test sets up.
    fn all_go() -> SystemsStatus {
        let mut systems = SystemsStatus::new();
        systems.update("db", Status::Go);
        systems.update("log", Status::Go);
        systems.update("notes", Status::Go);
        systems.update("project", Status::Go);
        systems.update("printer", Status::Go);
        systems.update("todo", Status::Go);
        systems.update("lists", Status::Go);
        systems
    }

    #[test]
    fn nogogo() {
        // Exercises `SystemsGoNogo::gonogo`'s status-combination logic in
        // isolation, starting from an all-`Go` baseline each time.
        let mut systems = all_go();
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
        // reset systems and try with Nogo system — one `Nogo` alone only
        // demotes `Init`/`Go`/`Unknown` down to `Degraded` (never straight to
        // `Nogo`); a second `Nogo` is needed to push an already-`Degraded`
        // status the rest of the way, so two fields are forced here.
        systems = all_go();
        systems.update("db", Status::Nogo);
        systems.update("printer", Status::Nogo);
        // reset status to init
        status.gono = Status::Init;
        status.gonogo(systems);
        assert_eq!(status.gono, Status::Nogo);
        // reset systems and try with unknown system
        systems = all_go();
        systems.update("project", Status::Unknown);
        // reset status to degraded
        status.gono = Status::Degraded;
        status.gonogo(systems);
        assert_eq!(status.gono, Status::Degraded);
        // reset systems and try with unknown system
        systems = all_go();
        systems.update("project", Status::Degraded);
        systems.update("printer", Status::Init);
        // reset status to degraded
        status.gono = Status::Go;
        status.gonogo(systems);
        assert_eq!(status.gono, Status::Degraded);
    }
}
