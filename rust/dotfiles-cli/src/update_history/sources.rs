//! update-history の curated fallback source と provenance 正規化を集約する。
//!
//! `record` / `backfill-version-only` が release notes source を決める際、この module が package 名・version・
//! homepage・changelog から安定 URL を返す。これらの入力値は外部 source 由来で信頼境界外なので、ここでは
//! repository / notes URL のヒント化と version 固定 URL の正規化だけを担当し、取得や要約は他 module へ委譲する。

use super::wire::PackageSource;

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

pub(crate) fn normalize_homepage(homepage: &str) -> &str {
    homepage.trim_end_matches('/')
}

pub(crate) fn repo_hint_for_package(name: &str, homepage: &str) -> String {
    match (name, normalize_homepage(homepage)) {
        ("nix", "https://nixos.org/nix") => "NixOS/nix".to_string(),
        _ => String::new(),
    }
}

pub(crate) fn package_notes_source(
    name: &str,
    version: &str,
    homepage: &str,
    changelog: &str,
) -> String {
    if !changelog.is_empty() {
        return changelog.to_string();
    }
    match (name, normalize_homepage(homepage)) {
        ("coreutils", "https://www.gnu.org/software/coreutils") => {
            "https://cgit.git.savannah.gnu.org/cgit/coreutils.git/plain/NEWS".to_string()
        }
        ("nix", "https://nixos.org/nix") => {
            nix_release_notes_url(Some(version)).unwrap_or_default()
        }
        ("google-chrome", "https://www.google.com/chrome/browser") => {
            "https://chromereleases.googleblog.com/".to_string()
        }
        ("chromedriver", "https://chromedriver.chromium.org") => {
            chrome_releases_search_url(Some(version)).unwrap_or_default()
        }
        ("docker-compose", _) => {
            github_release_tag_url("docker/compose", Some(version)).unwrap_or_default()
        }
        ("rustfmt", _) => rust_release_notes_url(Some(version)).unwrap_or_default(),
        ("discord", "https://discordapp.com") => "https://discord.com/tags/patch-notes".to_string(),
        ("slack", "https://slack.com/intl/en-jp/downloads/mac")
        | ("slack", "https://slack.com") => "https://slack.com/release-notes/mac".to_string(),
        ("temurin-bin", "https://adoptium.net") => {
            "https://adoptium.net/temurin/release-notes".to_string()
        }
        _ => String::new(),
    }
}

pub(crate) fn backfill_notes_source(
    name: &str,
    notes_url: Option<&str>,
    source: PackageSource,
    repo: Option<&str>,
    old: Option<&str>,
    new: Option<&str>,
) -> Option<String> {
    match (source, name) {
        (PackageSource::Nix, "coreutils") => {
            Some("https://cgit.git.savannah.gnu.org/cgit/coreutils.git/plain/NEWS".to_string())
        }
        (PackageSource::Nix, "discord") => Some("https://discord.com/tags/patch-notes".to_string()),
        (PackageSource::Nix, "nix") => nix_release_notes_url(new),
        (PackageSource::Nix, "slack") => Some("https://slack.com/release-notes/mac".to_string()),
        (PackageSource::Nix, "temurin-bin") => {
            Some("https://adoptium.net/temurin/release-notes".to_string())
        }
        (PackageSource::Brew, "bitwarden") => {
            Some("https://bitwarden.com/help/releasenotes/".to_string())
        }
        (PackageSource::Brew, "codex-app") => {
            Some("https://developers.openai.com/codex/changelog".to_string())
        }
        (PackageSource::Nix, "chromedriver") => chrome_releases_search_url(new),
        (PackageSource::Nix, "docker-compose") => github_release_tag_url("docker/compose", new),
        (PackageSource::Nix, "neovim") => neovim_news_url(new),
        (PackageSource::Nix, "docker") | (PackageSource::Nix, "docker-credential-helpers") => {
            repo.and_then(|repo| github_compare_url(repo, old, new))
        }
        (PackageSource::Nix, "rustfmt") => rust_release_notes_url(new),
        _ => notes_url
            .filter(|url| !url.is_empty())
            .map(std::string::ToString::to_string),
    }
}

pub(crate) fn github_compare_url(
    repo: &str,
    old: Option<&str>,
    new: Option<&str>,
) -> Option<String> {
    let old = old.map(str::trim).filter(|s| !s.is_empty())?;
    let new = new.map(str::trim).filter(|s| !s.is_empty())?;
    Some(format!(
        "https://github.com/{repo}/compare/{}...{}",
        version_tag(old),
        version_tag(new)
    ))
}

pub(crate) fn github_release_tag_url(repo: &str, version: Option<&str>) -> Option<String> {
    let repo = repo.trim();
    let version = version.map(str::trim).filter(|s| !s.is_empty())?;
    Some(format!(
        "https://github.com/{repo}/releases/tag/{}",
        version_tag(version)
    ))
}

pub(crate) fn neovim_news_url(version: Option<&str>) -> Option<String> {
    let version = version?;
    let mut parts = version.trim_start_matches('v').split('.');
    let major = parts.next()?;
    let minor = parts.next()?;
    Some(format!(
        "https://raw.githubusercontent.com/neovim/neovim/master/runtime/doc/news-{major}.{minor}.txt"
    ))
}

pub(crate) fn rust_release_notes_url(version: Option<&str>) -> Option<String> {
    let _version = version.map(str::trim).filter(|s| !s.is_empty())?;
    Some("https://doc.rust-lang.org/stable/releases.html".to_string())
}

pub(crate) fn nix_release_notes_url(version: Option<&str>) -> Option<String> {
    let version = version.map(str::trim).filter(|s| !s.is_empty())?;
    let version = version.trim_start_matches('v');
    let mut parts = version.split('.');
    let major = non_empty(parts.next())?;
    let minor = non_empty(parts.next())?;
    Some(format!(
        "https://nix.dev/manual/nix/{major}.{minor}/release-notes/rl-{major}.{minor}"
    ))
}

pub(crate) fn chrome_releases_search_url(version: Option<&str>) -> Option<String> {
    let version = version.map(str::trim).filter(|s| !s.is_empty())?;
    Some(format!(
        "https://chromereleases.googleblog.com/search?q={version}"
    ))
}

fn version_tag(version: &str) -> String {
    let trimmed = version.trim();
    if trimmed.starts_with('v') {
        trimmed.to_string()
    } else {
        format!("v{trimmed}")
    }
}
