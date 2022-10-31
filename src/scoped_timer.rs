use std::time::SystemTime;


// Simple ScopedTimer that prints out elapsed time when it goes out of scope.
pub struct ScopedTimer {
    // event_id is printed out in the result.
    event_id: String,
    start_time: SystemTime
}



impl ScopedTimer {
    pub fn new(event_id: String) -> Self {
        let start_time = std::time::SystemTime::now();
        ScopedTimer {
            event_id,
            start_time
        }
    }
}

impl Drop for ScopedTimer {
    fn drop(&mut self) {
        match SystemTime::now().duration_since(self.start_time) {
            Ok(elapsed) => log::info!("ScopedTimer[{}],{:#?}", self.event_id, elapsed),
            Err(e) => log::error!("ScopedTimer[{}] failed to compute elapsed time {}", self.event_id, e)
        }
    }
}
