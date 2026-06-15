//! Crash-log persistence rows.
#![allow(clippy::wildcard_imports)]

use super::*;

impl ModdeDb {
    /// Persist a raw local crash log and its structured correlation report.
    ///
    /// This stores logs locally only; callers must not upload or transmit the
    /// raw crash text.
    pub async fn record_crash_log(
        &self,
        profile_id: Option<i64>,
        report: &CrashCorrelationReport,
        raw_log: &str,
    ) -> Result<()> {
        let signature_json = serde_json::to_string(&report.signature).map_err(|e| {
            CoreError::Other(format!("failed to encode crash signature: {e}").into())
        })?;
        let report_json = serde_json::to_string(report)
            .map_err(|e| CoreError::Other(format!("failed to encode crash report: {e}").into()))?;
        self.db
            .execute(
                "INSERT INTO crash_logs
                    (game_id, profile_id, profile_name, source_path, logger_format,
                     raw_sha256, raw_log, signature_json, report_json)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                &vals![
                    report.game_id.clone(),
                    profile_id,
                    report.profile_name.clone(),
                    report.source_path.display().to_string(),
                    report.format.as_str(),
                    report.raw_sha256.clone(),
                    raw_log,
                    signature_json,
                    report_json,
                ],
            )
            .await?;
        Ok(())
    }
}
