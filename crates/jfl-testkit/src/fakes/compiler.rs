//! [`ScriptedWorker`]: a compile worker the test steps by hand (LSP-04; `doc/formal-models.md`,
//! "`diagnostics`", obligation 7), for the diagnostics coordinator's tests and its conformance
//! driver.
//!
//! It runs the jobs the coordinator starts, several at once, and finishes them in whatever order
//! the test says ([`ScriptedWorker::complete`]); a finished job's reply waits in the "pipe" until
//! the test reads it ([`ScriptedWorker::take`]). It crashes on demand, losing its running jobs but
//! not the replies it wrote, and is spawned again with the next incarnation.
//!
//! Its diagnostics **echo what the job was computed for** ([`Echo`]): the session, the epoch, the
//! worker incarnation and the text of every input it read, so a client that receives them can say
//! exactly which compile it is shown ([`Echo::parse`]). Texts are echoed by content, which the
//! tests keep short (a number); a file it did not find is `-`.
//!
//! Not behind a port yet: the `Compiler` port (plan §6.4) and its `compiler` contract suite, which
//! this worker and `jfl-adapter-javac` will run (ver §5), land with SEM-04; until then the caller
//! steps it by hand, as the coordinator's shell will step the real worker.
//!
//! Pinned by `compiler::tests`.

use std::collections::BTreeMap;
use std::fmt::Display;

use jfl_application::diagnostics::{Incarnation, Job, JobId, Reply};

/// What a scripted reply says it was computed for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Echo {
    /// The document's open session.
    pub session: u32,
    /// The document's input epoch.
    pub epoch: u64,
    /// The worker incarnation that compiled it.
    pub incarnation: u32,
    /// Each input file's name and the text read (`None`: not found).
    pub inputs: BTreeMap<String, Option<String>>,
}

impl Echo {
    /// What `job` was computed for.
    pub fn of<K: Display>(job: &Job<K>) -> Self {
        Self {
            session: job.session.0,
            epoch: job.epoch.0,
            incarnation: job.incarnation.0,
            inputs: job
                .inputs
                .iter()
                .map(|(k, t)| (k.to_string(), t.as_ref().map(|t| t.content().to_string())))
                .collect(),
        }
    }

    /// The diagnostic's message: `session=S epoch=E inc=I name=text ...` (`-` for not found).
    pub fn message(&self) -> String {
        let mut line = format!(
            "session={} epoch={} inc={}",
            self.session, self.epoch, self.incarnation
        );
        for (name, text) in &self.inputs {
            line.push(' ');
            line.push_str(name);
            line.push('=');
            line.push_str(text.as_deref().unwrap_or("-"));
        }
        line
    }

    /// Reads a [`Echo::message`] back; `None` for anything else.
    pub fn parse(message: &str) -> Option<Self> {
        let mut words = message.split(' ');
        let mut field = |name: &str| {
            words
                .next()
                .and_then(|w| w.strip_prefix(name))
                .and_then(|w| w.strip_prefix('='))
                .map(str::to_owned)
        };
        let session = field("session")?.parse().ok()?;
        let epoch = field("epoch")?.parse().ok()?;
        let incarnation = field("inc")?.parse().ok()?;
        let mut inputs = BTreeMap::new();
        for word in words {
            let (name, text) = word.split_once('=')?;
            let text = (text != "-").then(|| text.to_owned());
            inputs.insert(name.to_owned(), text);
        }
        Some(Self {
            session,
            epoch,
            incarnation,
            inputs,
        })
    }
}

/// Where a job is in the worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    /// Running.
    Running,
    /// Finished: its reply is written and not read yet.
    Done,
    /// Not in the worker: never given, read already, or lost by a crash.
    Absent,
}

/// The scripted worker. Its replies' diagnostics are one message, the job's [`Echo`].
#[derive(Debug, Clone, Default)]
pub struct ScriptedWorker {
    up: bool,
    incarnation: u32,
    lost: u32,
    running: BTreeMap<JobId, Echo>,
    done: BTreeMap<JobId, Echo>,
}

impl ScriptedWorker {
    /// A worker never spawned.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether it is up.
    pub fn up(&self) -> bool {
        self.up
    }

    /// Its incarnation: 0 before the first spawn.
    pub fn incarnation(&self) -> Incarnation {
        Incarnation(self.incarnation)
    }

    /// How many running jobs its crashes lost.
    pub fn lost(&self) -> u32 {
        self.lost
    }

    /// Spawns it, as the next incarnation; refused while it is up.
    pub fn spawn(&mut self) -> Result<Incarnation, String> {
        if self.up {
            return Err("the worker is up already".to_owned());
        }
        self.incarnation = self.incarnation.saturating_add(1);
        self.up = true;
        Ok(self.incarnation())
    }

