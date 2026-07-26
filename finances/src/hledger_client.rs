//! Thin subprocess wrapper around the external `hledger` CLI.
//!
//! Mirrors `notes::nb_client`'s `run()` helper: shells out, maps a missing
//! binary to a dedicated "not installed" error, and surfaces stderr on
//! failure rather than a bare non-zero exit code.

use crate::finances_prelude::*;
use tokio::process::Command;

pub async fn run(journal_path: &str, args: &[&str]) -> FinancesLibResult<String> {
    let mut cmd = Command::new("hledger");
    cmd.args(["-f", journal_path]);
    cmd.args(args);
    let out = cmd.output().await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            FinancesLibError::HledgerNotInstalled
        } else {
            FinancesLibError::Io(e)
        }
    })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let msg = if !stderr.is_empty() { stderr } else { stdout };
        return Err(FinancesLibError::Hledger(msg));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}
