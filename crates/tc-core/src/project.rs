//! Project inspection: git status, folder scanning, and AI summaries.

use serde::Serialize;
use std::path::Path;
use std::process::{Command, Stdio};

fn hidden(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GitInfo {
    pub branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub changed: u32,
}

pub fn parse_git_status(out: &str) -> GitInfo {
    let mut info = GitInfo {
        branch: None,
        ahead: 0,
        behind: 0,
        changed: 0,
    };
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            let (head, tracking) = match rest.split_once(" [") {
                Some((h, t)) => (h, Some(t.trim_end_matches(']'))),
                None => (rest, None),
            };
            let branch = head.split("...").next().unwrap_or(head);
            let branch = branch
                .strip_prefix("No commits yet on ")
                .unwrap_or(branch)
                .to_string();
            info.branch = Some(branch);
            if let Some(t) = tracking {
                for part in t.split(", ") {
                    if let Some(n) = part.strip_prefix("ahead ") {
                        info.ahead = n.parse().unwrap_or(0);
                    } else if let Some(n) = part.strip_prefix("behind ") {
                        info.behind = n.parse().unwrap_or(0);
                    }
                }
            }
        } else if !line.trim().is_empty() {
            info.changed += 1;
        }
    }
    info
}

pub fn git_status(path: &str) -> Option<GitInfo> {
    let out = hidden(
        Command::new("git")
            .args(["-C", path, "status", "--porcelain=v1", "-b"])
            .stdin(Stdio::null()),
    )
    .output()
    .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_git_status(&String::from_utf8_lossy(&out.stdout)))
}

/// Lists `root` and its direct subfolders that look like projects.
pub fn scan_projects(root: &str) -> Vec<String> {
    let mut found = Vec::new();
    let root = Path::new(root);
    let is_project = |p: &Path| {
        [".git", "package.json", "Cargo.toml", "pyproject.toml", "CLAUDE.md"]
            .iter()
            .any(|m| p.join(m).exists())
    };
    if is_project(root) {
        found.push(root.to_string_lossy().to_string());
    }
    if let Ok(entries) = std::fs::read_dir(root) {
        let mut dirs: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .filter(|p| {
                !p.file_name()
                    .map(|n| n.to_string_lossy().starts_with('.'))
                    .unwrap_or(true)
            })
            .filter(|p| is_project(p))
            .collect();
        dirs.sort();
        found.extend(dirs.into_iter().map(|p| p.to_string_lossy().to_string()));
    }
    found
}

fn read_head(path: &Path, max_chars: usize) -> Option<String> {
    let s = std::fs::read_to_string(path).ok()?;
    Some(s.chars().take(max_chars).collect())
}

/// The language a summary should be written in, from a UI language code.
fn summary_language(lang: &str) -> &'static str {
    match lang {
        "ja" => "Japanese",
        _ => "English",
    }
}

/// Builds the prompt for a short project summary from local files.
pub fn summary_prompt(path: &str, lang: &str) -> String {
    let dir = Path::new(path);
    let mut ctx = String::new();
    for name in ["README.md", "readme.md", "CLAUDE.md", "docs/design.md"] {
        if let Some(text) = read_head(&dir.join(name), 4000) {
            ctx.push_str(&format!("\n--- {name} ---\n{text}\n"));
        }
    }
    if let Some(text) = read_head(&dir.join("package.json"), 1500) {
        ctx.push_str(&format!("\n--- package.json ---\n{text}\n"));
    }
    if let Some(text) = read_head(&dir.join("Cargo.toml"), 1500) {
        ctx.push_str(&format!("\n--- Cargo.toml ---\n{text}\n"));
    }
    if let Ok(out) = hidden(
        Command::new("git")
            .args(["-C", path, "log", "--oneline", "-20"])
            .stdin(Stdio::null()),
    )
    .output()
    {
        if out.status.success() {
            ctx.push_str(&format!(
                "\n--- git log ---\n{}\n",
                String::from_utf8_lossy(&out.stdout)
            ));
        }
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        let names: Vec<String> = entries
            .flatten()
            .take(60)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        ctx.push_str(&format!("\n--- files ---\n{}\n", names.join(", ")));
    }
    format!(
        "Below is information collected from a software project's folder. \
Summarize what the project is for, what it is built with, and its current state \
in 2-3 short sentences, written in {}. Output only the summary, with no preamble or headings.\n\
Folder name: {}\n{}",
        summary_language(lang),
        dir.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        ctx
    )
}

/// Runs `claude -p` with tools disabled to summarize the project. Blocking.
pub fn generate_summary(claude: &Path, path: &str, lang: &str) -> Result<String, String> {
    use std::io::Write;
    let prompt = summary_prompt(path, lang);
    let mut command = Command::new(claude);
    for k in crate::launch::inherited_session_vars() {
        command.env_remove(k);
    }
    let mut child = hidden(
        command
            .args([
                "-p",
                "--tools",
                "",
                "--max-turns",
                "1",
                "--output-format",
                "text",
                "--no-session-persistence",
            ])
            .current_dir(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .spawn()
    .map_err(|e| format!("Could not start claude: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "claude -p failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_branch_and_tracking() {
        let g = parse_git_status("## main...origin/main [ahead 2, behind 1]\n M a.rs\n?? b.rs\n");
        assert_eq!(g.branch.as_deref(), Some("main"));
        assert_eq!((g.ahead, g.behind, g.changed), (2, 1, 2));
    }

    #[test]
    fn parses_fresh_repo() {
        let g = parse_git_status("## No commits yet on master\n");
        assert_eq!(g.branch.as_deref(), Some("master"));
        assert_eq!(g.changed, 0);
    }
}
