use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Default, Clone)]
pub struct TnmNet {
    pub last_target: Option<String>,
    pub last_status: Option<u16>,
    pub last_error: Option<String>,
}

impl TnmNet {
    fn clear_status(&mut self, target: &str) {
        self.last_target = Some(target.to_string());
        self.last_status = None;
        self.last_error = None;
    }

    fn set_error(&mut self, target: &str, err: &str) {
        self.last_target = Some(target.to_string());
        self.last_status = None;
        self.last_error = Some(err.to_string());
    }

    pub fn open_target(&mut self, target: &str) -> Result<()> {
        self.clear_status(target);
        #[cfg(target_os = "macos")]
        let mut cmd = {
            let mut c = Command::new("open");
            c.arg(target);
            c
        };
        #[cfg(target_os = "linux")]
        let mut cmd = {
            let mut c = Command::new("xdg-open");
            c.arg(target);
            c
        };
        #[cfg(target_os = "windows")]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.args(["/C", "start", "", target]);
            c
        };

        let status = cmd
            .status()
            .with_context(|| format!("open external target {target}"))?;
        if status.success() {
            Ok(())
        } else {
            let msg = format!("external opener exited with status {status}");
            self.set_error(target, &msg);
            Err(anyhow!(msg))
        }
    }

    pub fn open_file(&mut self, path: &Path) -> Result<()> {
        let target = path
            .to_str()
            .ok_or_else(|| anyhow!("non-utf8 file path"))?
            .to_string();
        self.open_target(&target)
    }

    pub fn open_url(&mut self, url: &str) -> Result<()> {
        self.open_target(url)
    }

    pub fn get_bytes(&mut self, url: &str) -> Result<Vec<u8>> {
        self.clear_status(url);
        let response = ureq::get(url)
            .call()
            .with_context(|| format!("GET {url}"))?;
        self.last_status = Some(response.status());
        let mut reader = response.into_reader();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut reader, &mut bytes)
            .with_context(|| format!("read response body from {url}"))?;
        Ok(bytes)
    }

    pub fn post_bytes(&mut self, url: &str, content_type: &str, body: &[u8]) -> Result<Vec<u8>> {
        self.clear_status(url);
        let response = ureq::post(url)
            .set("Content-Type", content_type)
            .send_bytes(body)
            .with_context(|| format!("POST {url}"))?;
        self.last_status = Some(response.status());
        let mut reader = response.into_reader();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut reader, &mut bytes)
            .with_context(|| format!("read response body from {url}"))?;
        Ok(bytes)
    }
}
