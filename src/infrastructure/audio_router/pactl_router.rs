use crate::application::ports::AudioRouter;
use crate::domain::{AudioAppInfo, DomainError};
use async_trait::async_trait;
use std::process::Command;
use tracing::{debug, info, warn};

pub struct PactlAudioRouter;

impl PactlAudioRouter {
    pub fn new() -> Self {
        Self
    }
    pub fn is_browser_app(app: &AudioAppInfo) -> bool {
        app.is_browser()
    }
}

impl Default for PactlAudioRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AudioRouter for PactlAudioRouter {
    async fn is_available(&self) -> bool {
        match Command::new("pactl").arg("info").output() {
            Ok(out) => out.status.success(),
            Err(_) => false,
        }
    }

    async fn create_null_sink(&self, sink_name: &str) -> Result<u32, DomainError> {
        let desc_arg = format!(
            "sink_properties=device.description=\"AudioDub_{}\"",
            sink_name
        );
        let output = Command::new("pactl")
            .args([
                "load-module",
                "module-null-sink",
                &format!("sink_name={}", sink_name),
                &desc_arg,
            ])
            .output()
            .map_err(|e| {
                DomainError::Internal(format!("Failed to execute pactl load-module: {}", e))
            })?;

        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            return Err(DomainError::Internal(format!(
                "pactl load-module failed: {}",
                err_msg.trim()
            )));
        }

        let id_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let module_id: u32 = id_str.parse().map_err(|e| {
            DomainError::Internal(format!(
                "Failed to parse module ID from pactl output '{}': {}",
                id_str, e
            ))
        })?;

        info!(
            "Created virtual null sink '{}' (module ID: {})",
            sink_name, module_id
        );
        Ok(module_id)
    }

    async fn unload_null_sink(&self, module_id: u32) -> Result<(), DomainError> {
        let output = Command::new("pactl")
            .args(["unload-module", &module_id.to_string()])
            .output()
            .map_err(|e| {
                DomainError::Internal(format!("Failed to execute pactl unload-module: {}", e))
            })?;

        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            warn!(
                "pactl unload-module {} warning: {}",
                module_id,
                err_msg.trim()
            );
        } else {
            info!("Unloaded virtual null sink module {}", module_id);
        }

        Ok(())
    }

    async fn list_sink_inputs(&self) -> Result<Vec<AudioAppInfo>, DomainError> {
        let output = Command::new("pactl")
            .args(["list", "sink-inputs"])
            .output()
            .map_err(|e| {
                DomainError::Internal(format!("Failed to execute pactl list sink-inputs: {}", e))
            })?;

        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            return Err(DomainError::Internal(format!(
                "pactl list sink-inputs failed: {}",
                err_msg.trim()
            )));
        }

        let raw_text = String::from_utf8_lossy(&output.stdout);
        let mut apps = Vec::new();

        let mut cur_id: Option<u32> = None;
        let mut cur_app_name: Option<String> = None;
        let mut cur_binary_name: Option<String> = None;
        let mut cur_media_name: Option<String> = None;

        for line in raw_text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("Sink Input #") {
                if let Some(id) = cur_id {
                    apps.push(AudioAppInfo {
                        sink_input_id: id,
                        application_name: cur_app_name.unwrap_or_else(|| format!("App #{}", id)),
                        binary_name: cur_binary_name.unwrap_or_else(|| "unknown".to_string()),
                        media_name: cur_media_name,
                    });
                }
                cur_id = trimmed
                    .strip_prefix("Sink Input #")
                    .and_then(|s| s.trim().parse::<u32>().ok());
                cur_app_name = None;
                cur_binary_name = None;
                cur_media_name = None;
            } else if trimmed.starts_with("application.name = ") {
                cur_app_name = trimmed
                    .strip_prefix("application.name = ")
                    .map(|s| s.trim_matches('"').to_string());
            } else if trimmed.starts_with("application.process.binary = ") {
                cur_binary_name = trimmed
                    .strip_prefix("application.process.binary = ")
                    .map(|s| s.trim_matches('"').to_string());
            } else if trimmed.starts_with("media.name = ") {
                cur_media_name = trimmed
                    .strip_prefix("media.name = ")
                    .map(|s| s.trim_matches('"').to_string());
            }
        }

        if let Some(id) = cur_id {
            apps.push(AudioAppInfo {
                sink_input_id: id,
                application_name: cur_app_name.unwrap_or_else(|| format!("App #{}", id)),
                binary_name: cur_binary_name.unwrap_or_else(|| "unknown".to_string()),
                media_name: cur_media_name,
            });
        }

        debug!("Discovered {} active sink inputs via pactl", apps.len());
        Ok(apps)
    }

    async fn move_sink_input(
        &self,
        sink_input_id: u32,
        sink_name: &str,
    ) -> Result<(), DomainError> {
        info!(
            "Moving sink-input {} to sink '{}'",
            sink_input_id, sink_name
        );
        let output = Command::new("pactl")
            .args(["move-sink-input", &sink_input_id.to_string(), sink_name])
            .output()
            .map_err(|e| {
                DomainError::Internal(format!("Failed to execute pactl move-sink-input: {}", e))
            })?;

        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            return Err(DomainError::Internal(format!(
                "pactl move-sink-input failed: {}",
                err_msg.trim()
            )));
        }

        Ok(())
    }

    async fn restore_sink_input(&self, sink_input_id: u32) -> Result<(), DomainError> {
        info!("Restoring sink-input {} to @DEFAULT_SINK@", sink_input_id);
        let output = Command::new("pactl")
            .args([
                "move-sink-input",
                &sink_input_id.to_string(),
                "@DEFAULT_SINK@",
            ])
            .output()
            .map_err(|e| DomainError::Internal(format!("Failed to restore sink-input: {}", e)))?;

        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            warn!(
                "Failed to restore sink-input {}: {}",
                sink_input_id,
                err_msg.trim()
            );
        }

        Ok(())
    }

    async fn get_default_sink_name(&self) -> Result<String, DomainError> {
        let output = Command::new("pactl")
            .arg("get-default-sink")
            .output()
            .map_err(|e| DomainError::Internal(format!("Failed to get default sink: {}", e)))?;

        if output.status.success() {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !name.is_empty() {
                return Ok(name);
            }
        }

        Ok("default".to_string())
    }
}
