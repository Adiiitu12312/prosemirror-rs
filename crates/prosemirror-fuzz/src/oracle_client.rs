use std::io::Write;
use std::process::{Command, Stdio};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Debug)]
pub struct OracleRequest {
    pub doc: Value,
    pub step: Value,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OracleResponse {
    pub success: bool,
    pub doc: Option<Value>,
    pub step_map: Option<Vec<usize>>,
    pub error: Option<String>,
}

pub struct OracleClient {
    oracle_script_path: String,
}

impl OracleClient {
    pub fn new(oracle_script_path: impl Into<String>) -> Self {
        Self {
            oracle_script_path: oracle_script_path.into(),
        }
    }

    pub fn apply_step(&self, doc: &Value, step: &Value) -> Result<OracleResponse, String> {
        let req = OracleRequest {
            doc: doc.clone(),
            step: step.clone(),
        };
        let input_json = serde_json::to_string(&req).map_err(|e| e.to_string())?;

        let mut child = Command::new("node")
            .arg(&self.oracle_script_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn node oracle: {e}"))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(input_json.as_bytes())
                .map_err(|e| e.to_string())?;
        }

        let output = child
            .wait_with_output()
            .map_err(|e| format!("Failed to wait for node oracle: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Node oracle exited with error: {stderr}"));
        }

        let resp: OracleResponse = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("Failed to parse oracle response: {e}"))?;

        Ok(resp)
    }
}
