use std::{
    collections::HashSet,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use sysinfo::{Pid, Process, ProcessesToUpdate, System};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryProcessSample {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub role: String,
    pub name: String,
    pub working_set_bytes: u64,
    pub private_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryProcessTreeSample {
    pub captured_at_ms: u64,
    pub root_pid: u32,
    pub total_working_set_bytes: u64,
    pub total_private_bytes: Option<u64>,
    pub processes: Vec<MemoryProcessSample>,
}

pub fn sample_current_process_tree() -> Result<MemoryProcessTreeSample, String> {
    let root_pid = std::process::id();
    let root = Pid::from_u32(root_pid);
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    if system.process(root).is_none() {
        return Err("Mnemora root process was not found during sampling.".to_string());
    }

    let mut included = HashSet::from([root]);
    loop {
        let before = included.len();
        for (pid, process) in system.processes() {
            if process
                .parent()
                .is_some_and(|parent| included.contains(&parent))
            {
                included.insert(*pid);
            }
        }
        if included.len() == before {
            break;
        }
    }

    let mut processes = included
        .into_iter()
        .filter_map(|pid| {
            system
                .process(pid)
                .map(|process| process_sample(pid, root, process))
        })
        .collect::<Vec<_>>();
    processes.sort_by_key(|process| process.pid);
    let total_working_set_bytes = processes
        .iter()
        .map(|process| process.working_set_bytes)
        .sum();
    let private_values = processes
        .iter()
        .map(|process| process.private_bytes)
        .collect::<Option<Vec<_>>>();

    Ok(MemoryProcessTreeSample {
        captured_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        root_pid,
        total_working_set_bytes,
        total_private_bytes: private_values.map(|values| values.into_iter().sum()),
        processes,
    })
}

fn process_sample(pid: Pid, root: Pid, process: &Process) -> MemoryProcessSample {
    let command_line = process
        .cmd()
        .iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let name = process.name().to_string_lossy().into_owned();
    MemoryProcessSample {
        pid: pid.as_u32(),
        parent_pid: process.parent().map(Pid::as_u32),
        role: classify_process(pid == root, &name, &command_line),
        name,
        working_set_bytes: process.memory(),
        private_bytes: private_bytes(pid.as_u32()),
    }
}

fn classify_process(is_root: bool, name: &str, command_line: &str) -> String {
    if is_root {
        return "mnemora".to_string();
    }
    let normalized_name = name.to_ascii_lowercase();
    let normalized_command = command_line.to_ascii_lowercase();
    if normalized_name.contains("crashpad") {
        return "crashpad".to_string();
    }
    if normalized_name.contains("msedgewebview2") {
        if normalized_command.contains("--type=renderer") {
            return "webview-renderer".to_string();
        }
        if normalized_command.contains("--type=gpu-process") {
            return "webview-gpu".to_string();
        }
        if normalized_command.contains("network.mojom.networkservice") {
            return "webview-network".to_string();
        }
        if normalized_command.contains("audio.mojom.audioservice") {
            return "webview-audio".to_string();
        }
        if normalized_command.contains("storage.mojom.storageservice") {
            return "webview-storage".to_string();
        }
        if normalized_command.contains("--type=utility") {
            return "webview-utility".to_string();
        }
        return "webview-browser".to_string();
    }
    "other".to_string()
}

#[cfg(target_os = "windows")]
fn private_bytes(pid: u32) -> Option<u64> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            ProcessStatus::{
                GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
            },
            Threading::{
                OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_VM_READ,
            },
        },
    };

    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
            0,
            pid,
        );
        if handle.is_null() {
            return None;
        }
        let mut counters: PROCESS_MEMORY_COUNTERS_EX = zeroed();
        let result = GetProcessMemoryInfo(
            handle,
            &mut counters as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS,
            size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        );
        CloseHandle(handle);
        (result != 0).then_some(counters.PrivateUsage as u64)
    }
}

#[cfg(not(target_os = "windows"))]
fn private_bytes(_pid: u32) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::classify_process;

    #[test]
    fn classifies_webview_roles_from_command_line() {
        assert_eq!(
            classify_process(false, "msedgewebview2.exe", "--type=renderer"),
            "webview-renderer"
        );
        assert_eq!(
            classify_process(false, "msedgewebview2.exe", "--type=gpu-process"),
            "webview-gpu"
        );
        assert_eq!(
            classify_process(
                false,
                "msedgewebview2.exe",
                "--type=utility --utility-sub-type=network.mojom.NetworkService"
            ),
            "webview-network"
        );
    }
}
