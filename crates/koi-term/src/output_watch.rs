//! Whether a program is working, told from its output: terminal agents
//! animate a spinner or a timer while they work and write nothing while
//! idle. They start working only on a submitted prompt, so animations set
//! off by a click, such as a spinning logo, are not work; output just after
//! any input is that input's echo.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Output this soon after input is its echo.
const ECHO: Duration = Duration::from_millis(300);
/// Output is working when it keeps coming over this long…
const SPAN: Duration = Duration::from_millis(800);
/// …within this window.
const WINDOW: Duration = Duration::from_millis(2500);
/// Work starts this soon after a submission, or it was not set off by one.
const START: Duration = Duration::from_secs(5);
/// No output for this long is idle; agents refresh their timers each second.
const QUIET: Duration = Duration::from_secs(3);

/// Output and input times of a terminal.
pub struct OutputWatch {
    input: Option<Instant>,
    submitted: Option<Instant>,
    /// Output times within [`WINDOW`], oldest first, echoes left out.
    recent: VecDeque<Instant>,
    /// Latest output, or the start.
    last: Instant,
    working: bool,
}

impl OutputWatch {
    pub fn new(now: Instant) -> Self {
        Self {
            input: None,
            submitted: None,
            recent: VecDeque::new(),
            last: now,
            working: false,
        }
    }

    /// The user typed, pasted, clicked or resized the terminal.
    pub fn input(&mut self, now: Instant) {
        self.input = Some(now);
    }

    /// The user submitted a line with enter.
    pub fn submit(&mut self, now: Instant) {
        self.input = Some(now);
        self.submitted = Some(now);
    }

    /// The program wrote output.
    pub fn output(&mut self, now: Instant) {
        if self
            .input
            .is_some_and(|input| now.saturating_duration_since(input) < ECHO)
        {
            return;
        }
        self.last = now;
        self.recent.push_back(now);
        while self
            .recent
            .front()
            .is_some_and(|&first| now.saturating_duration_since(first) > WINDOW)
        {
            self.recent.pop_front();
        }
    }

    /// `Some(true)` while output set off by a submission keeps coming,
    /// `Some(false)` once output has stopped for a while, `None` otherwise.
    pub fn busy(&mut self, now: Instant) -> Option<bool> {
        if now.saturating_duration_since(self.last) >= QUIET {
            self.working = false;
            return Some(false);
        }
        let mut recent = self
            .recent
            .iter()
            .filter(|&&time| now.saturating_duration_since(time) <= WINDOW);
        let first = *recent.next()?;
        let last = recent.next_back().copied().unwrap_or(first);
        let started = self
            .submitted
            .is_some_and(|submitted| now.saturating_duration_since(submitted) <= START);
        self.working |= started && last.saturating_duration_since(first) >= SPAN;
        self.working.then_some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    /// Output every 100 ms from `from` to `to`.
    fn spin(watch: &mut OutputWatch, start: Instant, from: u64, to: u64) {
        for at in (from..to).step_by(100) {
            watch.output(ms(start, at));
        }
    }

    #[test]
    fn a_spinner_after_a_prompt_is_work_and_silence_is_idle() {
        let start = Instant::now();
        let mut watch = OutputWatch::new(start);
        watch.submit(ms(start, 1000));
        spin(&mut watch, start, 1100, 3000);
        assert_eq!(watch.busy(ms(start, 3000)), Some(true));
        // Work goes on past the window in which it had to start.
        spin(&mut watch, start, 3000, 9000);
        assert_eq!(watch.busy(ms(start, 9000)), Some(true));
        assert_eq!(watch.busy(ms(start, 12000)), Some(false));
    }

    #[test]
    fn a_timer_ticking_each_second_is_work() {
        let start = Instant::now();
        let mut watch = OutputWatch::new(start);
        watch.submit(start);
        for second in 1..=3 {
            watch.output(ms(start, second * 1000));
        }
        assert_eq!(watch.busy(ms(start, 3500)), Some(true));
    }

    #[test]
    fn an_animation_set_off_by_a_click_is_not_work() {
        let start = Instant::now();
        let mut watch = OutputWatch::new(start);
        watch.input(ms(start, 1000));
        spin(&mut watch, start, 1100, 4000);
        assert_eq!(watch.busy(ms(start, 4000)), None);
        assert_eq!(watch.busy(ms(start, 7000)), Some(false));
    }

    #[test]
    fn startup_output_is_not_work() {
        let start = Instant::now();
        let mut watch = OutputWatch::new(start);
        spin(&mut watch, start, 0, 2000);
        assert_eq!(watch.busy(ms(start, 2000)), None);
        assert_eq!(watch.busy(ms(start, 5000)), Some(false));
    }

    #[test]
    fn echoes_of_typing_are_not_work() {
        let start = Instant::now();
        let mut watch = OutputWatch::new(start);
        for at in (0..3000).step_by(150) {
            watch.input(ms(start, at));
            watch.output(ms(start, at + 20));
        }
        // Only echoes since the start: the program has been idle all along.
        assert_eq!(watch.busy(ms(start, 3000)), Some(false));
    }

    #[test]
    fn a_single_redraw_is_not_work() {
        let start = Instant::now();
        let mut watch = OutputWatch::new(start);
        watch.submit(start);
        watch.output(ms(start, 1000));
        watch.output(ms(start, 1050));
        assert_eq!(watch.busy(ms(start, 1100)), None);
    }
}
