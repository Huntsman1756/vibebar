use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub fn normalize_provider_id(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

pub fn provider_label(id: &str) -> String {
    match id {
        "nan" => "NaN".into(),
        "opencode-go" => "OpenCode Go".into(),
        other => other
            .split(['-', '_'])
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

pub fn normalize_remote_url(remote: &str) -> Option<String> {
    let remote = remote.trim();
    if remote.is_empty() || remote.len() > 256 || remote.chars().any(|ch| ch.is_control()) {
        return None;
    }

    if remote.contains("://") {
        normalize_url_like_remote(remote)
    } else if remote.contains(':') {
        normalize_scp_like_remote(remote)
    } else {
        None
    }
}

pub fn repository_identifier(project_dir: &Path) -> String {
    let directory_name = project_dir.file_name().and_then(|name| name.to_str());
    let resolver = RepositoryResolver::new(true);
    resolver
        .resolve(project_dir)
        .unwrap_or_else(|| repository_identifier_from_remote(None, directory_name))
}

pub fn repository_identifier_from_remote(
    remote: Option<&str>,
    directory_name: Option<&str>,
) -> String {
    remote
        .and_then(normalize_remote_url)
        .unwrap_or_else(|| local_repository_identifier(directory_name))
}

pub struct RepositoryResolver {
    enabled: bool,
    cache: Mutex<HashMap<PathBuf, Option<String>>>,
}

impl RepositoryResolver {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn resolve(&self, project_dir: &Path) -> Option<String> {
        if !self.enabled {
            return None;
        }

        let key = project_dir.to_path_buf();
        if let Some(cached) = self
            .cache
            .lock()
            .expect("repository resolver cache poisoned")
            .get(&key)
            .cloned()
        {
            return cached;
        }

        let resolved = resolve_repository_identifier(project_dir);
        self.cache
            .lock()
            .expect("repository resolver cache poisoned")
            .insert(key, resolved.clone());
        resolved
    }
}

fn resolve_repository_identifier(project_dir: &Path) -> Option<String> {
    let config_path = resolve_git_config_path(project_dir)?;
    let remote = read_origin_remote(&config_path)?;
    normalize_remote_url(&remote)
}

fn resolve_git_config_path(project_dir: &Path) -> Option<PathBuf> {
    let dot_git = project_dir.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git.join("config"));
    }

    if dot_git.is_file() {
        let git_dir = read_git_dir_reference(&dot_git, project_dir)?;
        let git_dir = canonicalize_existing(git_dir)?;
        let common_dir = canonicalize_existing(read_common_dir(&git_dir)?)?;
        if is_safe_worktree_gitdir(&git_dir, &common_dir) {
            let common_config = common_dir.join("config");
            if common_config.is_file() {
                return Some(common_config);
            }
        }
    }

    None
}

fn read_git_dir_reference(gitfile: &Path, project_dir: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(gitfile).ok()?;
    let reference = content.trim();
    let reference = reference.strip_prefix("gitdir:")?.trim();
    if reference.is_empty() {
        return None;
    }

    let path = Path::new(reference);
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_dir.join(path)
    })
}

fn read_common_dir(git_dir: &Path) -> Option<PathBuf> {
    let common_dir = git_dir.join("commondir");
    let content = std::fs::read_to_string(common_dir).ok()?;
    let reference = content.trim();
    if reference.is_empty() {
        return None;
    }

    let path = Path::new(reference);
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        git_dir.join(path)
    })
}

fn canonicalize_existing(path: PathBuf) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

fn is_safe_worktree_gitdir(git_dir: &Path, common_dir: &Path) -> bool {
    common_dir.file_name().and_then(|name| name.to_str()) == Some(".git")
        && git_dir.starts_with(common_dir)
        && git_dir
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            == Some("worktrees")
}

fn read_origin_remote(config_path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(config_path).ok()?;
    let mut in_origin_section = false;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            in_origin_section = is_remote_origin_section(section);
            continue;
        }

        if !in_origin_section {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            if key.trim().eq_ignore_ascii_case("url") {
                let value = value.trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }

    None
}

fn is_remote_origin_section(section: &str) -> bool {
    let mut parts = section.split_whitespace();
    matches!(parts.next(), Some(name) if name.eq_ignore_ascii_case("remote"))
        && matches!(parts.next(), Some(name) if name.trim_matches('"').eq_ignore_ascii_case("origin"))
        && parts.next().is_none()
}

