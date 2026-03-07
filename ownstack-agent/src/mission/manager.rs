//! Mission Manager — Persistent mission lifecycle with checkpointing.
//!
//! Saves missions as JSON files in `.ownstack/missions/`.
//! Uses atomic writes (temp + rename) to prevent corruption.
//! Supports pub/sub for UI event streaming.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use super::models::{
    Checkpoint, MissionEvent, MissionRecord, MissionSpec, MissionStatus,
};

// ─── Mission Manager ────────────────────────────────────────────

/// Manages mission persistence and lifecycle.
pub struct MissionManager {
    missions_dir: PathBuf,
    cache: HashMap<String, MissionRecord>,
    event_tx: broadcast::Sender<MissionEvent>,
}

impl MissionManager {
    pub fn new(workspace: &Path) -> Self {
        let missions_dir = workspace.join(".ownstack").join("missions");
        let _ = std::fs::create_dir_all(&missions_dir);

        let (event_tx, _) = broadcast::channel(64);

        let mut mgr = Self {
            missions_dir,
            cache: HashMap::new(),
            event_tx,
        };
        mgr.load_existing();
        mgr
    }

    /// Subscribe to mission events (for UI streaming).
    pub fn subscribe(&self) -> broadcast::Receiver<MissionEvent> {
        self.event_tx.subscribe()
    }

    /// Create a new mission.
    pub fn create_mission(
        &mut self,
        id: &str,
        title: &str,
        description: &str,
    ) -> &MissionRecord {
        let record = MissionRecord::new(id, title, description);
        Self::save_record_to_disk(&self.missions_dir, &record);
        info!("MissionManager: created mission '{id}'");
        let mission = match self.cache.entry(id.to_string()) {
            Entry::Occupied(mut occupied) => {
                occupied.insert(record);
                occupied.into_mut()
            }
            Entry::Vacant(vacant) => vacant.insert(record),
        };
        &*mission
    }

    /// Get a mission by ID.
    pub fn get_mission(&self, id: &str) -> Option<&MissionRecord> {
        self.cache.get(id)
    }

    /// Update mission status and log a message.
    pub fn update_status(&mut self, id: &str, status: MissionStatus, message: &str) {
        if let Some(mission) = self.cache.get_mut(id) {
            mission.set_status(status.clone());

            let event = MissionEvent::new(format!("[{status}] {message}"));
            mission.add_event(event.clone());
            let _ = self.event_tx.send(event);

            Self::save_record_to_disk(&self.missions_dir, mission);
        } else {
            warn!("MissionManager: mission '{id}' not found for status update");
        }
    }

    /// Add a log entry to a mission.
    pub fn add_log(&mut self, id: &str, message: &str) {
        if let Some(mission) = self.cache.get_mut(id) {
            let event = MissionEvent::new(message);
            mission.add_event(event.clone());
            let _ = self.event_tx.send(event);
            // Save periodically (every 10 events to reduce I/O)
            if mission.events.len() % 10 == 0 {
                Self::save_record_to_disk(&self.missions_dir, mission);
            }
        }
    }

    /// Set the mission spec (compiled from prompt).
    pub fn set_spec(&mut self, id: &str, spec: MissionSpec) {
        if let Some(mission) = self.cache.get_mut(id) {
            mission.spec = Some(spec);
            Self::save_record_to_disk(&self.missions_dir, mission);
        }
    }

    /// Create a checkpoint at the current mission state.
    pub fn create_checkpoint(
        &mut self,
        id: &str,
        description: &str,
        git_hash: Option<String>,
    ) -> Option<String> {
        let mission = self.cache.get_mut(id)?;

        let ckpt_id = format!(
            "ckpt-{}-{}",
            mission.checkpoints.len(),
            &mission.id[..mission.id.len().min(8)]
        );

        let checkpoint = Checkpoint {
            id: ckpt_id.clone(),
            step_index: mission.events.len(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
            description: description.to_string(),
            git_hash,
        };

        mission.checkpoints.push(checkpoint);
        Self::save_record_to_disk(&self.missions_dir, mission);

        info!("MissionManager: created checkpoint '{ckpt_id}' for mission '{id}'");
        Some(ckpt_id)
    }

    /// Get all checkpoints for a mission.
    pub fn list_checkpoints(&self, id: &str) -> Vec<&Checkpoint> {
        self.cache
            .get(id)
            .map(|m| m.checkpoints.iter().collect())
            .unwrap_or_default()
    }

    /// List all missions, sorted by most recent first.
    pub fn list_missions(&self) -> Vec<&MissionRecord> {
        let mut missions: Vec<&MissionRecord> = self.cache.values().collect();
        missions.sort_by(|a, b| {
            b.updated_at
                .partial_cmp(&a.updated_at)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        missions
    }

    /// Flush a mission to disk (for graceful shutdown).
    pub fn flush(&self, id: &str) {
        if let Some(mission) = self.cache.get(id) {
            Self::save_record_to_disk(&self.missions_dir, mission);
        }
    }

    /// Flush all missions to disk.
    pub fn flush_all(&self) {
        for mission in self.cache.values() {
            Self::save_record_to_disk(&self.missions_dir, mission);
        }
    }

    // ─── Persistence ─────────────────────────────────────────────

    fn mission_path(missions_dir: &Path, id: &str) -> PathBuf {
        missions_dir.join(format!("{id}.json"))
    }

    /// Static method to avoid borrow issues — operates on the dir path directly.
    fn save_record_to_disk(missions_dir: &Path, mission: &MissionRecord) {
        let path = Self::mission_path(missions_dir, &mission.id);
        let temp_path = path.with_extension("json.tmp");

        match serde_json::to_string_pretty(mission) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&temp_path, &json) {
                    warn!("MissionManager: failed to write temp file: {e}");
                    return;
                }
                if let Err(e) = std::fs::rename(&temp_path, &path) {
                    warn!("MissionManager: failed to rename temp to final: {e}");
                    let _ = std::fs::remove_file(&temp_path);
                }
            }
            Err(e) => warn!("MissionManager: serialization failed: {e}"),
        }
    }

