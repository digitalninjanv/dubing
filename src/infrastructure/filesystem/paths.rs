use std::path::PathBuf;

pub struct AppPaths;

impl AppPaths {
    pub fn app_data_dir() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from(".local/share"))
            .join("audiodub")
    }

    pub fn jobs_dir() -> PathBuf {
        Self::app_data_dir().join("jobs")
    }

    pub fn job_dir(job_id: &str) -> PathBuf {
        Self::jobs_dir().join(job_id)
    }

    pub fn outputs_dir() -> PathBuf {
        Self::app_data_dir().join("outputs")
    }

    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("audiodub")
    }

    pub fn config_file() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    pub fn ensure_dirs() -> std::io::Result<()> {
        std::fs::create_dir_all(Self::jobs_dir())?;
        std::fs::create_dir_all(Self::outputs_dir())?;
        std::fs::create_dir_all(Self::config_dir())?;
        Ok(())
    }
}
