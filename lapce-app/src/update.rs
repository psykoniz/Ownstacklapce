use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use lapce_core::{directory::Directory, meta};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde::Deserialize;

const DEFAULT_RELEASE_REPO: &str = "psykoniz/Ownstack";
const ENV_RELEASE_REPO: &str = "OWNSTACK_RELEASE_REPO";
const ENV_SPARKLE_APPCAST_URL: &str = "OWNSTACK_SPARKLE_APPCAST_URL";
const ENV_WINSPARKLE_APPCAST_URL: &str = "OWNSTACK_WINSPARKLE_APPCAST_URL";

#[derive(Clone, Deserialize, Debug)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub target_commitish: String,
    pub assets: Vec<ReleaseAsset>,
    #[serde(skip)]
    pub version: String,
}

#[derive(Clone, Deserialize, Debug)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
}

#[derive(Clone, Debug)]
struct AppcastRelease {
    version: String,
    asset_name: String,
    asset_url: String,
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
enum UpdaterBackend {
    GitHub,
    LinuxCustom,
    SparkleAppcast,
    WinSparkleAppcast,
}

impl UpdaterBackend {
    fn label(self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::LinuxCustom => "linux-custom",
            Self::SparkleAppcast => "sparkle-appcast",
            Self::WinSparkleAppcast => "winsparkle-appcast",
        }
    }
}

pub fn get_latest_release() -> Result<ReleaseInfo> {
    let backend = select_updater_backend();

    let release = match backend {
        UpdaterBackend::SparkleAppcast => {
            if let Some(url) = appcast_url_for_backend(backend) {
                get_latest_release_from_appcast(&url)
            } else {
                tracing::warn!(
                    "Sparkle backend selected but '{}' is missing; falling back to GitHub",
                    ENV_SPARKLE_APPCAST_URL
                );
                get_latest_release_from_github()
            }
        }
        UpdaterBackend::WinSparkleAppcast => {
            if let Some(url) = appcast_url_for_backend(backend) {
                get_latest_release_from_appcast(&url)
            } else {
                tracing::warn!(
                    "WinSparkle backend selected but '{}' is missing; falling back to GitHub",
                    ENV_WINSPARKLE_APPCAST_URL
                );
                get_latest_release_from_github()
            }
        }
        UpdaterBackend::LinuxCustom | UpdaterBackend::GitHub => {
            get_latest_release_from_github()
        }
    }?;

    tracing::info!(
        "Updater backend '{}' selected release version {}",
        backend.label(),
        release.version
    );

    Ok(release)
}

fn select_updater_backend() -> UpdaterBackend {
    #[cfg(target_os = "macos")]
    {
        if env_non_empty(ENV_SPARKLE_APPCAST_URL).is_some() {
            return UpdaterBackend::SparkleAppcast;
        }
        return UpdaterBackend::GitHub;
    }

    #[cfg(target_os = "windows")]
    {
        if env_non_empty(ENV_WINSPARKLE_APPCAST_URL).is_some() {
            return UpdaterBackend::WinSparkleAppcast;
        }
        return UpdaterBackend::GitHub;
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
    {
        UpdaterBackend::LinuxCustom
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux",
        target_os = "freebsd",
        target_os = "openbsd"
    )))]
    {
        UpdaterBackend::GitHub
    }
}

fn appcast_url_for_backend(backend: UpdaterBackend) -> Option<String> {
    match backend {
        UpdaterBackend::SparkleAppcast => env_non_empty(ENV_SPARKLE_APPCAST_URL),
        UpdaterBackend::WinSparkleAppcast => {
            env_non_empty(ENV_WINSPARKLE_APPCAST_URL)
        }
        _ => None,
    }
}

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn release_repo() -> String {
    env_non_empty(ENV_RELEASE_REPO)
        .unwrap_or_else(|| DEFAULT_RELEASE_REPO.to_string())
}

fn github_release_info_url() -> Result<String> {
    let repo = release_repo();

    match meta::RELEASE {
        meta::ReleaseType::Debug => Err(anyhow!("no release for debug")),
        meta::ReleaseType::Nightly => Ok(format!(
            "https://api.github.com/repos/{repo}/releases/tags/nightly"
        )),
        _ => Ok(format!(
            "https://api.github.com/repos/{repo}/releases/latest"
        )),
    }
}

fn get_latest_release_from_github() -> Result<ReleaseInfo> {
    let url = github_release_info_url()?;
    let resp = lapce_proxy::get_url(&url, Some("OwnStack IDE"))?;
    if !resp.status().is_success() {
        return Err(anyhow!("get release info failed {}", resp.text()?));
    }

    let body = resp.text()?;
    let mut release: ReleaseInfo = serde_json::from_str(&body)?;
    release.version =
        normalize_release_version(&release.tag_name, &release.target_commitish);

    Ok(release)
}

