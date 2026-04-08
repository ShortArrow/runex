use std::time::{Duration, Instant};

/// A recorded phase with its name and duration.
#[derive(Debug, Clone)]
pub struct Phase {
    pub name: String,
    pub duration: Duration,
}

/// A recorded `command_exists` call with result and duration.
#[derive(Debug, Clone)]
pub struct CommandExistsCall {
    pub command: String,
    pub found: bool,
    pub duration: Duration,
}

/// Collects timing data for expand phases and command_exists calls.
#[derive(Debug, Default)]
pub struct Timings {
    phases: Vec<Phase>,
    command_exists_calls: Vec<CommandExistsCall>,
}

/// Lightweight timer — just wraps `Instant::now()`.
pub struct PhaseTimer {
    start: Instant,
}

impl PhaseTimer {
    pub fn start() -> Self {
        Self { start: Instant::now() }
    }

    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

impl Timings {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_phase(&mut self, name: &str, duration: Duration) {
        self.phases.push(Phase {
            name: name.to_string(),
            duration,
        });
    }

    pub fn record_command_exists(&mut self, command: &str, found: bool, duration: Duration) {
        self.command_exists_calls.push(CommandExistsCall {
            command: command.to_string(),
            found,
            duration,
        });
    }

    pub fn phases(&self) -> &[Phase] {
        &self.phases
    }

    pub fn command_exists_calls(&self) -> &[CommandExistsCall] {
        &self.command_exists_calls
    }

    pub fn total_duration(&self) -> Duration {
        self.phases.iter().map(|p| p.duration).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timings_new_is_empty() {
        let t = Timings::new();
        assert!(t.phases().is_empty());
        assert!(t.command_exists_calls().is_empty());
        assert_eq!(t.total_duration(), Duration::ZERO);
    }

    #[test]
    fn timings_record_phase() {
        let mut t = Timings::new();
        t.record_phase("config_load", Duration::from_micros(1230));
        assert_eq!(t.phases().len(), 1);
        assert_eq!(t.phases()[0].name, "config_load");
        assert_eq!(t.phases()[0].duration, Duration::from_micros(1230));
    }

    #[test]
    fn timings_record_command_exists_call() {
        let mut t = Timings::new();
        t.record_command_exists("git", true, Duration::from_micros(2340));
        t.record_command_exists("lsd", false, Duration::from_micros(3120));
        assert_eq!(t.command_exists_calls().len(), 2);
        assert_eq!(t.command_exists_calls()[0].command, "git");
        assert!(t.command_exists_calls()[0].found);
        assert_eq!(t.command_exists_calls()[1].command, "lsd");
        assert!(!t.command_exists_calls()[1].found);
    }

    #[test]
    fn timings_total_duration() {
        let mut t = Timings::new();
        t.record_phase("a", Duration::from_micros(100));
        t.record_phase("b", Duration::from_micros(200));
        assert_eq!(t.total_duration(), Duration::from_micros(300));
    }

    #[test]
    fn phase_timer_elapsed_is_positive() {
        let timer = PhaseTimer::start();
        std::thread::sleep(Duration::from_millis(1));
        assert!(timer.elapsed() >= Duration::from_millis(1));
    }
}
