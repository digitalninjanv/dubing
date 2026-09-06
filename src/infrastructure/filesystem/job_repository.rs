use super::paths::AppPaths;
use crate::application::ports::JobRepository;
use crate::domain::{DomainError, Job, JobId};
use async_trait::async_trait;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

pub struct FileJobRepository {
    base_dir: PathBuf,
}

impl FileJobRepository {
    pub fn new() -> Self {
        let base_dir = AppPaths::jobs_dir();
        let _ = fs::create_dir_all(&base_dir);
        Self { base_dir }
    }

    pub fn with_dir(base_dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&base_dir);
        Self { base_dir }
    }

    fn job_file(&self, id: &JobId) -> PathBuf {
        self.base_dir.join(id.as_str()).join("job.json")
    }
}

impl Default for FileJobRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl JobRepository for FileJobRepository {
    async fn save(&self, job: &Job) -> Result<(), DomainError> {
        let job_dir = self.base_dir.join(job.id.as_str());
        fs::create_dir_all(&job_dir)
            .map_err(|e| DomainError::Internal(format!("Failed to create job dir: {}", e)))?;

        let final_path = job_dir.join("job.json");
        let tmp_path = job_dir.join("job.json.tmp");

        let json_data = serde_json::to_vec_pretty(job)
            .map_err(|e| DomainError::Internal(format!("Failed to serialize job: {}", e)))?;

        // Atomic write: write to .tmp, fsync, then rename
        let mut file = File::create(&tmp_path)
            .map_err(|e| DomainError::Internal(format!("Failed to create tmp job file: {}", e)))?;
        file.write_all(&json_data)
            .map_err(|e| DomainError::Internal(format!("Failed to write tmp job file: {}", e)))?;
        file.sync_all()
            .map_err(|e| DomainError::Internal(format!("Failed to fsync tmp job file: {}", e)))?;
        drop(file);

        fs::rename(&tmp_path, &final_path).map_err(|e| {
            DomainError::Internal(format!("Failed to atomically rename job file: {}", e))
        })?;

        Ok(())
    }

    async fn load(&self, id: &JobId) -> Result<Option<Job>, DomainError> {
        let file_path = self.job_file(id);
        if !file_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&file_path)
            .map_err(|e| DomainError::Internal(format!("Failed to read job file: {}", e)))?;
        let job: Job = serde_json::from_str(&content)
            .map_err(|e| DomainError::Internal(format!("Failed to deserialize job file: {}", e)))?;

        Ok(Some(job))
    }

    async fn list(&self) -> Result<Vec<Job>, DomainError> {
        let mut jobs = Vec::new();
        if !self.base_dir.exists() {
            return Ok(jobs);
        }

        let entries = fs::read_dir(&self.base_dir)
            .map_err(|e| DomainError::Internal(format!("Failed to read jobs directory: {}", e)))?;

        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let job_file = entry.path().join("job.json");
                if job_file.exists() {
                    if let Ok(content) = fs::read_to_string(&job_file) {
                        if let Ok(job) = serde_json::from_str::<Job>(&content) {
                            jobs.push(job);
                        }
                    }
                }
            }
        }

        // Sort latest first
        jobs.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        Ok(jobs)
    }

    async fn delete(&self, id: &JobId) -> Result<(), DomainError> {
        let job_dir = self.base_dir.join(id.as_str());
        if job_dir.exists() {
            fs::remove_dir_all(&job_dir)
                .map_err(|e| DomainError::Internal(format!("Failed to remove job dir: {}", e)))?;
        }
        Ok(())
    }
}
