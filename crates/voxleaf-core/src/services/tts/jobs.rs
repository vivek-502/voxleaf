//! In-flight TTS generations, so a request can start one and return.
//!
//! Synthesis is measured in minutes, not milliseconds: a chapter is split into
//! chunks and each one is a round trip to Edge TTS. Holding a request open for
//! that long is unreliable — proxies and browsers give up, and a cancelled
//! request leaves the chapter with no audio at all. So a generation is a job
//! with a status the client polls, and the work continues on the server
//! regardless of whether anyone is watching.
//!
//! The registry is in-process on purpose. There is one user and one server,
//! and a job that dies with the process is a job whose chapter simply has no
//! audio yet — the same state as before the button was pressed, and
//! recoverable by pressing it again.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::services::tts::narration::NarrationSource;

/// Jobs are forgotten a while after they finish, so the map cannot grow
/// forever.
const RETAIN: Duration = Duration::from_secs(10 * 60);

/// Guard against unbounded growth if a book is generated over and over.
const MAX_JOBS: usize = 50;

/// What a generation is doing right now.
#[derive(Debug, Clone)]
pub(crate) struct TtsJob {
    pub chapter_id: String,
    pub state: JobState,
    /// Chunks finished so far.
    pub completed: i64,
    pub total: i64,
    /// Human-readable reason, set when `state` is `failed`.
    pub error: Option<String>,
    pub audio_id: Option<String>,
    /// What is being narrated, `None` until the source has been resolved,
    /// moments after the job starts.
    pub narration: Option<NarrationSource>,
    started_at: Instant,
    finished_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobState {
    Running,
    Done,
    Failed,
}

impl JobState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            JobState::Running => "running",
            JobState::Done => "done",
            JobState::Failed => "failed",
        }
    }
}

/// The shared registry. Cheap to clone; every handle sees the same jobs.
pub(crate) type JobRegistry = Arc<Mutex<HashMap<String, TtsJob>>>;

pub(crate) fn new_registry() -> JobRegistry {
    Arc::new(Mutex::new(HashMap::new()))
}

fn prune(jobs: &mut HashMap<String, TtsJob>) {
    let now = Instant::now();
    jobs.retain(|_, job| match job.finished_at {
        Some(finished) => now.duration_since(finished) <= RETAIN,
        None => true,
    });
    // Still full of live jobs: drop the oldest rather than refuse to track.
    if jobs.len() >= MAX_JOBS {
        let oldest = jobs
            .iter()
            .min_by_key(|(_, job)| job.started_at)
            .map(|(chapter_id, _)| chapter_id.clone());
        if let Some(oldest) = oldest {
            jobs.remove(&oldest);
        }
    }
}

/// The job for a chapter, if one is running or has just finished.
pub(crate) fn get(registry: &JobRegistry, chapter_id: &str) -> Option<TtsJob> {
    let mut jobs = registry.lock().ok()?;
    prune(&mut jobs);
    jobs.get(chapter_id).cloned()
}

/// Claim the chapter for generation.
///
/// Returns `None` when a job is already running, so a second click cannot
/// start a second synthesis of the same text — the two would race to write
/// the same audio row and the slower one would win for no reason.
pub(crate) fn start(registry: &JobRegistry, chapter_id: &str, total: i64) -> Option<TtsJob> {
    let mut jobs = registry.lock().ok()?;
    prune(&mut jobs);

    if let Some(existing) = jobs.get(chapter_id) {
        if existing.state == JobState::Running && existing.finished_at.is_none() {
            return None;
        }
    }

    let job = TtsJob {
        chapter_id: chapter_id.to_string(),
        state: JobState::Running,
        completed: 0,
        total,
        error: None,
        audio_id: None,
        narration: None,
        started_at: Instant::now(),
        finished_at: None,
    };
    jobs.insert(chapter_id.to_string(), job.clone());
    Some(job)
}

/// Record what is being narrated.
///
/// Set once the source is resolved, which is before the first chunk is
/// synthesized — so the client can say which language and voice the job
/// settled on while it is still running, rather than only after it has
/// finished.
pub(crate) fn set_narration(registry: &JobRegistry, chapter_id: &str, narration: NarrationSource) {
    if let Ok(mut jobs) = registry.lock() {
        if let Some(job) = jobs.get_mut(chapter_id) {
            job.narration = Some(narration);
        }
    }
}

pub(crate) fn advance(registry: &JobRegistry, chapter_id: &str, completed: i64) {
    if let Ok(mut jobs) = registry.lock() {
        if let Some(job) = jobs.get_mut(chapter_id) {
            job.completed = completed.min(job.total);
        }
    }
}

pub(crate) fn finish(registry: &JobRegistry, chapter_id: &str, audio_id: String) {
    if let Ok(mut jobs) = registry.lock() {
        if let Some(job) = jobs.get_mut(chapter_id) {
            job.state = JobState::Done;
            job.completed = job.total;
            job.audio_id = Some(audio_id);
            job.finished_at = Some(Instant::now());
        }
    }
}

pub(crate) fn fail(registry: &JobRegistry, chapter_id: &str, error: String) {
    if let Ok(mut jobs) = registry.lock() {
        if let Some(job) = jobs.get_mut(chapter_id) {
            job.state = JobState::Failed;
            job.error = Some(error);
            job.finished_at = Some(Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_job_cannot_be_claimed_twice() {
        let registry = new_registry();
        assert!(start(&registry, "c1", 4).is_some());
        assert!(start(&registry, "c1", 4).is_none());
    }

    #[test]
    fn a_finished_job_can_be_claimed_again() {
        let registry = new_registry();
        start(&registry, "c1", 2);
        finish(&registry, "c1", "au_1".to_string());
        assert!(start(&registry, "c1", 2).is_some());
    }

    #[test]
    fn progress_never_exceeds_the_total() {
        let registry = new_registry();
        start(&registry, "c1", 3);
        advance(&registry, "c1", 99);
        assert_eq!(get(&registry, "c1").unwrap().completed, 3);
    }

    #[test]
    fn narration_is_recorded_for_the_polling_client() {
        let registry = new_registry();
        start(&registry, "c1", 1);
        let source = NarrationSource {
            text: "hello".to_string(),
            language: Some("hi".to_string()),
            language_name: Some("Hindi".to_string()),
            voice: "hi-IN-SwaraNeural".to_string(),
            fell_back_to_original: false,
            reason: None,
        };
        set_narration(&registry, "c1", source);
        let job = get(&registry, "c1").unwrap();
        assert_eq!(job.narration.unwrap().voice, "hi-IN-SwaraNeural");
    }

    #[test]
    fn failures_carry_their_reason() {
        let registry = new_registry();
        start(&registry, "c1", 1);
        fail(&registry, "c1", "No audio was received.".to_string());
        let job = get(&registry, "c1").unwrap();
        assert_eq!(job.state, JobState::Failed);
        assert_eq!(job.error.as_deref(), Some("No audio was received."));
    }
}
