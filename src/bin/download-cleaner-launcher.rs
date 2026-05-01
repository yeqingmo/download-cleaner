use std::env;
use std::error::Error;
use std::os::unix::process::CommandExt;
use std::process::Command;

fn main() -> Result<(), Box<dyn Error>> {
    let exe = env::current_exe()?;
    let app_dir = exe
        .parent()
        .and_then(|path| path.parent())
        .ok_or("无法定位 app bundle 目录")?;
    let binary = app_dir.join("Resources").join("download-cleaner");

    let mut command = Command::new(binary);
    command.arg("panel");
    command.env("DOWNLOAD_CLEANER_AUTO_INSTALL_LAUNCHD", "1");
    Err(command.exec().into())
}