fn get_latest_release_from_appcast(appcast_url: &str) -> Result<ReleaseInfo> {
    let resp = lapce_proxy::get_url(appcast_url, Some("OwnStack IDE"))?;
    if !resp.status().is_success() {
        return Err(anyhow!("get appcast failed {}", resp.text()?));
    }

    let xml = resp.text()?;
    let appcast = parse_appcast_release(&xml)?;

    Ok(ReleaseInfo {
        tag_name: format!("v{}", appcast.version),
        target_commitish: String::new(),
        version: appcast.version,
        assets: vec![ReleaseAsset {
            name: appcast.asset_name,
            browser_download_url: appcast.asset_url,
        }],
    })
}

fn parse_appcast_release(xml: &str) -> Result<AppcastRelease> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut in_item = false;
    let mut current_tag: Option<Vec<u8>> = None;

    let mut title: Option<String> = None;
    let mut version: Option<String> = None;
    let mut asset_url: Option<String> = None;
    let mut asset_name: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.name().as_ref().to_vec();
                if tag_matches(&name, b"item") {
                    in_item = true;
                    current_tag = None;
                } else if in_item {
                    if tag_matches(&name, b"enclosure") {
                        parse_enclosure(
                            &e,
                            &mut asset_url,
                            &mut asset_name,
                            &mut version,
                        )?;
                    }
                    current_tag = Some(name);
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name().as_ref().to_vec();
                if in_item && tag_matches(&name, b"enclosure") {
                    parse_enclosure(
                        &e,
                        &mut asset_url,
                        &mut asset_name,
                        &mut version,
                    )?;
                }
            }
            Ok(Event::Text(e)) => {
                if !in_item {
                    buf.clear();
                    continue;
                }

                let value = e.unescape()?.into_owned();
                if value.trim().is_empty() {
                    buf.clear();
                    continue;
                }

                if let Some(tag) = current_tag.as_ref() {
                    if tag_matches(tag, b"title") {
                        title = Some(value.trim().to_string());
                    } else if tag_matches(tag, b"version")
                        || tag_matches(tag, b"shortVersionString")
                    {
                        version = Some(value.trim().to_string());
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name().as_ref().to_vec();
                if tag_matches(&name, b"item") {
                    break;
                }
                current_tag = None;
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(err) => {
                return Err(anyhow!("failed to parse appcast XML: {err}"));
            }
        }

        buf.clear();
    }

    let asset_url = asset_url
        .ok_or_else(|| anyhow!("appcast item does not contain enclosure url"))?;
    let asset_name = asset_name
        .or_else(|| file_name_from_url(&asset_url))
        .ok_or_else(|| anyhow!("cannot derive asset filename from appcast"))?;

    let version = version
        .or_else(|| title.as_ref().and_then(extract_semver_like))
        .unwrap_or_else(|| "0.0.0".to_string());

    let normalized_version = version.trim().trim_start_matches('v').to_string();

    Ok(AppcastRelease {
        version: normalized_version,
        asset_name,
        asset_url,
    })
}

fn parse_enclosure(
    e: &BytesStart<'_>,
    asset_url: &mut Option<String>,
    asset_name: &mut Option<String>,
    version: &mut Option<String>,
) -> Result<()> {
    for attr in e.attributes() {
        let attr = attr?;
        let key = attr.key.as_ref();
        let value = attr.unescape_value()?.into_owned();

        if tag_matches(key, b"url") {
            *asset_name = file_name_from_url(&value);
            *asset_url = Some(value);
        } else if tag_matches(key, b"version")
            || tag_matches(key, b"shortVersionString")
        {
            *version = Some(value.trim().to_string());
        }
    }

    Ok(())
}

fn extract_semver_like(s: &String) -> Option<String> {
    let bytes = s.as_bytes();
    let mut start = None;
    let mut seen_dot = false;

    for (i, c) in bytes.iter().enumerate() {
        if c.is_ascii_digit() {
            if start.is_none() {
                start = Some(i);
            }
            continue;
        }

        if *c == b'.' && start.is_some() {
            seen_dot = true;
            continue;
        }

        if start.is_some() {
            let st = start.unwrap_or(0);
            let candidate = &s[st..i];
            if seen_dot {
                return Some(candidate.to_string());
            }
            start = None;
            seen_dot = false;
        }
    }

    if let Some(st) = start {
        let candidate = &s[st..];
        if seen_dot {
            return Some(candidate.to_string());
        }
    }

    None
}

