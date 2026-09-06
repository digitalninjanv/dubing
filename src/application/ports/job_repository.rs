use crate::domain::{DomainError, Job, JobId};
use async_trait::async_trait;

#[async_trait]
pub trait JobRepository: Send + Sync {
    async fn save(&self, job: &Job) -> Result<(), DomainError>;
    async fn load(&self, id: &JobId) -> Result<Option<Job>, DomainError>;
    async fn list(&self) -> Result<Vec<Job>, DomainError>;
    async fn delete(&self, id: &JobId) -> Result<(), DomainError>;
}
