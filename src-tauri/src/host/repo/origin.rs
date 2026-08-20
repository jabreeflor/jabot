//! Parsing a git remote into `{ host, owner, name }`.
//!
//! Parsed the way `gh` does it: `git@github.com:org/repo.git` and
//! `https://github.com/org/repo.git` are the same repository on the same host,
//! because SSH versus HTTPS is a fact about how the *agent* pushes, not about
//! which repository the PR view should ask about
//! (`docs/research/git-and-prs/folders-and-auth.md`).
//!
//! The host is kept rather than assumed: GitHub Enterprise is a different
//! hostname with the same shapes, and `gh` itself is addressed per host
//! (`gh auth token -h <host>`). A folder whose origin is GitLab or a bare
//! local path parses to whatever it is — or to nothing — and still works as a
//! folder; only the PR surface skips it.

/// A remote, split the way every downstream caller wants it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub url: String,
    pub host: String,
    pub owner: String,
    pub name: String,
}

impl Origin {
    /// `owner/name` — the one spelling `gh --repo`, `thread_prs.repo` and the
    /// PR view all use, defined once so they cannot disagree.
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

pub fn parse(url: &str) -> Option<Origin> {
    let raw = url.trim();
    if raw.is_empty() {
        return None;
    }
    let (host, path) = split_host_and_path(raw)?;
    let (owner, name) = split_owner_and_name(path)?;
    Some(Origin {
        url: raw.to_string(),
        host: host.to_ascii_lowercase(),
        owner,
        name,
    })
}

/// `scheme://[user[:pass]@]host[:port]/path` and the scp-like `[user@]host:path`.
fn split_host_and_path(raw: &str) -> Option<(&str, &str)> {
    if let Some((_scheme, rest)) = raw.split_once("://") {
        let rest = rest.strip_prefix_userinfo();
        let (authority, path) = rest.split_once('/')?;
        return Some((strip_port(authority), path));
    }
    // Not a URL. A Windows drive letter or an absolute path is a local remote,
    // which has no host and therefore no forge.
    if raw.starts_with('/') || raw.starts_with('.') {
        return None;
    }
    let (authority, path) = raw.split_once(':')?;
    let authority = authority.strip_prefix_userinfo();
    if authority.is_empty() || path.is_empty() {
        return None;
    }
    Some((authority, path))
}

/// The last segment is the repository; everything before it is the owner. Not
/// `split('/')` into exactly two, because GitLab subgroups are legal owners
/// (`group/subgroup/repo`) and dropping the middle would name a repo that does
/// not exist.
fn split_owner_and_name(path: &str) -> Option<(String, String)> {
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let path = path.trim_end_matches('/');
    let (owner, name) = path.rsplit_once('/')?;
    let owner = owner.trim_matches('/');
    if owner.is_empty() || name.is_empty() {
        return None;
    }
    Some((owner.to_string(), name.to_string()))
}

fn strip_port(authority: &str) -> &str {
    match authority.rsplit_once(':') {
        // Only a numeric tail is a port; `git@host` has already been stripped
        // and an IPv6 literal is not a shape git remotes use here.
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => host,
        _ => authority,
    }
}

trait StripUserinfo {
    fn strip_prefix_userinfo(&self) -> &str;
}

impl StripUserinfo for str {
    fn strip_prefix_userinfo(&self) -> &str {
        match self.split_once('@') {
            Some((_userinfo, rest)) => rest,
            None => self,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(url: &str) -> (String, String, String) {
        let origin = parse(url).unwrap_or_else(|| panic!("expected {url} to parse"));
        (origin.host, origin.owner, origin.name)
    }

    #[test]
    fn ssh_and_https_agree_on_the_repository() {
        let ssh = parsed("git@github.com:jabreeflor/jabot.git");
        let https = parsed("https://github.com/jabreeflor/jabot.git");
        assert_eq!(ssh, https);
        assert_eq!(
            ssh,
            (
                "github.com".to_string(),
                "jabreeflor".to_string(),
                "jabot".to_string()
            )
        );
    }

    #[test]
    fn tolerates_the_shapes_git_actually_writes() {
        assert_eq!(parsed("https://github.com/o/r").1, "o");
        assert_eq!(parsed("https://github.com/o/r/").2, "r");
        assert_eq!(parsed("ssh://git@github.com/o/r.git").0, "github.com");
        assert_eq!(parsed("git://github.com/o/r.git").0, "github.com");
        assert_eq!(
            parsed("https://user:token@github.com/o/r.git").0,
            "github.com"
        );
        assert_eq!(
            parsed("ssh://git@ssh.github.com:443/o/r.git").0,
            "ssh.github.com"
        );
        assert_eq!(parsed("HTTPS://GitHub.com/o/r.git").0, "github.com");
    }

    #[test]
    fn keeps_the_enterprise_host_instead_of_assuming_github() {
        assert_eq!(
            parsed("git@git.corp.example.com:team/thing.git").0,
            "git.corp.example.com"
        );
        assert_eq!(
            parsed("https://gitlab.com/group/sub/thing.git"),
            (
                "gitlab.com".to_string(),
                "group/sub".to_string(),
                "thing".to_string()
            )
        );
    }

    #[test]
    fn a_local_or_malformed_remote_is_no_origin_rather_than_a_wrong_one() {
        assert!(parse("/srv/git/thing.git").is_none());
        assert!(parse("../sibling").is_none());
        assert!(parse("github.com").is_none());
        assert!(parse("git@github.com:").is_none());
        assert!(parse("https://github.com/lonely.git").is_none());
        assert!(parse("").is_none());
    }

    #[test]
    fn slug_is_what_gh_and_the_pr_table_call_it() {
        assert_eq!(
            parse("git@github.com:jabreeflor/jabot.git").unwrap().slug(),
            "jabreeflor/jabot"
        );
    }
}
