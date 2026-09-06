use crate::domain::DomainError;

pub trait SecretStore: Send + Sync {
    fn get_api_key(&self) -> Result<Option<String>, DomainError>;
    fn set_api_key(&self, key: &str) -> Result<(), DomainError>;
    fn delete_api_key(&self) -> Result<(), DomainError>;
}
