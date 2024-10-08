#[macro_export]
macro_rules! system_init {
    ( s_s, sys ) => {
        s_s.sys = sys::init()
            .map_err(|e| error::AppError::sys(e).print())
            .is_ok()
    };
}

#[macro_export]
macro_rules! subsystem_init {
    ( s_s, sys ) => {
        s_s.sys = sys::init()
            .map_err(|e| error::AppError::sys(e).print())
            .is_ok()
    };
}
