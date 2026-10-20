//! Keys, pastes and resizes from the terminal, read on a thread of their own
//! that steps aside while another program (mpv) has the terminal.
//!
//! crossterm's `EventStream` cannot be set aside that way: dropping it wakes
//! crossterm's shared input reader even when nothing waits on it (`Drop for
//! EventStream`), and the leftover wake-up makes the next query of the
//! terminal, such as the cursor position ratatui reads to clear the screen,
//! give up at once.

use std::io;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

/// How long a read waits for input, hence the longest `pause` waits.
const TICK: Duration = Duration::from_millis(50);

pub struct TerminalEvents {
    events: UnboundedReceiver<io::Result<Event>>,
    gate: Arc<Gate>,
}

impl TerminalEvents {
    /// Starts reading the terminal, which must be in raw mode.
    pub fn spawn() -> Self {
        Self::with_reader(read_terminal)
    }

    /// `read` returns the next event, or `None` when there was none for a
    /// while.
    fn with_reader(read: impl FnMut() -> Option<io::Result<Event>> + Send + 'static) -> Self {
        let (sender, events) = mpsc::unbounded_channel();
        let gate = Arc::new(Gate::default());
        let reader_gate = gate.clone();
        thread::spawn(move || forward(&reader_gate, &sender, read));
        Self { events, gate }
    }

    pub async fn next(&mut self) -> Option<io::Result<Event>> {
        self.events.recv().await
    }

    /// Stops reading the terminal until `resume`, so that another program
    /// gets every key. Blocks until the read in progress ends, at most
    /// `TICK`.
    pub fn pause(&self) {
        self.gate.pause();
    }

    pub fn resume(&self) {
        self.gate.resume();
    }
}

fn read_terminal() -> Option<io::Result<Event>> {
    match event::poll(TICK) {
        Ok(true) => Some(event::read()),
        Ok(false) => None,
        Err(e) => Some(Err(e)),
    }
}

/// Sends what `read` returns until the receiving side is gone, reading only
/// while `gate` allows it.
fn forward(
    gate: &Gate,
    events: &UnboundedSender<io::Result<Event>>,
    mut read: impl FnMut() -> Option<io::Result<Event>>,
) {
    while !events.is_closed() {
        gate.enter();
        let event = read();
        gate.leave();
        if let Some(event) = event
            && events.send(event).is_err()
        {
            return;
        }
    }
}

/// Lets the reader thread read only while not paused, and tells `pause`
/// when the read in progress is over.
#[derive(Default)]
struct Gate {
    state: Mutex<GateState>,
    changed: Condvar,
}

#[derive(Default)]
struct GateState {
    paused: bool,
    reading: bool,
}

impl Gate {
    fn enter(&self) {
        let mut state = self.state.lock().unwrap();
        while state.paused {
            state = self.changed.wait(state).unwrap();
        }
        state.reading = true;
    }

    fn leave(&self) {
        self.state.lock().unwrap().reading = false;
        self.changed.notify_all();
    }

    fn pause(&self) {
        let mut state = self.state.lock().unwrap();
        state.paused = true;
        while state.reading {
            state = self.changed.wait(state).unwrap();
        }
    }

    fn resume(&self) {
        self.state.lock().unwrap().paused = false;
        self.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::sync::mpsc as std_mpsc;

    /// Long enough for the reader thread to have acted, were it to.
    const SHORT: Duration = Duration::from_millis(100);
    const LONG: Duration = Duration::from_secs(2);

    fn key(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    /// Events read from a fake terminal: each read reports its start on the
    /// returned receiver, then lasts until the returned sender gives the
    /// event it reads.
    fn controlled() -> (
        TerminalEvents,
        std_mpsc::Receiver<()>,
        std_mpsc::Sender<Event>,
    ) {
        let (started, reads) = std_mpsc::channel();
        let (typed, keyboard) = std_mpsc::channel::<Event>();
        let events = TerminalEvents::with_reader(move || {
            started.send(()).ok()?;
            keyboard.recv().ok().map(Ok)
        });
        (events, reads, typed)
    }

    /// Calls `pause` on another thread, since it blocks, and returns once
    /// the pause is requested; the receiver hears once `pause` returned.
    fn pause_elsewhere(events: &TerminalEvents) -> std_mpsc::Receiver<()> {
        let gate = events.gate.clone();
        let (done, paused) = std_mpsc::channel();
        thread::spawn(move || {
            gate.pause();
            let _ = done.send(());
        });
        while !events.gate.state.lock().unwrap().paused {
            thread::sleep(Duration::from_millis(1));
        }
        paused
    }

    #[test]
    fn forwards_what_the_terminal_sends() {
        let (mut events, reads, typed) = controlled();
        reads.recv_timeout(LONG).unwrap();
        typed.send(key('a')).unwrap();
        reads.recv_timeout(LONG).unwrap();
        typed.send(key('b')).unwrap();

        assert_eq!(events.events.blocking_recv().unwrap().unwrap(), key('a'));
        assert_eq!(events.events.blocking_recv().unwrap().unwrap(), key('b'));
    }

    #[test]
    fn pausing_waits_for_the_read_in_progress() {
        let (events, reads, typed) = controlled();
        reads.recv_timeout(LONG).unwrap();

        let paused = pause_elsewhere(&events);
        assert!(paused.recv_timeout(SHORT).is_err(), "paused mid-read");
        typed.send(key('a')).unwrap();
        assert!(paused.recv_timeout(LONG).is_ok());
    }

    #[test]
    fn reads_nothing_while_paused() {
        let (mut events, reads, typed) = controlled();
        reads.recv_timeout(LONG).unwrap();
        let paused = pause_elsewhere(&events);
        typed.send(key('a')).unwrap();
        paused.recv_timeout(LONG).unwrap();

        assert!(reads.recv_timeout(SHORT).is_err(), "read while paused");
        assert_eq!(events.events.blocking_recv().unwrap().unwrap(), key('a'));

        events.resume();
        assert!(reads.recv_timeout(LONG).is_ok());
    }
}