fn file_name_from_url(url: &str) -> Option<String> {
    let no_fragment = url.split('#').next().unwrap_or(url);
    let no_query = no_fragment.split('?').next().unwrap_or(no_fragment);
    no_query
        .rsplit('/')
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn tag_matches(name: &[u8], expected_local: &[u8]) -> bool {
    if name == expected_local {
        return true;
    }

    if name.len() > expected_local.len()
        && name.ends_with(expected_local)
        && name[name.len() - expected_local.len() - 1] == b':'
    {
        return true;
    }

    false
}

fn normalize_release_version(tag_name: &str, target_commitish: &str) -> String {
    match tag_name {
        "nightly" => {
            let short_commit = target_commitish.get(..7).unwrap_or(target_commitish);
            format!("{}+Nightly.{short_commit}", env!("CARGO_PKG_VERSION"))
        }
        _ => tag_name.strip_prefix('v').unwrap_or(tag_name).to_owned(),
    }
}

pub fn download_release(release: &ReleaseInfo) -> Result<PathBuf> {
    let dir =
        Directory::updates_directory().ok_or_else(|| anyhow!("no directory"))?;

    let candidates = platform_asset_candidates()?;

    for candidate in &candidates {
        if let Some(asset) = release
            .assets
            .iter()
            .find(|asset| asset.name.eq_ignore_ascii_case(candidate))
        {
            return download_release_asset(asset, &dir);
        }
    }

    if release.assets.len() == 1 {
        return download_release_asset(&release.assets[0], &dir);
    }

    Err(anyhow!(
        "can't download release; no matching assets for this platform. candidates={:?}",
        candidates
    ))
}

fn platform_asset_candidates() -> Result<Vec<&'static str>> {
    let names = match std::env::consts::OS {
        "macos" => vec!["OwnStack-macos.dmg", "Lapce-macos.dmg"],
        "linux" => match std::env::consts::ARCH {
            "aarch64" => {
                vec!["ownstack-linux-arm64.tar.gz", "lapce-linux-arm64.tar.gz"]
            }
            "x86_64" => {
                vec!["ownstack-linux-amd64.tar.gz", "lapce-linux-amd64.tar.gz"]
            }
            _ => return Err(anyhow!("arch not supported")),
        },
        #[cfg(feature = "portable")]
        "windows" => vec![
            "OwnStack-windows-portable.zip",
            "Lapce-windows-portable.zip",
        ],
        #[cfg(not(feature = "portable"))]
        "windows" => vec!["OwnStack-windows.msi", "Lapce-windows.msi"],
        _ => return Err(anyhow!("os not supported")),
    };

    Ok(names)
}

fn download_release_asset(asset: &ReleaseAsset, dir: &Path) -> Result<PathBuf> {
    let file_path = dir.join(&asset.name);

    let mut resp = lapce_proxy::get_url(&asset.browser_download_url, None)?;
    if !resp.status().is_success() {
        return Err(anyhow!("download file error {}", resp.text()?));
    }

    let mut out = std::fs::File::create(&file_path)?;
    resp.copy_to(&mut out)?;
    Ok(file_path)
}

