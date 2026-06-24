use std::process::Command;

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "AIVoice";

#[cfg(target_os = "windows")]
pub fn set_launch_at_login(enabled: bool) -> anyhow::Result<()> {
    if enabled {
        let exe = std::env::current_exe()?;
        let status = Command::new("reg")
            .args([
                "add",
                RUN_KEY,
                "/v",
                VALUE_NAME,
                "/t",
                "REG_SZ",
                "/d",
                exe.to_string_lossy().as_ref(),
                "/f",
            ])
            .status()?;
        if !status.success() {
            anyhow::bail!("ログイン時起動の登録に失敗しました。");
        }
    } else {
        let status = Command::new("reg")
            .args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])
            .status()?;
        if !status.success() {
            tracing::debug!("launch-at-login value was already absent or could not be removed");
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn set_launch_at_login(_enabled: bool) -> anyhow::Result<()> {
    Ok(())
}
