use crate::sandbox::{Sandbox, SandboxLevel};
use crate::tool_result::ToolResult;
use async_trait::async_trait;
use bollard::container::{
    Config, CreateContainerOptions, LogOutput, RemoveContainerOptions,
    StartContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecOptions};
use bollard::Docker;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::path::Path;
use tracing::debug;

pub struct DockerSandbox {
    image: String,
}

impl DockerSandbox {
    pub fn new(image: Option<String>) -> Self {
        Self {
            image: image.unwrap_or_else(|| "alpine:latest".to_string()),
        }
    }

    async fn get_docker() -> Result<Docker, String> {
        #[cfg(windows)]
        {
            Docker::connect_with_named_pipe_defaults()
                .map_err(|e| format!("Failed to connect to Docker pipe: {}", e))
        }
        #[cfg(unix)]
        {
            Docker::connect_with_unix_defaults()
                .map_err(|e| format!("Failed to connect to Docker socket: {}", e))
        }
    }
}

#[async_trait]
impl Sandbox for DockerSandbox {
    async fn exec(
        &self,
        command_str: &str,
        cwd: &Path,
        level: SandboxLevel,
    ) -> ToolResult {
        let docker = match Self::get_docker().await {
            Ok(d) => d,
            Err(e) => return ToolResult::failure(e, None),
        };

        let container_name = format!("ownstack-sandbox-{}", uuid::Uuid::new_v4());

        // 1. Create container
        let config = Config {
            image: Some(self.image.clone()),
            cmd: Some(vec![
                "sh".to_string(),
                "-c".to_string(),
                "tail -f /dev/null".to_string(),
            ]), // Keep it alive
            network_disabled: Some(true),
            host_config: Some(bollard::service::HostConfig {
                auto_remove: Some(true),
                memory: Some(match level {
                    SandboxLevel::Light => 512 * 1024 * 1024,
                    SandboxLevel::Standard => 1024 * 1024 * 1024,
                    SandboxLevel::Strict => 2048 * 1024 * 1024,
                }),
                cpu_quota: Some(match level {
                    SandboxLevel::Light => 50000,
                    SandboxLevel::Standard => 100000,
                    SandboxLevel::Strict => 200000,
                }),
                cpu_period: Some(100000),
                binds: Some(vec![format!(
                    "{}:/workspace:rw",
                    cwd.to_string_lossy()
                )]),
                ..Default::default()
            }),
            working_dir: Some("/workspace".to_string()),
            ..Default::default()
        };

        if let Err(e) = docker
            .create_container(
                Some(CreateContainerOptions {
                    name: container_name.clone(),
                    ..Default::default()
                }),
                config,
            )
            .await
        {
            return ToolResult::failure(
                format!("Container creation failed: {}", e),
                None,
            );
        }

        // 2. Start container
        if let Err(e) = docker
            .start_container(&container_name, None::<StartContainerOptions<String>>)
            .await
        {
            let _ = docker
                .remove_container(&container_name, None::<RemoveContainerOptions>)
                .await;
            return ToolResult::failure(
                format!("Container start failed: {}", e),
                None,
            );
        }

        // 3. Exec command
        let exec_config = CreateExecOptions {
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            cmd: Some(vec![
                "sh".to_string(),
                "-c".to_string(),
                command_str.to_string(),
            ]),
            ..Default::default()
        };

        let exec_id = match docker.create_exec(&container_name, exec_config).await {
            Ok(e) => e.id,
            Err(e) => {
                let _ = docker
                    .remove_container(
                        &container_name,
                        Some(RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    )
                    .await;
                return ToolResult::failure(
                    format!("Exec creation failed: {}", e),
                    None,
                );
            }
        };

        let mut stdout = String::new();
        let mut stderr = String::new();

        use bollard::exec::StartExecResults;
        match docker.start_exec(&exec_id, None::<StartExecOptions>).await {
            Ok(StartExecResults::Attached { mut output, .. }) => {
                while let Some(msg) = output.next().await {
                    match msg {
                        Ok(LogOutput::StdOut { message }) => {
                            stdout.push_str(&String::from_utf8_lossy(&message));
                        }
                        Ok(LogOutput::StdErr { message }) => {
                            stderr.push_str(&String::from_utf8_lossy(&message));
                        }
                        _ => {}
                    }
                }
            }
            Ok(StartExecResults::Detached) => {
                debug!("Exec was detached, no output captured.");
            }
            Err(e) => {
                let _ = docker
                    .remove_container(
                        &container_name,
                        Some(RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    )
                    .await;
                return ToolResult::failure(
                    format!("Exec start failed: {}", e),
                    None,
                );
            }
        }

        // 4. Get exit code
        let inspect = match docker.inspect_exec(&exec_id).await {
            Ok(i) => i,
            Err(_) => {
                let _ = docker
                    .remove_container(
                        &container_name,
                        Some(RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    )
                    .await;
                return ToolResult::failure(
                    "Failed to inspect exec result".to_string(),
                    None,
                );
            }
        };

        let exit_code = inspect.exit_code.map(|c| c as i32);
        let success = exit_code == Some(0);

        // 5. Cleanup
        let _ = docker
            .remove_container(
                &container_name,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;

        ToolResult {
            success,
            stdout,
            stderr,
            exit_code,
            metadata: HashMap::new(),
        }
    }
}
