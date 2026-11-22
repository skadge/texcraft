//! A backend on its own thread. At most one job runs; a job submitted while one runs waits, and a
//! newer submission replaces a waiting one, so typing fast never queues a backlog of builds.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use crate::{Backend, Job, Outcome};

pub struct Worker {
    jobs: Sender<Job>,
    outcomes: Receiver<Outcome>,
    running: Option<u64>,
    waiting: Option<Job>,
    name: String,
    can_cancel: bool,
}

impl Worker {
    pub fn new(mut backend: Box<dyn Backend>) -> Worker {
        let (jobs, job_rx) = channel::<Job>();
        let (out_tx, outcomes) = channel();
        let name = backend.name();
        let can_cancel = backend.can_cancel();
        let spawned = std::thread::Builder::new().name("texcraft-compile".to_owned()).spawn(move || {
            for job in job_rx {
                let id = job.id;
                let outcome = catch_unwind(AssertUnwindSafe(|| backend.compile(&job)))
                    .unwrap_or_else(|_| Outcome::unavailable(id, "The typesetting engine stopped unexpectedly; the build was abandoned."));
                if out_tx.send(outcome).is_err() {
                    break;
                }
            }
        });
        if let Err(e) = spawned {
            log::error!("cannot start the compile thread: {e}");
        }
        Worker { jobs, outcomes, running: None, waiting: None, name, can_cancel }
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
            Ok(()) => self.running = Some(id),
            Err(_) => log::error!("the compile thread is gone; job {id} was not started"),
        }
    }

    /// The job running, if any.
    pub fn running(&self) -> Option<u64> {
        self.running
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
