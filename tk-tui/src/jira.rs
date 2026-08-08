//! Data source: shells out to jira-cli so auth/config stay in one place
//! (JIRA_API_TOKEN env + ~/.config/.jira/.config.yml). No HTTP code here.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::process::Command;

pub struct Comment {
    pub author: String,
    pub created: String,
    pub body: Value, // ADF
}

pub struct Ticket {
    pub key: String,
    pub summary: String,
    pub status: String,
    pub itype: String,
    pub description: Option<Value>, // ADF
    pub comments: Vec<Comment>,
}

fn s(v: &Value, path: &[&str]) -> String {
    let mut cur = v;
    for p in path {
        cur = &cur[p];
    }
    cur.as_str().unwrap_or_default().to_string()
}

/// "2026-08-01T11:41:20.339+0100" → "2026-08-01 11:41"
fn short_date(raw: &str) -> String {
    if raw.len() >= 16 {
        raw[..16].replace('T', " ")
    } else {
        raw.to_string()
    }
}

pub fn fetch(key: &str) -> Result<Ticket> {
    let out = Command::new("jira")
        .args(["issue", "view", key, "--raw"])
        .output()
        .context("failed to run `jira` — is jira-cli installed?")?;
    if !out.status.success() {
        bail!(
            "jira issue view {key} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let v: Value = serde_json::from_slice(&out.stdout)
        .context("jira --raw returned something that isn't JSON (auth problem? run: tk doctor)")?;
    let f = &v["fields"];

    let comments = f["comment"]["comments"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|c| Comment {
                    author: s(c, &["author", "displayName"]),
                    created: short_date(&s(c, &["created"])),
                    body: c["body"].clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Ticket {
        key: v["key"].as_str().unwrap_or(key).to_string(),
        summary: s(f, &["summary"]),
        status: s(f, &["status", "name"]),
        itype: s(f, &["issuetype", "name"]),
        description: match &f["description"] {
            Value::Null => None,
            d => Some(d.clone()),
        },
        comments,
    })
}

pub fn add_comment(key: &str, body: &str) -> Result<()> {
    let out = Command::new("jira")
        .args(["issue", "comment", "add", key, body, "--no-input"])
        .output()
        .context("failed to run `jira`")?;
    if !out.status.success() {
        bail!(
            "comment failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

pub fn open_in_browser(key: &str) {
    let _ = Command::new("jira")
        .args(["open", key])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