fn normalize_url_like_remote(remote: &str) -> Option<String> {
    let (scheme, rest) = remote.split_once("://")?;
    if !matches!(scheme, "http" | "https" | "ssh") {
        return None;
    }

    let rest = strip_query_and_fragment(rest).trim_end_matches('/');
    let (authority, path) = rest.split_once('/')?;
    let host = strip_credentials(authority);
    normalize_host_path(host, path)
}

fn normalize_scp_like_remote(remote: &str) -> Option<String> {
    let rest = strip_query_and_fragment(remote).trim_end_matches('/');
    let colon = rest.find(':')?;
    let authority = &rest[..colon];
    let path = &rest[colon + 1..];
    if path.is_empty() || path.starts_with('/') {
        return None;
    }
    let host = strip_credentials(authority);
    normalize_host_path(host, path)
}

fn strip_query_and_fragment(input: &str) -> &str {
    input.split(['?', '#']).next().unwrap_or(input)
}

fn strip_credentials(authority: &str) -> &str {
    authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority)
}

fn normalize_host_path(host: &str, path: &str) -> Option<String> {
    if host.is_empty() || path.is_empty() {
        return None;
    }

    let segments: Vec<_> = path.split('/').collect();
    if segments.is_empty()
        || segments.iter().any(|segment| {
            segment.is_empty()
                || *segment == "."
                || *segment == ".."
                || segment.chars().any(|ch| ch.is_control())
        })
    {
        return None;
    }

    let normalized_path = if let Some(last) = segments.last() {
        if last == &".git" {
            return None;
        } else if let Some(stripped) = last.strip_suffix(".git") {
            if stripped.is_empty() {
                return None;
            }
            let mut normalized = segments[..segments.len() - 1].join("/");
            if !normalized.is_empty() {
                normalized.push('/');
            }
            normalized.push_str(stripped);
            normalized
        } else {
            segments.join("/")
        }
    } else {
        return None;
    };

    if normalized_path.len() > 256 || host.len() + normalized_path.len() + 1 > 256 {
        return None;
    }

    Some(format!("{host}/{normalized_path}"))
}

fn local_repository_identifier(directory_name: Option<&str>) -> String {
    let directory_name = directory_name
        .and_then(normalize_local_name)
        .unwrap_or_else(|| "unknown".to_string());
    format!("local/{directory_name}")
}

