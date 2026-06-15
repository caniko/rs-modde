//! Tool settings and applied-file state.
#![allow(clippy::wildcard_imports)]

use super::rows::*;
use super::*;

impl ModdeDb {
    pub async fn save_tool_config(
        &self,
        game_id: &GameId,
        tool_id: &str,
        enabled: bool,
        settings_json: &str,
    ) -> Result<()> {
        self.save_tool_config_with_reason(game_id, tool_id, enabled, settings_json, "update")
            .await
    }

    /// Save a tool configuration and append a history node.
    pub async fn save_tool_config_with_reason(
        &self,
        game_id: &GameId,
        tool_id: &str,
        enabled: bool,
        settings_json: &str,
        reason: &str,
    ) -> Result<()> {
        let parent_node_id = self.current_tool_setting_node_id(game_id, tool_id).await?;
        let node_id = new_tool_setting_node_id(game_id, tool_id);
        let reason = if reason.trim().is_empty() {
            "update"
        } else {
            reason.trim()
        };

        self.db
            .execute(
                "INSERT INTO tool_setting_nodes (node_id, game_id, tool_id, enabled, settings, reason)
                 VALUES (?, ?, ?, ?, ?, ?)",
                &vals![node_id.clone(), game_id, tool_id, enabled, settings_json, reason],
            )
            .await?;
        if let Some(parent_node_id) = parent_node_id {
            self.db
                .execute(
                    "INSERT INTO tool_setting_edges (parent_node_id, child_node_id)
                     VALUES (?, ?)
                     ON CONFLICT(parent_node_id, child_node_id) DO NOTHING",
                    &vals![parent_node_id, node_id.clone()],
                )
                .await?;
        }
        self.db
            .execute(
                "INSERT INTO game_tools (game_id, tool_id, enabled, settings, updated_at, current_node_id)
                 VALUES (?, ?, ?, ?, {NOW}, ?)
                 ON CONFLICT(game_id, tool_id) DO UPDATE SET
                     enabled = excluded.enabled,
                     settings = excluded.settings,
                     updated_at = excluded.updated_at,
                     current_node_id = excluded.current_node_id",
                &vals![game_id, tool_id, enabled, settings_json, node_id],
            )
            .await?;
        Ok(())
    }

    /// Load recent settings history nodes for a tool.
    pub async fn list_tool_setting_history(
        &self,
        game_id: &GameId,
        tool_id: &str,
        limit: usize,
    ) -> Result<Vec<ToolSettingHistoryNode>> {
        let current_node_id = self.current_tool_setting_node_id(game_id, tool_id).await?;
        let current = current_node_id.clone();
        self.db
            .fetch_all(
                "SELECT node_id, game_id, tool_id, enabled, settings, reason, created_at
                 FROM tool_setting_nodes
                 WHERE game_id = ? AND tool_id = ?
                 ORDER BY id DESC
                 LIMIT ?",
                &vals![game_id, tool_id, limit as i64],
                move |r| {
                    let node_id = r.string(0)?;
                    Ok(ToolSettingHistoryNode {
                        is_current: current.as_deref() == Some(node_id.as_str()),
                        node_id,
                        game_id: r.string(1)?,
                        tool_id: r.string(2)?,
                        enabled: r.bool(3)?,
                        settings_json: r.string(4)?,
                        reason: r.string(5)?,
                        created_at: r.string(6)?,
                    })
                },
            )
            .await
    }

    /// Load DAG edges for a tool's recorded settings history.
    pub async fn list_tool_setting_edges(
        &self,
        game_id: &GameId,
        tool_id: &str,
    ) -> Result<Vec<ToolSettingHistoryEdge>> {
        self.db
            .fetch_all(
                "SELECT e.parent_node_id, e.child_node_id
                 FROM tool_setting_edges e
                 JOIN tool_setting_nodes child ON child.node_id = e.child_node_id
                 WHERE child.game_id = ? AND child.tool_id = ?
                 ORDER BY e.id",
                &vals![game_id, tool_id],
                |r| {
                    Ok(ToolSettingHistoryEdge {
                        parent_node_id: r.string(0)?,
                        child_node_id: r.string(1)?,
                    })
                },
            )
            .await
    }

