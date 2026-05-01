use crate::pathing::{home_dir, log};
use anyhow::{anyhow, Context, Result};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const LABEL: &str = "com.yy.download-cleaner";
const AUTO_INSTALL_ENV: &str = "DOWNLOAD_CLEANER_AUTO_INSTALL_LAUNCHD";

pub fn maybe_install_launch_agent() -> Result<()> {
    if env::var_os(AUTO_INSTALL_ENV).as_deref() != Some(OsStr::new("1")) {
        return Ok(());
    }

    let daemon_binary = env::current_exe().context("无法读取当前可执行文件路径")?;
    let (plist_path, existed_before, needs_write) = ensure_launch_agent_plist(&daemon_binary)?;
    let uid = current_uid()?;
    let domain = format!("gui/{uid}");
    let service = format!("{domain}/{LABEL}");
    let loaded = launchctl_status(&["print", &service])?;

    if loaded && needs_write {
        let _ = launchctl_status(&["bootout", &service]);
        let plist_path_string = plist_path.to_string_lossy().into_owned();
        run_launchctl(&["bootstrap", &domain, &plist_path_string])?;
        run_launchctl(&["kickstart", "-k", &service])?;
    } else if !loaded && !existed_before {
        let plist_path_string = plist_path.to_string_lossy().into_owned();
        run_launchctl(&["bootstrap", &domain, &plist_path_string])?;
        run_launchctl(&["kickstart", "-k", &service])?;
    }

    Ok(())
}

pub fn stop_monitoring() -> Result<()> {
    let uid = current_uid()?;
    let service = service_name(uid);
    let _ = launchctl_status(&["bootout", &service]);
    if monitoring_running()? {
        return Err(anyhow!("监控仍在运行"));
    }
    Ok(())
}

pub fn restart_monitoring() -> Result<()> {
    let daemon_binary = env::current_exe().context("无法读取当前可执行文件路径")?;
    let (plist_path, _existed_before, _needs_write) = ensure_launch_agent_plist(&daemon_binary)?;

    let uid = current_uid()?;
    let domain = format!("gui/{uid}");
    let service = format!("{domain}/{LABEL}");
    let _ = launchctl_status(&["bootout", &service]);

    let plist_path_string = plist_path.to_string_lossy().into_owned();
    run_launchctl(&["bootstrap", &domain, &plist_path_string])?;
    run_launchctl(&["kickstart", "-k", &service])?;
    if !monitoring_running()? {
        return Err(anyhow!("监控未成功启动"));
    }
    Ok(())
}

pub fn monitoring_running() -> Result<bool> {
    let uid = current_uid()?;
    let service = service_name(uid);
    launchctl_status(&["print", &service])
}

fn ensure_launch_agent_plist(daemon_binary: &Path) -> Result<(PathBuf, bool, bool)> {
    let launch_agents_dir = home_dir()?.join("Library").join("LaunchAgents");
    fs::create_dir_all(&launch_agents_dir)?;

    let plist_path = launch_agents_dir.join(format!("{LABEL}.plist"));
    let existed_before = plist_path.exists();
    let plist = render_plist(daemon_binary)?;
    let needs_write = match fs::read_to_string(&plist_path) {
        Ok(existing) => existing != plist,
        Err(_) => true,
    };

    if needs_write {
        fs::write(&plist_path, plist)?;
        fs::set_permissions(&plist_path, fs::Permissions::from_mode(0o644))?;
        log(&format!("已写入 LaunchAgent: {}", plist_path.display()));
    }

    Ok((plist_path, existed_before, needs_write))
}

fn launchctl_status(args: &[&str]) -> Result<bool> {
    let status = Command::new("launchctl")
        .args(args)
        .status()
        .with_context(|| format!("launchctl 失败: {}", args.join(" ")))?;
    Ok(status.success())
}

fn run_launchctl(args: &[&str]) -> Result<()> {
    let status = Command::new("launchctl")
        .args(args)
        .status()
        .with_context(|| format!("launchctl 失败: {}", args.join(" ")))?;
    if !status.success() {
        return Err(anyhow!("launchctl 命令失败: {}", args.join(" ")));
    }
    Ok(())
}

fn current_uid() -> Result<u32> {
    let output = Command::new("id").arg("-u").output().context("无法读取 UID")?;
    if !output.status.success() {
        return Err(anyhow!(
            "读取 UID 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let uid = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
        .context("UID 解析失败")?;
    Ok(uid)
}

fn service_name(uid: u32) -> String {
    format!("gui/{uid}/{LABEL}")
}

fn render_plist(daemon_binary: &Path) -> Result<String> {
    let binary = xml_escape(&daemon_binary.to_string_lossy());
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{binary}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>/tmp/download-cleaner.out.log</string>
  <key>StandardErrorPath</key>
  <string>/tmp/download-cleaner.err.log</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
  </dict>
</dict>
</plist>
"#
    ))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
