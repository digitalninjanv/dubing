use crate::domain::JobId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatchItemStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchItem {
    pub id: String,
    pub input_path: PathBuf,
    pub job_id: Option<JobId>,
    pub status: BatchItemStatus,
    pub error_message: Option<String>,
    pub output_audio: Option<PathBuf>,
    pub output_video: Option<PathBuf>,
    pub subtitle_path: Option<PathBuf>,
}

impl BatchItem {
    pub fn new(input_path: PathBuf) -> Self {
        let id = format!("item_{}", uuid::Uuid::new_v4().simple());
        Self {
            id,
            input_path,
            job_id: None,
            status: BatchItemStatus::Pending,
            error_message: None,
            output_audio: None,
            output_video: None,
            subtitle_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchJob {
    pub id: String,
    pub items: Vec<BatchItem>,
    pub source_language: String,
    pub target_language: String,
}

impl BatchJob {
    pub fn new(items: Vec<PathBuf>, source_language: String, target_language: String) -> Self {
        let id = format!("batch_{}", uuid::Uuid::new_v4().simple());
        let items = items.into_iter().map(BatchItem::new).collect();
        Self {
            id,
            items,
            source_language,
            target_language,
        }
    }

    pub fn completed_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.status == BatchItemStatus::Completed)
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.status == BatchItemStatus::Failed)
            .count()
    }

    pub fn total_count(&self) -> usize {
        self.items.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_job_creation_and_counts() {
        let mut batch = BatchJob::new(
            vec![PathBuf::from("a.mp3"), PathBuf::from("b.mp4")],
            "auto".to_string(),
            "en".to_string(),
        );

        assert_eq!(batch.total_count(), 2);
        assert_eq!(batch.completed_count(), 0);
        assert_eq!(batch.failed_count(), 0);

        batch.items[0].status = BatchItemStatus::Completed;
        assert_eq!(batch.completed_count(), 1);

        batch.items[1].status = BatchItemStatus::Failed;
        assert_eq!(batch.failed_count(), 1);
    }
}
