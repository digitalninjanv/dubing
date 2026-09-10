use super::paths::AppPaths;
use std::fs;
use std::path::Path;

pub struct CleanupManager;

impl CleanupManager {
    /// Cleans up temporary segment audio files and intermediate artifacts in the job directory (F8).
    pub fn cleanup_temp_segments(job_dir: &Path) {
        if let Ok(entries) = fs::read_dir(job_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                    if file_name.starts_with("seg_")
                        || file_name.starts_with("align_")
                        || file_name.starts_with("silence_")
                        || file_name.starts_with("ducked_")
                        || file_name.starts_with("dubbed_")
                        || file_name == "extracted_source_audio.mp3"
                        || file_name == "input_16k.pcm"
                        || file_name == "output_24k.pcm"
                        || file_name == "input.pcm"
                        || file_name == "output.pcm"
                        || file_name == "concat_list.txt"
                        || file_name.ends_with(".tmp")
                        || file_name == "transcript.json.tmp"
                        || file_name == "translated.json.tmp"
                    {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }
    }

    /// Reconciles orphan directories on startup
    pub fn startup_reconciliation() {
        let jobs_dir = AppPaths::jobs_dir();
        if !jobs_dir.exists() {
            return;
        }

        if let Ok(entries) = fs::read_dir(jobs_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let job_file = path.join("job.json");
                    // If directory has no job.json, it is an orphan temporary directory from a hard crash
                    if !job_file.exists() {
                        let _ = fs::remove_dir_all(&path);
                    }
                }
            }
        }
    }

    /// Deletes terminal job dirs older than `retention_days` (0 = keep all).
    /// Only touches jobs in a terminal stage; running/retryable jobs are kept.
    pub fn retention_sweep(retention_days: u64) {
        Self::retention_sweep_in(&AppPaths::jobs_dir(), retention_days);
    }

    pub fn retention_sweep_in(jobs_dir: &std::path::Path, retention_days: u64) {
        if retention_days == 0 {
            return;
        }
        if !jobs_dir.exists() {
            return;
        }
        let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
        let Ok(entries) = fs::read_dir(jobs_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest = path.join("job.json");
            let Ok(content) = fs::read_to_string(&manifest) else {
                continue;
            };
            let Ok(job) = serde_json::from_str::<crate::domain::Job>(&content) else {
                continue;
            };
            if job.stage.is_terminal() && job.updated_at < cutoff {
                tracing::info!("Removing retained job dir {}", path.display());
                let _ = fs::remove_dir_all(&path);
            }
        }
    }
}
