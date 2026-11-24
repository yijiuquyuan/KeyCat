//! Start at login. Normally through the per-user Run key: the value name is the product name,
//! which is what the Tauri uninstaller removes, and what the installer hook writes when it
//! carries the setting over from the 1.0.x Electron build. Windows skips elevated programs listed
//! there, so when KeyCat runs as administrator it starts from a scheduled task instead, which
//! starts it elevated at login without a UAC prompt. Creating that task needs elevation. A
//! normal start of KeyCat with the administrator setting on runs that same task too, so it comes
//! up elevated without asking again.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Off,
    RunKey,
    Task,
}

#[cfg(windows)]
mod imp {
    use std::io::{Error, ErrorKind, Result};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Output};

    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    use super::Mode;

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "KeyCat";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn user() -> String {
        std::env::var("USERNAME").unwrap_or_default()
    }

    /// One task per Windows account, so two people on one PC don't overwrite each other's.
    fn task_name() -> String {
        format!("KeyCat ({})", user())
    }

    fn schtasks(args: &[&str]) -> Result<Output> {
        Command::new("schtasks").args(args).creation_flags(CREATE_NO_WINDOW).output()
    }

    fn check(output: Output) -> Result<()> {
        if output.status.success() {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(Error::other(format!("schtasks failed: {text}")))
    }

    fn task_exists() -> bool {
        schtasks(&["/Query", "/TN", &task_name()]).map(|output| output.status.success()).unwrap_or(false)
    }

    fn escape(text: &str) -> String {
        text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
    }

    /// Whether the task exists and starts this very program, not a copy installed elsewhere
    /// before. Anything unexpected in the answer counts as no.
    pub fn task_runs_this() -> bool {
        let Ok(exe) = std::env::current_exe() else { return false };
        let want = format!("<Command>{}</Command>", escape(&exe.display().to_string())).to_lowercase();
        schtasks(&["/Query", "/TN", &task_name(), "/XML"])
            .is_ok_and(|output| output.status.success() && text(&output.stdout).to_lowercase().contains(&want))
    }

    /// schtasks may answer in UTF-16 (with a byte order mark) or in a byte encoding.
    fn text(bytes: &[u8]) -> String {
        match bytes {
            [0xFF, 0xFE, rest @ ..] => {
                let units: Vec<u16> = rest.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
                String::from_utf16_lossy(&units)
            }
            _ => String::from_utf8_lossy(bytes).into_owned(),
        }
    }

    /// Starts KeyCat through the task, elevated and without a UAC prompt. False if there is no
    /// task for this program or it would not start.
    pub fn run_task() -> bool {
        task_runs_this() && schtasks(&["/Run", "/TN", &task_name()]).is_ok_and(|output| output.status.success())
    }

    /// Logs on with this account, highest privileges, no time limit, also on battery.
    fn create_task() -> Result<()> {
        let exe = std::env::current_exe()?;
        let domain = std::env::var("USERDOMAIN").unwrap_or_default();
        let account = escape(&if domain.is_empty() { user() } else { format!(r"{domain}\{}", user()) });
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Starts KeyCat as administrator at login.</Description></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{account}</UserId></LogonTrigger></Triggers>
  <Principals>
    <Principal id="Author"><UserId>{account}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author"><Exec><Command>{}</Command></Exec></Actions>
</Task>
"#,
            escape(&exe.display().to_string())
        );
        // schtasks reads task XML as UTF-16 with a byte order mark.
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
        let file = std::env::temp_dir().join("keycat-task.xml");
        std::fs::write(&file, bytes)?;
        let created = schtasks(&["/Create", "/F", "/TN", &task_name(), "/XML", &file.display().to_string()]);
        let _ = std::fs::remove_file(&file);
        check(created?)
    }

    fn run_key_set() -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(RUN)
            .and_then(|key| key.get_value::<String, _>(NAME))
            .is_ok()
    }

    /// Asks schtasks, which takes a moment, so callers keep the answer.
    pub fn current() -> Mode {
        if task_exists() {
            Mode::Task
        } else if run_key_set() {
            Mode::RunKey
        } else {
            Mode::Off
        }
    }

    pub fn set(mode: Mode) -> Result<()> {
        // Only one of the two may stay, or KeyCat would start twice.
        if mode != Mode::Task && task_exists() {
            check(schtasks(&["/Delete", "/F", "/TN", &task_name()])?)?;
        }
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN)?;
        if mode != Mode::RunKey {
            match key.delete_value(NAME) {
                Err(error) if error.kind() != ErrorKind::NotFound => return Err(error),
                _ => {}
            }
        }
        match mode {
            Mode::RunKey => key.set_value(NAME, &format!("\"{}\"", std::env::current_exe()?.display())),
            Mode::Task => create_task(),
            Mode::Off => Ok(()),
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Mode;

    pub fn current() -> Mode {
        Mode::Off
    }

    pub fn set(_mode: Mode) -> std::io::Result<()> {
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "autostart is Windows-only"))
    }

    pub fn task_runs_this() -> bool {
        false
    }

    pub fn run_task() -> bool {
        false
    }
}

pub use imp::{current, run_task, set, task_runs_this};