    /// Restore a settings node by appending a new child node with copied state.
    pub async fn restore_tool_setting_node(
        &self,
        game_id: &GameId,
        tool_id: &str,
        node_id: &str,
    ) -> Result<()> {
        let (enabled, settings_json) = self
            .db
            .fetch_one(
                "SELECT enabled, settings FROM tool_setting_nodes
                 WHERE game_id = ? AND tool_id = ? AND node_id = ?",
                &vals![game_id, tool_id, node_id],
                |r| Ok((r.bool(0)?, r.string(1)?)),
            )
            .await?;
        let reason = format!("restore:{node_id}");
        self.save_tool_config_with_reason(game_id, tool_id, enabled, &settings_json, &reason)
            .await
    }

    async fn current_tool_setting_node_id(
        &self,
        game_id: &GameId,
        tool_id: &str,
    ) -> Result<Option<String>> {
        Ok(self
            .db
            .fetch_optional(
                "SELECT current_node_id FROM game_tools WHERE game_id = ? AND tool_id = ?",
                &vals![game_id, tool_id],
                |r| r.opt_string(0),
            )
            .await?
            .flatten())
    }

    /// Load all tool configurations for a game.
    pub async fn load_tool_configs(&self, game_id: &GameId) -> Result<Vec<ToolConfigRow>> {
        self.db
            .fetch_all(
                "SELECT tool_id, enabled, settings FROM game_tools WHERE game_id = ?",
                &vals![game_id],
                |r| {
                    Ok(ToolConfigRow {
                        tool_id: r.string(0)?,
                        enabled: r.bool(1)?,
                        settings_json: r.string(2)?,
                    })
                },
            )
            .await
    }

    /// Load a single tool configuration for a game.
    pub async fn load_tool_config(
        &self,
        game_id: &GameId,
        tool_id: &str,
    ) -> Result<Option<ToolConfigRow>> {
        self.db
            .fetch_optional(
                "SELECT tool_id, enabled, settings FROM game_tools
                 WHERE game_id = ? AND tool_id = ?",
                &vals![game_id, tool_id],
                |r| {
                    Ok(ToolConfigRow {
                        tool_id: r.string(0)?,
                        enabled: r.bool(1)?,
                        settings_json: r.string(2)?,
                    })
                },
            )
            .await
    }

    /// Record files applied by a tool to a game directory.
    pub async fn save_applied_files(
        &self,
        game_id: &GameId,
        tool_id: &str,
        rel_paths: &[String],
    ) -> Result<()> {
        for path in rel_paths {
            self.db
                .execute(
                    "INSERT INTO tool_applied_files (game_id, tool_id, rel_path)
                     VALUES (?, ?, ?)
                     ON CONFLICT(game_id, tool_id, rel_path) DO NOTHING",
                    &vals![game_id, tool_id, path.clone()],
                )
                .await?;
        }
        Ok(())
    }

    /// Load files previously applied by a tool.
    pub async fn load_applied_files(&self, game_id: &GameId, tool_id: &str) -> Result<Vec<String>> {
        self.db
            .fetch_all(
                "SELECT rel_path FROM tool_applied_files
                 WHERE game_id = ? AND tool_id = ?",
                &vals![game_id, tool_id],
                |r| r.string(0),
            )
            .await
    }

    /// Load every file recorded as applied by any managed tool for this game.
    pub async fn load_all_applied_files(&self, game_id: &GameId) -> Result<Vec<String>> {
        self.db
            .fetch_all(
                "SELECT rel_path FROM tool_applied_files
                 WHERE game_id = ?
                 ORDER BY tool_id, rel_path",
                &vals![game_id],
                |r| r.string(0),
            )
            .await
    }

    /// Load every file recorded as applied by any managed tool for this game,
    /// preserving the owning tool id for provenance exports.
    pub async fn load_all_applied_file_rows(
        &self,
        game_id: &GameId,
    ) -> Result<Vec<ToolAppliedFileRow>> {
        self.db
            .fetch_all(
                "SELECT tool_id, rel_path FROM tool_applied_files
                 WHERE game_id = ?
                 ORDER BY tool_id, rel_path",
                &vals![game_id],
                |r| {
                    Ok(ToolAppliedFileRow {
                        tool_id: r.string(0)?,
                        rel_path: r.string(1)?,
                    })
                },
            )
            .await
    }

    /// Clear all applied file records for a tool on a game.
    pub async fn clear_applied_files(&self, game_id: &GameId, tool_id: &str) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM tool_applied_files WHERE game_id = ? AND tool_id = ?",
                &vals![game_id, tool_id],
            )
            .await?;
        Ok(())
    }
}
