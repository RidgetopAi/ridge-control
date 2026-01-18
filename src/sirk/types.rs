use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeConfig {
    pub run_name: String,
    pub total_instances: u32,
    pub project: String,
    pub seed_path: String,
    #[serde(default = "default_spindles_proxy_url")]
    pub spindles_proxy_url: String,
    #[serde(default = "default_mandrel_url")]
    pub mandrel_url: String,
    #[serde(default = "default_timeout_minutes")]
    pub timeout_minutes: u32,
}

fn default_spindles_proxy_url() -> String {
    "http://localhost:8082".to_string()
}

fn default_mandrel_url() -> String {
    "http://localhost:8080".to_string()
}

fn default_timeout_minutes() -> u32 {
    30
}

impl Default for ForgeConfig {
    fn default() -> Self {
        Self {
            run_name: String::new(),
            total_instances: 1,
            project: String::new(),
            seed_path: String::new(),
            spindles_proxy_url: default_spindles_proxy_url(),
            mandrel_url: default_mandrel_url(),
            timeout_minutes: default_timeout_minutes(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ForgeEvent {
    RunStarted(RunStartedEvent),
    InstanceStarted(InstanceStartedEvent),
    InstanceCompleted(InstanceCompletedEvent),
    InstanceFailed(InstanceFailedEvent),
    RunCompleted(RunCompletedEvent),
    Error(ErrorEvent),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStartedEvent {
    pub run_name: String,
    pub total_instances: u32,
    pub timestamp: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceStartedEvent {
    pub run_name: String,
    pub instance_number: u32,
    pub total_instances: u32,
    pub timestamp: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceCompletedEvent {
    pub run_name: String,
    pub instance_number: u32,
    pub success: bool,
    pub duration_ms: u64,
    pub timestamp: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceFailedEvent {
    pub run_name: String,
    pub instance_number: u32,
    pub error: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCompletedEvent {
    pub run_name: String,
    pub success_count: u32,
    pub fail_count: u32,
    pub timestamp: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ErrorEvent {
    pub message: String,
    pub fatal: bool,
    pub timestamp: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_run_started() {
        let json = r#"{"type":"run_started","runName":"test-run","totalInstances":5,"timestamp":"2026-01-17T12:00:00Z"}"#;
        let event: ForgeEvent = serde_json::from_str(json).unwrap();
        match event {
            ForgeEvent::RunStarted(e) => {
                assert_eq!(e.run_name, "test-run");
                assert_eq!(e.total_instances, 5);
            }
            _ => panic!("Expected RunStarted"),
        }
    }

    #[test]
    fn test_parse_instance_completed() {
        let json = r#"{"type":"instance_completed","runName":"test-run","instanceNumber":3,"success":true,"durationMs":12345,"timestamp":"2026-01-17T12:05:00Z"}"#;
        let event: ForgeEvent = serde_json::from_str(json).unwrap();
        match event {
            ForgeEvent::InstanceCompleted(e) => {
                assert_eq!(e.instance_number, 3);
                assert!(e.success);
                assert_eq!(e.duration_ms, 12345);
            }
            _ => panic!("Expected InstanceCompleted"),
        }
    }

    #[test]
    fn test_parse_error_event() {
        let json = r#"{"type":"error","message":"Mandrel unavailable","fatal":true,"timestamp":"2026-01-17T12:00:00Z"}"#;
        let event: ForgeEvent = serde_json::from_str(json).unwrap();
        match event {
            ForgeEvent::Error(e) => {
                assert_eq!(e.message, "Mandrel unavailable");
                assert!(e.fatal);
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_forge_config_serialization() {
        let config = ForgeConfig {
            run_name: "my-run".to_string(),
            total_instances: 10,
            project: "forge".to_string(),
            seed_path: "/path/to/seed.md".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("runName"));
        assert!(json.contains("totalInstances"));
    }
}