    /// Takes `job` to compile it; refused unless it is up and the job is for this incarnation.
    pub fn submit<K: Display>(&mut self, job: &Job<K>) -> Result<(), String> {
        if !self.up || job.incarnation != self.incarnation() {
            return Err(format!(
                "job {} is for incarnation {}, the worker is {}{}",
                job.id.0,
                job.incarnation.0,
                self.incarnation,
                if self.up { "" } else { " (down)" }
            ));
        }
        self.running.insert(job.id, Echo::of(job));
        Ok(())
    }

    /// Finishes the running job `id`: its reply is written.
    pub fn complete(&mut self, id: JobId) -> Result<(), String> {
        let echo = self
            .running
            .remove(&id)
            .ok_or_else(|| format!("job {} is not running", id.0))?;
        self.done.insert(id, echo);
        Ok(())
    }

    /// Reads the reply of job `id`, once it is written.
    pub fn take(&mut self, id: JobId) -> Option<Reply<Vec<String>>> {
        self.done.remove(&id).map(|echo| Reply {
            job: id,
            diagnostics: vec![echo.message()],
        })
    }

    /// Dies: the running jobs are lost (returned); the written replies stay to be read.
    pub fn crash(&mut self) -> Result<Vec<JobId>, String> {
        if !self.up {
            return Err("the worker is down".to_owned());
        }
        self.up = false;
        let lost: Vec<JobId> = std::mem::take(&mut self.running).into_keys().collect();
        self.lost = self
            .lost
            .saturating_add(u32::try_from(lost.len()).unwrap_or(u32::MAX));
        Ok(lost)
    }

    /// Where job `id` is.
    pub fn state(&self, id: JobId) -> JobState {
        if self.running.contains_key(&id) {
            JobState::Running
        } else if self.done.contains_key(&id) {
            JobState::Done
        } else {
            JobState::Absent
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jfl_application::diagnostics::{Coordinator, Text};

    #[test]
    fn should_echo_what_the_job_read_when_its_reply_is_parsed() {
        // given
        let mut coordinator: Coordinator<String> =
            Coordinator::new(3, [("b".to_owned(), Text::new(2, "2"))]);
        let _ = coordinator.track("a".to_owned(), ["b".to_owned(), "c".to_owned()]);
        let _ = coordinator.did_open("a".to_owned(), 1, Text::new(1, "1"));
        let mut worker = ScriptedWorker::new();
        let inc = worker.spawn().expect("spawns");
        coordinator.worker_started(inc).expect("up");

        // when
        let job = coordinator.start(&"a".to_owned()).expect("the job starts");
        worker.submit(&job).expect("the worker takes it");
        worker.complete(job.id).expect("it finishes");
        let reply = worker.take(job.id).expect("its reply is written");

        // then
        let echo = reply.diagnostics.first().and_then(|m| Echo::parse(m));
        assert_eq!(
            echo,
            Some(Echo {
                session: 1,
                epoch: 1,
                incarnation: 1,
                inputs: [
                    ("a".to_owned(), Some("1".to_owned())),
                    ("b".to_owned(), Some("2".to_owned())),
                    ("c".to_owned(), None),
                ]
                .into_iter()
                .collect(),
            })
        );
        assert_eq!(worker.state(job.id), JobState::Absent);
    }

    #[test]
    fn should_keep_written_replies_and_lose_running_jobs_when_it_crashes() {
        // given
        let mut coordinator: Coordinator<String> = Coordinator::new(3, []);
        let mut worker = ScriptedWorker::new();
        let inc = worker.spawn().expect("spawns");
        coordinator.worker_started(inc).expect("up");
        let _ = coordinator.did_open("a".to_owned(), 1, Text::new(1, "1"));
        let first = coordinator.start(&"a".to_owned()).expect("starts");
        worker.submit(&first).expect("taken");
        worker.complete(first.id).expect("finishes");
        let _ = coordinator.did_change("a".to_owned(), 2, Text::new(2, "2"));
        let second = coordinator.start(&"a".to_owned()).expect("starts");
        worker.submit(&second).expect("taken");

        // when
        let lost = worker.crash().expect("crashes");

        // then
        assert_eq!(lost, vec![second.id]);
        assert_eq!(worker.lost(), 1);
        assert_eq!(worker.state(first.id), JobState::Done);
        assert!(
            worker.submit(&second).is_err(),
            "a dead worker takes nothing"
        );
        assert_eq!(worker.spawn().map(|i| i.0), Ok(2));
    }

    #[test]
    fn should_read_nothing_when_the_message_is_not_an_echo() {
        // given
        let messages = [
            "",
            "session=1",
            "epoch=1 session=1 inc=1",
            "session=x epoch=1 inc=1",
        ];

        // when
        let parsed: Vec<Option<Echo>> = messages.iter().map(|m| Echo::parse(m)).collect();

        // then
        assert!(parsed.iter().all(Option::is_none), "{parsed:?}");
    }
}
