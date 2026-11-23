//! Detection and verification utilities for Microsoft Edge WebView2 Runtime.
//!
//! On Windows, Dioxus Desktop relies on WebView2 (via Wry and Tao) to render the GUI.
//! This module provides runtime verification to ensure WebView2 is present, meets minimum
//! version criteria, and supplies diagnostic guidance if the runtime is missing.

use serde::{Deserialize, Serialize};

pub const WEBVIEW2_DOWNLOAD_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";
pub const MINIMUM_SUPPORTED_WEBVIEW2_MAJOR: u32 = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebView2RuntimeStatus {
    pub detected: bool,
    pub version: Option<String>,
    pub meets_minimum: bool,
    pub install_type: String,
    pub source: String,
    pub download_url: &'static str,
    pub advisory: String,
}

impl Default for WebView2RuntimeStatus {
    fn default() -> Self {
        Self {
            detected: false,
            version: None,
            meets_minimum: false,
            install_type: "Unknown".to_owned(),
            source: "Unchecked".to_owned(),
            download_url: WEBVIEW2_DOWNLOAD_URL,
            advisory: String::new(),
        }
    }
}

/// Parses the output of `reg query ... /v pv` to extract the version string.
pub fn parse_reg_pv_output(output: &str) -> Option<String> {
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("pv") {
            // Lines typically look like: "pv    REG_SZ    157.0.4341.60"
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 3 && parts[1].eq_ignore_ascii_case("REG_SZ") {
                return Some(parts[2].trim().to_owned());
            }
        }
    }
    None
}

/// Checks whether a semantic or quad-part version meets the minimum major version.
pub fn version_meets_minimum(version: &str, min_major: u32) -> bool {
    let mut parts = version.split('.');
    if let Some(first) = parts.next()
        && let Ok(major) = first.trim().parse::<u32>()
    {
        return major >= min_major;
    }
    false
}

/// Detects the installed Microsoft Edge WebView2 Runtime on the current host.
#[cfg(target_os = "windows")]
pub fn detect_webview2_runtime() -> WebView2RuntimeStatus {
    use std::process::Command;

    // Registry paths for WebView2 Evergreen Runtime:
    // 1. HKLM 64-bit / WOW6432Node
    // 2. HKLM 32-bit / native
    // 3. HKCU (per-user install)
    let candidate_keys = [
        (
            r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
            "Machine (WOW6432Node)",
        ),
        (
            r"HKLM\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
            "Machine (Native)",
        ),
        (
            r"HKCU\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
            "Current User",
        ),
    ];

    for (key, label) in candidate_keys {
        let output = Command::new("reg")
            .args(["query", key, "/v", "pv"])
            .output();

        if let Ok(output) = output
            && output.status.success()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(version) = parse_reg_pv_output(&stdout) {
                let meets_minimum =
                    version_meets_minimum(&version, MINIMUM_SUPPORTED_WEBVIEW2_MAJOR);
                let advisory = if meets_minimum {
                    format!("WebView2 Evergreen Runtime {version} is active and supported.")
                } else {
                    format!(
                        "WebView2 Runtime version {version} is below the recommended minimum {MINIMUM_SUPPORTED_WEBVIEW2_MAJOR}. Please update."
                    )
                };
                return WebView2RuntimeStatus {
                    detected: true,
                    version: Some(version),
                    meets_minimum,
                    install_type: "Evergreen".to_owned(),
                    source: format!("Windows Registry ({label})"),
                    download_url: WEBVIEW2_DOWNLOAD_URL,
                    advisory,
                };
            }
        }
    }

    WebView2RuntimeStatus {
        detected: false,
        version: None,
        meets_minimum: false,
        install_type: "Not Found".to_owned(),
        source: "Windows Registry".to_owned(),
        download_url: WEBVIEW2_DOWNLOAD_URL,
        advisory: "Microsoft Edge WebView2 Runtime is not installed. The desktop app requires WebView2 Evergreen Bootstrapper."
            .to_owned(),
    }
}

/// Non-Windows platforms do not require Microsoft Edge WebView2.
#[cfg(not(target_os = "windows"))]
pub fn detect_webview2_runtime() -> WebView2RuntimeStatus {
    WebView2RuntimeStatus {
        detected: true,
        version: Some("N/A".to_owned()),
        meets_minimum: true,
        install_type: "Platform Native (WebKitGTK/Cocoa)".to_owned(),
        source: "Host OS".to_owned(),
        download_url: WEBVIEW2_DOWNLOAD_URL,
        advisory: "Host platform uses native system web view components.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_reg_query_output() {
        let sample = "\
HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}
    pv    REG_SZ    157.0.4341.60

";
        let parsed = parse_reg_pv_output(sample);
        assert_eq!(parsed, Some("157.0.4341.60".to_owned()));
    }

    #[test]
    fn parses_missing_or_error_reg_output() {
        let sample = "ERROR: The system was unable to find the specified registry key or value.";
        let parsed = parse_reg_pv_output(sample);
        assert_eq!(parsed, None);
    }

    #[test]
    fn verifies_minimum_major_version() {
        assert!(version_meets_minimum("157.0.4341.60", 100));
        assert!(version_meets_minimum("100.0.0.0", 100));
        assert!(!version_meets_minimum("99.0.1234.5", 100));
        assert!(!version_meets_minimum("invalid", 100));
    }

    #[test]
    fn detects_runtime_on_current_host() {
        let status = detect_webview2_runtime();
        // Since we are running tests on the Windows host which has WebView2 installed:
        #[cfg(target_os = "windows")]
        {
            assert!(status.detected);
            assert!(status.meets_minimum);
            assert!(status.version.is_some());
        }
    }
}