    fn load_existing(&mut self) {
        let entries = match std::fs::read_dir(&self.missions_dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "json") {
                match std::fs::read_to_string(&path) {
                    Ok(content) => {
                        match serde_json::from_str::<MissionRecord>(&content) {
                            Ok(record) => {
                                debug!(
                                    "MissionManager: loaded mission '{}' from disk",
                                    record.id
                                );
                                self.cache.insert(record.id.clone(), record);
                            }
                            Err(e) => {
                                warn!(
                                    "MissionManager: corrupt mission file {:?}: {e}",
                                    path
                                );
                                let corrupted_dir =
                                    self.missions_dir.join(".corrupted");
                                let _ = std::fs::create_dir_all(&corrupted_dir);
                                let _ = std::fs::rename(
                                    &path,
                                    corrupted_dir
                                        .join(path.file_name().unwrap_or_default()),
                                );
                            }
                        }
                    }
                    Err(e) => warn!("MissionManager: can't read {:?}: {e}", path),
                }
            }
        }

        info!(
            "MissionManager: loaded {} mission(s) from disk",
            self.cache.len()
        );
    }
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_create_and_get_mission() {
        let dir = tempdir().unwrap();
        let mut mgr = MissionManager::new(dir.path());

        mgr.create_mission("m-001", "Fix auth", "Fix the broken login");
        let mission = mgr.get_mission("m-001");
        assert!(mission.is_some());
        assert_eq!(mission.unwrap().title, "Fix auth");
    }

    #[test]
    fn test_status_update_and_events() {
        let dir = tempdir().unwrap();
        let mut mgr = MissionManager::new(dir.path());

        mgr.create_mission("m-002", "Test", "desc");
        mgr.update_status("m-002", MissionStatus::Planning, "Generating plan");

        let mission = mgr.get_mission("m-002").unwrap();
        assert_eq!(mission.status, MissionStatus::Planning);
        assert!(!mission.events.is_empty());
    }

    #[test]
    fn test_persistence_across_instances() {
        let dir = tempdir().unwrap();

        {
            let mut mgr = MissionManager::new(dir.path());
            mgr.create_mission("persist-001", "Persist Test", "desc");
            mgr.update_status("persist-001", MissionStatus::Running, "exec");
        }

        {
            let mgr = MissionManager::new(dir.path());
            let mission = mgr.get_mission("persist-001");
            assert!(mission.is_some());
            assert_eq!(mission.unwrap().status, MissionStatus::Running);
        }
    }

    #[test]
    fn test_checkpointing() {
        let dir = tempdir().unwrap();
        let mut mgr = MissionManager::new(dir.path());

        mgr.create_mission("ckpt-001", "Checkpoint Test", "desc");
        let ckpt_id = mgr.create_checkpoint(
            "ckpt-001",
            "After step 1",
            Some("abc123".to_string()),
        );

        assert!(ckpt_id.is_some());
        let checkpoints = mgr.list_checkpoints("ckpt-001");
        assert_eq!(checkpoints.len(), 1);
        assert_eq!(checkpoints[0].git_hash, Some("abc123".to_string()));
    }

    #[test]
    fn test_list_missions_ordering() {
        let dir = tempdir().unwrap();
        let mut mgr = MissionManager::new(dir.path());

        mgr.create_mission("a", "First", "desc");
        mgr.create_mission("b", "Second", "desc");
        mgr.update_status("a", MissionStatus::Running, "updated later");

        let list = mgr.list_missions();
        assert_eq!(list[0].id, "a");
    }

    #[test]
    fn test_corrupted_file_handling() {
        let dir = tempdir().unwrap();
        let missions_dir = dir.path().join(".ownstack").join("missions");
        std::fs::create_dir_all(&missions_dir).unwrap();

        std::fs::write(missions_dir.join("bad.json"), "not valid json!!!").unwrap();

        let mgr = MissionManager::new(dir.path());
        assert!(mgr.get_mission("bad").is_none());
        assert!(missions_dir.join(".corrupted").join("bad.json").exists());
    }
}
