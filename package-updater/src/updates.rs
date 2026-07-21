use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::process::Command;
use tokio::process::Command as TokioCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AurHelper {
    Paru,
    Yay,
}

impl AurHelper {
    pub fn name(&self) -> &'static str {
        match self {
            AurHelper::Paru => "paru",
            AurHelper::Yay => "yay",
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemTools {
    pub checkupdates: bool,
    pub aur_helper: Option<AurHelper>,
    pub flatpak: bool,
}

impl SystemTools {
    pub fn detect() -> Self {
        let aur_helper = if Self::is_available("paru") {
            Some(AurHelper::Paru)
        } else if Self::is_available("yay") {
            Some(AurHelper::Yay)
        } else {
            None
        };

        Self {
            checkupdates: Self::is_available("checkupdates"),
            aur_helper,
            flatpak: Self::is_available("flatpak"),
        }
    }

    fn is_available(binary: &str) -> bool {
        Command::new("which")
            .arg(binary)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone)]
pub struct PackageUpdate {
    pub name: String,
    pub current_version: String,
    pub new_version: String,
}

#[derive(Debug, Clone, Default)]
pub enum SourceState {
    /// Tool not installed or source disabled in settings; section is hidden.
    #[default]
    Disabled,
    Checked(Vec<PackageUpdate>),
    Error(String),
}

impl SourceState {
    pub fn count(&self) -> usize {
        match self {
            SourceState::Checked(packages) => packages.len(),
            _ => 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct UpdateReport {
    pub pacman: SourceState,
    pub aur: SourceState,
    pub flatpak: SourceState,
}

impl UpdateReport {
    pub fn total(&self) -> usize {
        self.pacman.count() + self.aur.count() + self.flatpak.count()
    }

    pub fn has_updates(&self) -> bool {
        self.total() > 0
    }

    pub fn all_failed(&self) -> bool {
        let states = [&self.pacman, &self.aur, &self.flatpak];
        states.iter().any(|s| matches!(s, SourceState::Error(_)))
            && !states.iter().any(|s| matches!(s, SourceState::Checked(_)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateTarget {
    Pacman,
    Aur,
    Flatpak,
    All,
}

impl UpdateTarget {
    pub fn command(&self, tools: &SystemTools) -> Option<String> {
        match self {
            UpdateTarget::Pacman => Some("sudo pacman -Syu".to_string()),
            UpdateTarget::Aur => tools
                .aur_helper
                .map(|helper| format!("{} -Sua", helper.name())),
            UpdateTarget::Flatpak => tools.flatpak.then(|| "flatpak update".to_string()),
            UpdateTarget::All => {
                let system = match tools.aur_helper {
                    Some(helper) => format!("{} -Syu", helper.name()),
                    None => "sudo pacman -Syu".to_string(),
                };
                // `;` so a declined pacman transaction still lets flatpak run
                Some(if tools.flatpak {
                    format!("{system}; flatpak update")
                } else {
                    system
                })
            }
        }
    }
}

fn get_lock_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime_dir).join("cosmic-package-updater.lock")
}

fn get_sync_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime_dir).join("cosmic-package-updater.sync")
}

fn notify_check_completed() {
    // Touch the sync file to notify other instances
    let sync_path = get_sync_path();
    if let Ok(mut file) = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&sync_path)
    {
        let _ = writeln!(
            file,
            "{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );
    }
}

fn acquire_lock() -> Result<File, String> {
    let lock_path = get_lock_path();

    match OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&lock_path)
    {
        Ok(mut file) => {
            let _ = writeln!(file, "{}", std::process::id());
            Ok(file)
        }
        Err(e) if e.kind() == ErrorKind::PermissionDenied => {
            Err("Another instance is checking for updates".to_string())
        }
        Err(e) => Err(format!("Failed to acquire lock: {e}")),
    }
}

/// Check all enabled sources. Pacman and AUR run sequentially (both touch the
/// pacman db state); flatpak is independent and runs concurrently.
pub async fn check_all(tools: SystemTools, check_aur: bool, check_flatpak: bool) -> UpdateReport {
    let _lock = match acquire_lock() {
        Ok(lock) => lock,
        Err(first_err) => {
            eprintln!("Could not acquire lock: {first_err}. Waiting and retrying...");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            match acquire_lock() {
                Ok(lock) => lock,
                Err(e) => {
                    let msg = format!("Update check already in progress: {e}");
                    return UpdateReport {
                        pacman: SourceState::Error(msg.clone()),
                        aur: SourceState::Error(msg.clone()),
                        flatpak: SourceState::Error(msg),
                    };
                }
            }
        }
    };

    let arch = async {
        let pacman = if tools.checkupdates {
            check_source_with_retry("checkupdates", &[], parse_arch_line).await
        } else {
            SourceState::Error("checkupdates not found — install pacman-contrib".to_string())
        };

        let aur = match (tools.aur_helper, check_aur) {
            (Some(helper), true) => {
                check_source_with_retry(helper.name(), &["-Qu", "--aur"], parse_arch_line).await
            }
            _ => SourceState::Disabled,
        };

        (pacman, aur)
    };

    let flatpak = async {
        if tools.flatpak && check_flatpak {
            check_source_with_retry("flatpak", &["remote-ls", "--updates"], parse_flatpak_line)
                .await
        } else {
            SourceState::Disabled
        }
    };

    let ((pacman, aur), flatpak) = tokio::join!(arch, flatpak);

    notify_check_completed();

    UpdateReport {
        pacman,
        aur,
        flatpak,
    }
}

async fn check_source_with_retry(
    cmd: &str,
    args: &[&str],
    parse: fn(&str) -> Option<PackageUpdate>,
) -> SourceState {
    match run_check(cmd, args, parse).await {
        Ok(packages) => SourceState::Checked(packages),
        Err(first_err) => {
            eprintln!("Update check via {cmd} failed: {first_err}, retrying...");
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            match run_check(cmd, args, parse).await {
                Ok(packages) => SourceState::Checked(packages),
                Err(e) => SourceState::Error(e),
            }
        }
    }
}

async fn run_check(
    cmd: &str,
    args: &[&str],
    parse: fn(&str) -> Option<PackageUpdate>,
) -> Result<Vec<PackageUpdate>, String> {
    let output = TokioCommand::new(cmd)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("failed to run {cmd}: {e}"))?;

    if !output.status.success() {
        let exit_code = output.status.code().unwrap_or(-1);

        // checkupdates returns 2 when no updates are available;
        // paru/yay return 1 when no updates are available
        if (cmd == "checkupdates" && exit_code == 2)
            || ((cmd == "paru" || cmd == "yay") && exit_code == 1)
        {
            return Ok(Vec::new());
        }

        // Any other exit code might still have valid output
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("Update check failed with exit code {exit_code}: {stderr}");
            return Err(format!(
                "Failed to check for updates (exit {exit_code}): {stderr}"
            ));
        }
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().filter_map(parse).collect())
}

/// Arch-based output: "package 1.0.0-1 -> 1.0.1-1" or "package 1.0.1-1"
fn parse_arch_line(line: &str) -> Option<PackageUpdate> {
    if line.trim().is_empty() {
        return None;
    }

    let parts: Vec<&str> = line.split_whitespace().collect();
    if line.contains(" -> ") {
        if parts.len() >= 4 && parts[2] == "->" {
            return Some(PackageUpdate {
                name: parts[0].to_string(),
                current_version: parts[1].to_string(),
                new_version: parts[3].to_string(),
            });
        }
    } else if parts.len() >= 2 {
        return Some(PackageUpdate {
            name: parts[0].to_string(),
            current_version: "unknown".to_string(),
            new_version: parts[1].to_string(),
        });
    }

    None
}

/// Flatpak output: "name\tapp-id\tversion\tbranch\tremote"
fn parse_flatpak_line(line: &str) -> Option<PackageUpdate> {
    if line.trim().is_empty() {
        return None;
    }

    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() >= 3 {
        return Some(PackageUpdate {
            name: parts[0].to_string(),
            current_version: "unknown".to_string(),
            new_version: parts[2].to_string(),
        });
    }

    None
}
