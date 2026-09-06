use super::paths::AppPaths;
use std::fs;
use std::path::Path;

pub struct CleanupManager;

impl CleanupManager {
    /// Cleans up temporary segment audio files (*.wav, *.mp3 segment chunks) in the job directory
    pub fn cleanup_temp_segments(job_dir: &Path) {
        if let Ok(entries) = fs::read_dir(job_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                    if file_name.starts_with("seg_")
                        || file_name.starts_with("align_")
                        || file_name.ends_with(".tmp")
                        || file_name == "concat_list.txt"
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
}
