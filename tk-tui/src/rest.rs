//! The bits of Jira that jira-cli can't reach.
//!
//! jira-cli covers reading an issue and adding a comment, which is all the
//! ticket pane ever needed. The checklist needs two more things it doesn't
//! expose: a search that returns descriptions, and a description *write*. So
//! this module talks to the REST API directly — still with no HTTP crate, by
//! shelling out to curl the way `tk doctor` already does.
//!
//! Auth and server config are read from jira-cli's own files, so there is
//! still exactly one place to configure Jira.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

pub struct Config {
    pub server: String,
    pub login: String,
    pub token: String,
    /// jira-cli's default project. Scoping to it is what keeps Atlassian's
    /// "(Example) …" sample project out of your list.
    pub project: Option<String>,
}

fn config_path() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
        .join(".config/.jira/.config.yml")
}

/// Pull `server`, `login` and `project.key` out of jira-cli's config.
///
/// Hand-rolled rather than pulling in a YAML crate: we want three scalars from
/// a file we don't own, and the shape we care about is flat top-level keys plus
/// one nested `key:` under `project:`. `tk doctor` reads the same file the same
/// way with awk.
pub fn parse_config(yaml: &str) -> (Option<String>, Option<String>, Option<String>) {
    let (mut server, mut login, mut project) = (None, None, None);
    let mut in_project = false;
    for line in yaml.lines() {
        let indented = line.starts_with([' ', '\t']);
        let trimmed = line.trim();
        if !indented {
            // any new top-level key ends the project block
            in_project = trimmed.starts_with("project:");
        }
        let Some((k, v)) = trimmed.split_once(':') else {
            continue;
        };
        let v = v.trim().trim_matches('"').trim_matches('\'');
        match (k.trim(), indented) {
            ("server", false) if !v.is_empty() => server = Some(v.to_string()),
            ("login", false) if !v.is_empty() => login = Some(v.to_string()),
            ("key", true) if in_project && !v.is_empty() => project = Some(v.to_string()),
            _ => {}
        }
    }
    (server, login, project)
}

pub fn config() -> Result<Config> {
    let path = config_path();
    let raw = std::fs::read_to_string(&path)
        .context(format!("reading {} — run: jira init", path.display()))?;
    let (server, login, project) = parse_config(&raw);
    let server = server.context("no `server` in jira config — run: jira init")?;
    let login = login.context("no `login` in jira config — run: jira init")?;
    let token = std::env::var("JIRA_API_TOKEN")
        .ok()
        .filter(|t| !t.trim().is_empty())
        .context("JIRA_API_TOKEN not set — launch via `tk todo` so it's sourced")?;
    Ok(Config {
        server: server.trim_end_matches('/').to_string(),
        login,
        token,
        project,
    })
}

pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Run curl and return the parsed body, or a useful error.
///
/// Credentials go in on stdin as a curl config file rather than in argv, so
/// the token never shows up in `ps`.
fn curl(cfg: &Config, method: &str, url: &str, body: Option<&str>) -> Result<Option<Value>> {
    let mut cmd = Command::new("curl");
    cmd.args(["-sS", "--config", "-"])
        .args(["-X", method])
        .args(["-H", "Accept: application/json"])
        .args(["-w", "\n%{http_code}"])
        .arg(url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let tmp = body
        .map(|b| -> Result<_> {
            let path = std::env::temp_dir().join(format!(
                "tk-tui-{}-{}.json",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            std::fs::write(&path, b).context("staging the request body")?;
            Ok(path)
        })
        .transpose()?;
    if let Some(path) = &tmp {
        cmd.args(["-H", "Content-Type: application/json"]);
        cmd.arg("--data-binary").arg(format!("@{}", path.display()));
    }

    let mut child = cmd
        .spawn()
        .context("failed to run `curl` — is it installed? (try: tk doctor)")?;
    {
        let mut stdin = child.stdin.take().expect("piped");
        // curl config-file quoting: backslash and double-quote are escaped.
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        writeln!(stdin, "user = \"{}:{}\"", esc(&cfg.login), esc(&cfg.token))?;
    }
    let out = child.wait_with_output().context("waiting for curl")?;
    if let Some(path) = &tmp {
        std::fs::remove_file(path).ok();
    }
    if !out.status.success() {
        bail!("curl failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let (payload, code) = text.rsplit_once('\n').unwrap_or(("", text.trim()));
    match code.trim() {
        "200" | "201" => {}
        "204" => return Ok(None),
        "401" | "403" => bail!("jira auth rejected (HTTP {code}) — run: tk doctor"),
        "404" => bail!("not found (HTTP 404)"),
        other => {
            // Jira puts something readable in errorMessages; surface it.
            let msg = serde_json::from_str::<Value>(payload)
                .ok()
                .and_then(|v| {
                    v["errorMessages"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join("; ")
                        })
                        .filter(|s| !s.is_empty())
                })
                .unwrap_or_else(|| payload.chars().take(200).collect());
            bail!("jira returned HTTP {other}: {msg}");
        }
    }
    if payload.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(
        serde_json::from_str(payload).context("jira returned something that isn't JSON")?,
    ))
}

pub fn get(cfg: &Config, path: &str) -> Result<Value> {
    let url = format!("{}{}", cfg.server, path);
    curl(cfg, "GET", &url, None)?.context("jira returned an empty body")
}

pub fn put(cfg: &Config, path: &str, body: &Value) -> Result<()> {
    let url = format!("{}{}", cfg.server, path);
    curl(cfg, "PUT", &url, Some(&body.to_string()))?;
    Ok(())
}

/// The JQL behind the checklist. Scoped to jira-cli's default project when it
/// has one, so a sandbox full of Atlassian's sample issues stays out of the
/// list. `TK_TODO_JQL` overrides the whole thing.
pub fn todo_jql(cfg: &Config) -> String {
    if let Ok(q) = std::env::var("TK_TODO_JQL") {
        if !q.trim().is_empty() {
            return q;
        }
    }
    let mut q = String::from("assignee = currentUser() AND statusCategory != Done");
    if let Some(p) = &cfg.project {
        q.push_str(&format!(" AND project = {p}"));
    }
    q.push_str(" ORDER BY updated DESC");
    q
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real file, shape-wise: flat scalars plus a nested project block,
    /// with other nested `key:` lines (custom fields) that must not be mistaken
    /// for the project key.
    const SAMPLE: &str = "auth_type: basic\nboard: \"\"\nissue:\n    fields:\n        custom:\n            - name: Rank\n              key: customfield_10019\ninstallation: Cloud\nlogin: someone@example.com\nproject:\n    key: JROZ\n    type: next-gen\nserver: https://example.atlassian.net\n";

    #[test]
    fn reads_server_login_and_project_key_only() {
        let (server, login, project) = parse_config(SAMPLE);
        assert_eq!(server.as_deref(), Some("https://example.atlassian.net"));
        assert_eq!(login.as_deref(), Some("someone@example.com"));
        assert_eq!(
            project.as_deref(),
            Some("JROZ"),
            "customfield key lines must not win"
        );
    }

    #[test]
    fn tolerates_a_config_without_a_project() {
        let (_, _, project) = parse_config("login: a@b.c\nserver: https://x\n");
        assert_eq!(project, None);
    }

    #[test]
    fn scopes_the_query_to_the_configured_project() {
        let cfg = |p: Option<&str>| Config {
            server: "s".into(),
            login: "l".into(),
            token: "t".into(),
            project: p.map(str::to_string),
        };
        assert!(todo_jql(&cfg(Some("JROZ"))).contains("AND project = JROZ"));
        assert!(!todo_jql(&cfg(None)).contains("project ="));
    }

    #[test]
    fn percent_encodes_what_a_jql_actually_contains() {
        assert_eq!(encode("a = b"), "a%20%3D%20b");
        assert_eq!(encode("currentUser()"), "currentUser%28%29");
        assert_eq!(encode("safe-_.~"), "safe-_.~");
    }
}
