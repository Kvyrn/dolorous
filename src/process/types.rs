use tokio::time::Instant;

#[derive(Debug)]
pub enum WantedState {
    Running,
    Stopped,
}

#[derive(Debug)]
pub enum ProcessState {
    Stopped,
    Watching {
        pid: i32,
        timeout_at: Instant,
        attempt: u16,
    },
    WaitingRestart {
        timeout_at: Instant,
        attempt: u16,
    },
    Running {
        pid: i32,
    },
    Stopping(StoppingState),
}

#[derive(Debug)]
pub enum StoppingState {
    Command { timeout_at: Instant, pid: i32 },
    Terminate { timeout_at: Instant, pid: i32 },
    Kill,
}

#[derive(Debug)]
pub enum Event {
    Start,
    Stop,
    ProcessExited { pid: i32, exit_code: i32 },
    TimeoutReached,
}
