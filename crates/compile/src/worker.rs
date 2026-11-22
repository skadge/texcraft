//! A backend on its own thread. At most one job runs; a job submitted while one runs waits, and a
//! newer submission replaces a waiting one, so typing fast never queues a backlog of builds.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use std::time::Instant;

use crate::{Backend, Event, Job, Outcome, Progress};

pub struct Worker {
    jobs: Sender<Job>,
    outcomes: Receiver<Outcome>,
    events: Receiver<(u64, Event)>,
    /// The running job's progress, and when it started.
    progress: Progress,
    started: Option<Instant>,
    running: Option<u64>,
    waiting: Option<Job>,
    name: String,
    can_cancel: bool,
    canceller: Option<crate::Canceller>,
    cleaner: Option<crate::Cleaner>,
}

impl Worker {
    pub fn new(mut backend: Box<dyn Backend>) -> Worker {
        let (jobs, job_rx) = channel::<Job>();
        let (out_tx, outcomes) = channel();
        let (event_tx, events) = channel();
        let name = backend.name();
        let can_cancel = backend.can_cancel();
        let canceller = backend.canceller();
        let cleaner = backend.cleaner();
        let spawned = std::thread::Builder::new().name("texcraft-compile".to_owned()).spawn(move || {
            for job in job_rx {
                let id = job.id;
                let events = event_tx.clone();
                let mut report = move |e: Event| {
                    let _ = events.send((id, e));
                };
                let outcome = catch_unwind(AssertUnwindSafe(|| backend.compile_reporting(&job, &mut report)))
                    .unwrap_or_else(|_| Outcome::unavailable(id, "The typesetting engine stopped unexpectedly; the build was abandoned."));
                if out_tx.send(outcome).is_err() {
                    break;
                }
            }
        });
        if let Err(e) = spawned {
            log::error!("cannot start the compile thread: {e}");
        }
        Worker {
            jobs,
            outcomes,
            events,
            progress: Progress::default(),
            started: None,
            running: None,
            waiting: None,
            name,
            can_cancel,
            canceller,
            cleaner,
        }
    }

    pub fn backend_name(&self) -> &str {
        &self.name
    }

    pub fn can_cancel(&self) -> bool {
        self.can_cancel
    }

    /// Starts `job` now, or after the running one (replacing any job already waiting).
    pub fn submit(&mut self, job: Job) {
        if self.running.is_some() {
            self.waiting = Some(job);
        } else {
            self.start(job);
        }
    }

    fn start(&mut self, job: Job) {
        let id = job.id;
        match self.jobs.send(job) {
            Ok(()) => {
                self.running = Some(id);
                self.progress = Progress::default();
                self.started = Some(Instant::now());
            }
            Err(_) => log::error!("the compile thread is gone; job {id} was not started"),
        }
    }

    /// Stops the running build (it ends with a cancelled outcome) and drops a waiting one.
    pub fn cancel(&mut self) -> bool {
        self.waiting = None;
        match (&self.canceller, self.running) {
            (Some(cancel), Some(_)) => {
                cancel();
                true
            }
            _ => false,
        }
    }

    /// Deletes the backend's build files; refused while a build runs or waits.
    pub fn clean(&mut self) -> Result<(), String> {
        if self.running.is_some() || self.waiting.is_some() {
            return Err("A build is running: stop it or wait for it first.".to_owned());
        }
        match &self.cleaner {
            Some(clean) => clean(),
            None => Err("This typesetting engine keeps no build files.".to_owned()),
        }
    }

    /// Whether the backend keeps build files that can be deleted.
    pub fn can_clean(&self) -> bool {
        self.cleaner.is_some()
    }

    /// The job running, if any.
    pub fn running(&self) -> Option<u64> {
        self.running
    }

    /// The running job's progress and how long it has run, in seconds.
    pub fn progress(&mut self) -> Option<(Progress, f64)> {
        let running = self.running?;
        while let Ok((id, event)) = self.events.try_recv() {
            if id == running {
                self.progress.apply(event);
            }
        }
        Some((self.progress.clone(), self.started.map_or(0.0, |s| s.elapsed().as_secs_f64())))
    }

    /// A finished outcome, without blocking.
    pub fn poll(&mut self) -> Option<Outcome> {
        match self.outcomes.try_recv() {
            Ok(outcome) => {
                self.running = None;
                if let Some(next) = self.waiting.take() {
                    self.start(next);
                }
                Some(outcome)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                let id = self.running.take()?;
                Some(Outcome::unavailable(id, "The typesetting engine is not running."))
            }
        }
    }

    /// Waits for the running job (the CLI and the agent tools build synchronously).
    pub fn wait(&mut self) -> Option<Outcome> {
        self.running?;
        match self.outcomes.recv() {
            Ok(outcome) => {
                self.running = None;
                Some(outcome)
            }
            Err(_) => self.running.take().map(|id| Outcome::unavailable(id, "The typesetting engine is not running.")),
        }
    }
}