fn normalize_local_name(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty()
        || name.len() > 256
        || name == "."
        || name == ".."
        || name
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '/' | '\\'))
    {
        return None;
    }
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_provider_id, normalize_remote_url, provider_label, repository_identifier,
        repository_identifier_from_remote, RepositoryResolver,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn normalizes_and_labels_known_and_unknown_providers() {
        assert_eq!(normalize_provider_id("NaN"), "nan");
        assert_eq!(normalize_provider_id("opencode-go"), "opencode-go");
        assert_eq!(normalize_provider_id("custom-provider"), "custom-provider");
        assert_eq!(provider_label("nan"), "NaN");
        assert_eq!(provider_label("opencode-go"), "OpenCode Go");
    }

    #[test]
    fn normalizes_supported_remote_urls() {
        assert_eq!(
            normalize_remote_url("git@github.com:Acme/Private.git"),
            Some("github.com/Acme/Private".into())
        );
        assert_eq!(
            normalize_remote_url("git@github.com:Acme/Private.git.git"),
            Some("github.com/Acme/Private.git".into())
        );
        assert_eq!(
            normalize_remote_url("https://github.com/Acme/Private.git"),
            Some("github.com/Acme/Private".into())
        );
        assert_eq!(
            normalize_remote_url("https://gitlab.example/team/tool.git"),
            Some("gitlab.example/team/tool".into())
        );
    }

    #[test]
    fn rejects_unsafe_remote_text() {
        assert_eq!(normalize_remote_url("git@github.com:.git"), None);
        assert_eq!(normalize_remote_url("https://github.com/.git"), None);
        assert_eq!(
            normalize_remote_url("git@github.com:Acme/\u{0007}Private.git"),
            None
        );
        assert_eq!(normalize_remote_url("git@github.com:/Private.git"), None);
        assert_eq!(
            normalize_remote_url("git@github.com:Acme/../Private.git"),
            None
        );
    }

    #[test]
    fn falls_back_to_local_repository_labels() {
        assert_eq!(
            repository_identifier_from_remote(None, Some("client-app")),
            "local/client-app"
        );
        assert_eq!(
            repository_identifier_from_remote(None, None),
            "local/unknown"
        );
    }

    #[test]
    fn repository_identifier_uses_local_fallback_without_git_metadata() {
        assert_eq!(repository_identifier(Path::new("/")), "local/unknown");
    }

    fn unique_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}"))
    }

    #[test]
    fn resolver_is_disabled_by_flag() {
        let dir = unique_dir("repository-resolver-disabled");
        fs::create_dir_all(&dir).expect("create temp dir");
        let resolver = RepositoryResolver::new(false);
        assert_eq!(resolver.resolve(&dir), None);
    }

    #[test]
    fn resolves_remote_from_regular_git_config() {
        let repo_dir = unique_dir("repository-resolver-regular");
        let git_dir = repo_dir.join(".git");
        fs::create_dir_all(&git_dir).expect("create git dir");
        fs::write(
            git_dir.join("config"),
            r#"[remote "origin"]
	url = git@github.com:Acme/Private.git
"#,
        )
        .expect("write git config");

        let resolver = RepositoryResolver::new(true);
        assert_eq!(
            resolver.resolve(&repo_dir),
            Some("github.com/Acme/Private".into())
        );
    }

    #[test]
    fn resolves_remote_through_worktree_commondir() {
        let root_dir = unique_dir("repository-resolver-worktree-root");
        let common_git_dir = root_dir.join(".git");
        fs::create_dir_all(&common_git_dir).expect("create common git dir");
        fs::write(
            common_git_dir.join("config"),
            r#"[remote "origin"]
	url = https://gitlab.example/team/tool.git
"#,
        )
        .expect("write common config");

        let worktree_dir = unique_dir("repository-resolver-worktree");
        let worktree_git_dir = root_dir.join(".git").join("worktrees").join("client-app");
        fs::create_dir_all(&worktree_dir).expect("create worktree dir");
        fs::create_dir_all(&worktree_git_dir).expect("create worktree git dir");
        fs::write(worktree_git_dir.join("commondir"), "../..").expect("write commondir");
        fs::write(
            worktree_dir.join(".git"),
            format!("gitdir: {}\n", worktree_git_dir.display()),
        )
        .expect("write gitfile");

        let resolver = RepositoryResolver::new(true);
        assert_eq!(
            resolver.resolve(&worktree_dir),
            Some("gitlab.example/team/tool".into())
        );
    }

    #[test]
    fn rejects_unsafe_worktree_pointer_to_unrelated_config() {
        let unrelated_root = unique_dir("repository-resolver-unrelated");
        let unrelated_git_dir = unrelated_root.join("outside");
        let unrelated_common_dir = unrelated_root.join("metadata");
        fs::create_dir_all(&unrelated_git_dir).expect("create unrelated git dir");
        fs::create_dir_all(&unrelated_common_dir).expect("create unrelated common dir");
        fs::write(unrelated_git_dir.join("commondir"), "../metadata").expect("write commondir");
        fs::write(
            unrelated_common_dir.join("config"),
            r#"[remote "origin"]
	url = https://example.invalid/unsafe/target.git
"#,
        )
        .expect("write unrelated config");

        let repo_dir = unique_dir("repository-resolver-unsafe-pointer");
        fs::create_dir_all(&repo_dir).expect("create repo dir");
        fs::write(
            repo_dir.join(".git"),
            format!("gitdir: {}\n", unrelated_git_dir.display()),
        )
        .expect("write gitfile");

        let resolver = RepositoryResolver::new(true);
        assert_eq!(resolver.resolve(&repo_dir), None);
    }

    #[test]
    fn resolves_missing_or_non_git_paths_to_none() {
        let missing_dir = unique_dir("repository-resolver-missing");
        let non_git_dir = unique_dir("repository-resolver-non-git");
        fs::create_dir_all(&non_git_dir).expect("create non-git dir");

        let resolver = RepositoryResolver::new(true);
        assert_eq!(resolver.resolve(&missing_dir), None);
        assert_eq!(resolver.resolve(&non_git_dir), None);
    }

    #[test]
    fn returns_none_when_origin_remote_is_missing() {
        let repo_dir = unique_dir("repository-resolver-no-remote");
        let git_dir = repo_dir.join(".git");
        fs::create_dir_all(&git_dir).expect("create git dir");
        fs::write(
            git_dir.join("config"),
            "[core]\n\trepositoryformatversion = 0\n",
        )
        .expect("write git config");

        let resolver = RepositoryResolver::new(true);
        assert_eq!(resolver.resolve(&repo_dir), None);
    }
}