#[cfg(target_os = "macos")]
pub fn extract(src: &Path, process_path: &Path) -> Result<PathBuf> {
    let info = dmg::Attach::new(src).with()?;
    let dest = process_path.parent().ok_or_else(|| anyhow!("no parent"))?;
    let dest = if dest.file_name().and_then(|s| s.to_str()) == Some("MacOS") {
        dest.parent().unwrap().parent().unwrap().parent().unwrap()
    } else {
        dest
    };

    let _ = std::fs::remove_dir_all(dest.join("OwnStack.app"));
    let _ = std::fs::remove_dir_all(dest.join("Lapce.app"));

    let source_app = if info.mount_point.join("OwnStack.app").exists() {
        info.mount_point.join("OwnStack.app")
    } else {
        info.mount_point.join("Lapce.app")
    };

    fs_extra::copy_items(
        &[source_app],
        dest,
        &fs_extra::dir::CopyOptions {
            overwrite: true,
            skip_exist: false,
            buffer_size: 64000,
            copy_inside: true,
            content_only: false,
            depth: 0,
        },
    )?;

    if dest.join("OwnStack.app").exists() {
        Ok(dest.join("OwnStack.app"))
    } else {
        Ok(dest.join("Lapce.app"))
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
pub fn extract(src: &Path, process_path: &Path) -> Result<PathBuf> {
    let tar_gz = std::fs::File::open(src)?;
    let tar = flate2::read::GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(tar);
    let parent = src.parent().ok_or_else(|| anyhow::anyhow!("no parent"))?;
    archive.unpack(parent)?;

    let extracted = [
        parent.join("OwnStack").join("ownstack-ide"),
        parent.join("OwnStack").join("lapce"),
        parent.join("Lapce").join("ownstack-ide"),
        parent.join("Lapce").join("lapce"),
    ]
    .into_iter()
    .find(|p| p.exists())
    .ok_or_else(|| anyhow!("cannot locate extracted linux binary"))?;

    if process_path.exists() {
        std::fs::remove_file(process_path)?;
    }
    std::fs::copy(&extracted, process_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(process_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(process_path, perms)?;
    }

    Ok(process_path.to_path_buf())
}

#[cfg(all(target_os = "windows", feature = "portable"))]
pub fn extract(src: &Path, process_path: &Path) -> Result<PathBuf> {
    let parent = src
        .parent()
        .ok_or_else(|| anyhow::anyhow!("src has no parent"))?;
    let dst_parent = process_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("process_path has no parent"))?;

    {
        let mut archive = zip::ZipArchive::new(std::fs::File::open(src)?)?;
        archive.extract(parent)?;
    }

    let current_name = process_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("ownstack-ide.exe");
    let backup_name = format!("{current_name}.bak");

    std::fs::rename(process_path, dst_parent.join(backup_name))?;

    let extracted = [parent.join("ownstack-ide.exe"), parent.join("lapce.exe")]
        .into_iter()
        .find(|p| p.exists())
        .ok_or_else(|| anyhow!("cannot locate extracted windows portable binary"))?;

    std::fs::copy(extracted, process_path)?;

    Ok(process_path.to_path_buf())
}

#[cfg(all(target_os = "windows", not(feature = "portable")))]
pub fn extract(src: &Path, _process_path: &Path) -> Result<PathBuf> {
    // We downloaded an uncompressed MSI installer, nothing to extract.
    Ok(src.to_path_buf())
}

#[cfg(target_os = "macos")]
pub fn restart(path: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let _ = std::process::Command::new("open")
        .arg("-n")
        .arg(path)
        .arg("--args")
        .arg("-n")
        .exec();
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
pub fn restart(path: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let _ = std::process::Command::new(path).arg("-n").exec();
    Ok(())
}

#[cfg(all(target_os = "windows", feature = "portable"))]
pub fn restart(path: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x00000008;
    let process_id = std::process::id();
    let path = path
        .to_str()
        .ok_or_else(|| anyhow!("can't get path to str"))?;
    std::process::Command::new("cmd")
        .raw_arg(format!(
            r#"/C taskkill /PID {process_id} & start "" "{path}""#
        ))
        .creation_flags(DETACHED_PROCESS)
        .spawn()?;
    Ok(())
}

#[cfg(all(target_os = "windows", not(feature = "portable")))]
pub fn restart(path: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x00000008;
    let process_id = std::process::id();
    let path = path
        .to_str()
        .ok_or_else(|| anyhow!("can't get path to str"))?;

    let ownstack_exe = std::env::current_exe()
        .map_err(|err| anyhow!("can't get path to exe: {err}"))?;
    let ownstack_exe = ownstack_exe
        .to_str()
        .ok_or_else(|| anyhow!("can't convert exe path to str"))?;

    std::process::Command::new("cmd")
        .raw_arg(format!(
            r#"/C taskkill /PID {process_id} & msiexec /i "{path}" /qb & start "" "{ownstack_exe}""#,
        ))
        .creation_flags(DETACHED_PROCESS)
        .spawn()?;
    Ok(())
}

#[cfg(all(target_os = "windows", feature = "portable"))]
pub fn cleanup() {
    if let Ok(process_path) = std::env::current_exe() {
        if let Some(dst_parent) = process_path.parent() {
            if let Some(name) = process_path.file_name().and_then(|n| n.to_str()) {
                let backup = dst_parent.join(format!("{name}.bak"));
                if let Err(err) = std::fs::remove_file(backup) {
                    tracing::error!("{err:?}");
                }
            }
        }
    }
}

#[cfg(any(
    not(target_os = "windows"),
    all(target_os = "windows", not(feature = "portable"))
))]
pub fn cleanup() {
    // Nothing to do yet.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_appcast_release_extracts_version_and_asset() {
        let xml = r#"
        <rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
          <channel>
            <item>
              <title>OwnStack 0.5.0</title>
              <enclosure
                url="https://example.com/OwnStack-macos.dmg"
                sparkle:version="0.5.0"
                sparkle:shortVersionString="0.5.0" />
            </item>
          </channel>
        </rss>
        "#;

        let parsed = parse_appcast_release(xml).expect("parse appcast");
        assert_eq!(parsed.version, "0.5.0");
        assert_eq!(parsed.asset_name, "OwnStack-macos.dmg");
        assert_eq!(parsed.asset_url, "https://example.com/OwnStack-macos.dmg");
    }

    #[test]
    fn file_name_from_url_works_with_query_and_fragment() {
        let url = "https://example.com/download/OwnStack-windows.msi?x=1#frag";
        assert_eq!(
            file_name_from_url(url).as_deref(),
            Some("OwnStack-windows.msi")
        );
    }
}
