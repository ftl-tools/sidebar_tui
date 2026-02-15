//! Cross-platform environment variable capture from running processes.
//!
//! This module provides functions to capture environment variables from a running
//! shell process, used for persisting session state across daemon restarts.

use std::collections::HashMap;

/// List of environment variable prefixes/names to filter out when saving.
/// These may contain sensitive information or are session-specific.
const SENSITIVE_ENV_PREFIXES: &[&str] = &[
    "SSH_",           // SSH agent/connection info
    "GPG_",           // GPG agent info
    "AWS_",           // AWS credentials
    "AZURE_",         // Azure credentials
    "GCP_",           // GCP credentials
    "GOOGLE_",        // Google credentials
    "API_KEY",        // Generic API keys
    "SECRET",         // Generic secrets
    "TOKEN",          // Generic tokens
    "PASSWORD",       // Passwords
    "CREDENTIAL",     // Credentials
    "PRIVATE_KEY",    // Private keys
    "ANTHROPIC_",     // Anthropic API keys
    "OPENAI_",        // OpenAI API keys
];

/// Environment variables to always exclude (exact match).
const EXCLUDED_ENV_VARS: &[&str] = &[
    "TERM_SESSION_ID",     // macOS terminal session
    "WINDOWID",            // X11 window ID
    "DISPLAY",             // X11 display (may not be valid after reboot)
    "XDG_SESSION_ID",      // Session-specific
    "XDG_RUNTIME_DIR",     // Session-specific runtime dir
    "DBUS_SESSION_BUS_ADDRESS", // D-Bus session
    "SSH_AUTH_SOCK",       // SSH agent socket
    "SSH_AGENT_PID",       // SSH agent PID
    "GPG_AGENT_INFO",      // GPG agent info
    "SECURITYSESSIONID",   // macOS security session
    "Apple_PubSub_Socket_Render", // macOS-specific
    "_",                   // Last command (set by shell)
    "OLDPWD",              // Previous directory (shell-managed)
    "SHLVL",               // Shell level (session-specific)
];

/// Capture environment variables from a process by PID.
/// Returns None if the process doesn't exist or can't be read.
pub fn capture_environment(pid: u32) -> Option<HashMap<String, String>> {
    #[cfg(target_os = "linux")]
    {
        capture_environment_linux(pid)
    }

    #[cfg(target_os = "macos")]
    {
        capture_environment_macos(pid)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        // Unsupported platform - return empty
        let _ = pid;
        None
    }
}

/// Capture environment variables on Linux by reading /proc/{pid}/environ.
#[cfg(target_os = "linux")]
fn capture_environment_linux(pid: u32) -> Option<HashMap<String, String>> {
    use std::fs;

    let environ_path = format!("/proc/{}/environ", pid);
    let data = fs::read(&environ_path).ok()?;

    let mut env = HashMap::new();
    // /proc/pid/environ uses null bytes as separators
    for entry in data.split(|&b| b == 0) {
        if entry.is_empty() {
            continue;
        }
        if let Ok(s) = std::str::from_utf8(entry) {
            if let Some((key, value)) = s.split_once('=') {
                env.insert(key.to_string(), value.to_string());
            }
        }
    }

    Some(filter_sensitive_vars(env))
}

/// Capture environment variables on macOS using ps eww.
#[cfg(target_os = "macos")]
fn capture_environment_macos(pid: u32) -> Option<HashMap<String, String>> {
    use std::process::Command;

    // ps eww -p <pid> -o command= gives us the command line with environment
    // The format is: command args... VAR1=value1 VAR2=value2 ...
    let output = Command::new("ps")
        .args(["eww", "-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut env = HashMap::new();

    // Parse the output - env vars appear after the command and its arguments
    // They're separated by spaces and look like VAR=value
    // We need to be careful because values might contain spaces if quoted
    for part in stdout.split_whitespace() {
        if let Some((key, value)) = part.split_once('=') {
            // Only include if the key looks like an env var name
            // (all uppercase or contains underscore, starts with letter)
            if is_likely_env_var_name(key) {
                env.insert(key.to_string(), value.to_string());
            }
        }
    }

    Some(filter_sensitive_vars(env))
}

/// Check if a string looks like an environment variable name.
fn is_likely_env_var_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let first = s.chars().next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Filter out sensitive environment variables.
fn filter_sensitive_vars(env: HashMap<String, String>) -> HashMap<String, String> {
    env.into_iter()
        .filter(|(key, _)| {
            // Check exact exclusions
            if EXCLUDED_ENV_VARS.contains(&key.as_str()) {
                return false;
            }
            // Check prefix exclusions (case-insensitive)
            let upper_key = key.to_uppercase();
            for prefix in SENSITIVE_ENV_PREFIXES {
                if upper_key.starts_with(prefix) || upper_key.contains(prefix) {
                    return false;
                }
            }
            true
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_sensitive_vars() {
        let mut env = HashMap::new();
        env.insert("PATH".to_string(), "/usr/bin".to_string());
        env.insert("HOME".to_string(), "/home/user".to_string());
        env.insert("SSH_AUTH_SOCK".to_string(), "/tmp/ssh-xxx".to_string());
        env.insert("AWS_SECRET_KEY".to_string(), "secret123".to_string());
        env.insert("MY_API_KEY".to_string(), "key123".to_string());
        env.insert("NORMAL_VAR".to_string(), "value".to_string());

        let filtered = filter_sensitive_vars(env);

        assert!(filtered.contains_key("PATH"));
        assert!(filtered.contains_key("HOME"));
        assert!(filtered.contains_key("NORMAL_VAR"));
        assert!(!filtered.contains_key("SSH_AUTH_SOCK"));
        assert!(!filtered.contains_key("AWS_SECRET_KEY"));
        assert!(!filtered.contains_key("MY_API_KEY"));
    }

    #[test]
    fn test_is_likely_env_var_name() {
        assert!(is_likely_env_var_name("PATH"));
        assert!(is_likely_env_var_name("HOME"));
        assert!(is_likely_env_var_name("MY_VAR_123"));
        assert!(is_likely_env_var_name("_PRIVATE"));
        assert!(!is_likely_env_var_name("123VAR"));
        assert!(!is_likely_env_var_name(""));
        assert!(!is_likely_env_var_name("my-var"));
    }

    #[test]
    fn test_capture_current_process() {
        // Test capturing our own process's environment
        let pid = std::process::id();
        let env = capture_environment(pid);

        // We should be able to capture at least some environment
        if let Some(env) = env {
            // PATH should exist in most environments
            assert!(env.contains_key("PATH") || env.contains_key("HOME"),
                "Should capture at least PATH or HOME");
        }
        // It's OK if capture returns None on unsupported platforms
    }
}
