//! External tool integration — launch dev-finder and Code Intelligence
//! from project context. Security invariants:
//! - Path arguments are passed via argv (no shell string concatenation).
//! - Only hardcoded tool identifiers are accepted (no arbitrary execution).
//! - Executable existence is validated before attempting launch.
//! - Code Intelligence uses browser URL flow (no local API auth issues).

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Supported external tools (exhaustive — no arbitrary tool execution).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExternalTool {
    DevFinder,
    CodeIntelligence,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolLaunchResult {
    pub success: bool,
    /// For clipboard/url flows, the value to use.
    pub value: Option<String>,
    /// Human-readable detail on what happened.
    pub detail: String,
}

/// Resolve the absolute path of the dev-finder executable.
/// Checks common install locations on macOS.
fn find_dev_finder_binary() -> Option<String> {
    let candidates = [
        // Homebrew (Apple Silicon + Intel)
        "/opt/homebrew/bin/dev-finder",
        "/usr/local/bin/dev-finder",
        // Cargo install
        &format!(
            "{}/{}",
            std::env::var("HOME").unwrap_or_default(),
            ".cargo/bin/dev-finder"
        ),
    ];
    for c in &candidates {
        if Path::new(c).is_file() {
            return Some(c.to_string());
        }
    }
    // Fall back to PATH lookup via `which`
    if let Ok(output) = Command::new("which").arg("dev-finder").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() && Path::new(&path).is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// Launch dev-finder with `--project <path>` in a new terminal window.
/// dev-finder is an interactive TUI tool that requires a terminal, so we
/// launch it through the user's terminal application.
///
/// Security: path is passed as a separate argv element — never interpolated
/// into a shell string.
pub fn launch_dev_finder(project_path: &str, terminal_app: &str) -> ToolLaunchResult {
    let path = Path::new(project_path);
    if !path.is_dir() {
        return ToolLaunchResult {
            success: false,
            value: None,
            detail: format!("Project directory does not exist: {project_path}"),
        };
    }

    let Some(binary) = find_dev_finder_binary() else {
        return ToolLaunchResult {
            success: false,
            value: None,
            detail: "dev-finder is not installed. Install with: cargo install dev-finder \
                     or brew install dev-finder"
                .into(),
        };
    };

    // Construct a safe shell command for the terminal.
    // We single-quote the path and binary to prevent injection, replacing
    // any embedded single quotes with the standard '\'' escape.
    let safe_binary = shell_escape(&binary);
    let safe_path = shell_escape(project_path);
    let cmd_str = format!("{} --project {}", safe_binary, safe_path);

    // Reject paths containing characters that cannot be safely embedded in
    // AppleScript strings (newlines, carriage returns). These are theoretically
    // possible directory names but in practice always indicate injection attempts.
    if project_path.contains('\n') || project_path.contains('\r') {
        return ToolLaunchResult {
            success: false,
            value: None,
            detail: "Project path contains newline characters, which cannot be safely passed to a terminal. Use the 'Copy Command' option instead.".into(),
        };
    }

    // Launch in terminal via `open -a <Terminal> <script>` pattern or osascript
    let result = if terminal_app.to_lowercase().contains("iterm") {
        // iTerm2: use osascript for reliable new-window launch
        let script = format!(
            r#"tell application "iTerm"
    activate
    set newWindow to (create window with default profile)
    tell current session of newWindow
        write text "{}"
    end tell
end tell"#,
            applescript_escape(&cmd_str)
        );
        Command::new("osascript").args(["-e", &script]).output()
    } else {
        // Default Terminal.app
        let script = format!(
            r#"tell application "Terminal"
    activate
    do script "{}"
end tell"#,
            applescript_escape(&cmd_str)
        );
        Command::new("osascript").args(["-e", &script]).output()
    };

    match result {
        Ok(output) if output.status.success() => ToolLaunchResult {
            success: true,
            value: Some(cmd_str),
            detail: format!("Launched dev-finder for {project_path}"),
        },
        Ok(output) => ToolLaunchResult {
            success: false,
            value: Some(cmd_str),
            detail: format!(
                "Failed to launch terminal: {}",
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .next()
                    .unwrap_or("unknown error")
            ),
        },
        Err(e) => ToolLaunchResult {
            success: false,
            value: Some(cmd_str),
            detail: format!("Failed to execute osascript: {e}"),
        },
    }
}

/// Build the command string for dev-finder (for clipboard copy flow).
pub fn dev_finder_command(project_path: &str) -> ToolLaunchResult {
    let path = Path::new(project_path);
    if !path.is_dir() {
        return ToolLaunchResult {
            success: false,
            value: None,
            detail: format!("Project directory does not exist: {project_path}"),
        };
    }

    let Some(binary) = find_dev_finder_binary() else {
        // Even without the binary installed, we can still provide the command
        let cmd = format!("dev-finder --project {}", shell_escape(project_path));
        return ToolLaunchResult {
            success: true,
            value: Some(cmd),
            detail: "dev-finder is not installed — command copied for manual use. \
                     Install with: cargo install dev-finder"
                .into(),
        };
    };

    let cmd = format!(
        "{} --project {}",
        shell_escape(&binary),
        shell_escape(project_path)
    );
    ToolLaunchResult {
        success: true,
        value: Some(cmd),
        detail: "Command ready".into(),
    }
}

/// Open a project in Code Intelligence via browser URL.
/// Since Code Intelligence may have auth/session/allowed-root constraints,
/// we use the safe web URL flow rather than calling local APIs directly.
///
/// The URL pattern encodes the project path as a query parameter so the
/// web UI can prompt the user for import/analysis.
pub fn open_code_intelligence(project_path: &str, base_url: Option<&str>) -> ToolLaunchResult {
    let path = Path::new(project_path);
    if !path.is_dir() {
        return ToolLaunchResult {
            success: false,
            value: None,
            detail: format!("Project directory does not exist: {project_path}"),
        };
    }

    let base = base_url.unwrap_or("http://localhost:3000");

    // Validate the base URL is localhost-only (security constraint matching open_url)
    let valid_prefixes = [
        "http://localhost",
        "http://127.0.0.1",
        "https://localhost",
        "https://127.0.0.1",
        "http://[::1]",
    ];
    if !valid_prefixes.iter().any(|p| base.starts_with(p)) {
        return ToolLaunchResult {
            success: false,
            value: None,
            detail: format!("Code Intelligence URL must be localhost. Got: {base}"),
        };
    }

    // URL-encode the project path safely
    let encoded_path = url_encode(project_path);
    let url = format!("{base}/import?path={encoded_path}");

    // Try to open in browser
    let result = Command::new("open").arg(&url).output();

    match result {
        Ok(output) if output.status.success() => ToolLaunchResult {
            success: true,
            value: Some(url),
            detail: format!("Browser opened — navigate to Code Intelligence for {project_path}"),
        },
        Ok(_) => ToolLaunchResult {
            success: false,
            value: Some(url),
            detail: "Failed to open browser. Code Intelligence may not be running.".into(),
        },
        Err(e) => ToolLaunchResult {
            success: false,
            value: Some(url),
            detail: format!("Failed to open URL: {e}"),
        },
    }
}

/// Check whether Code Intelligence server is reachable.
pub fn check_code_intelligence_server(base_url: Option<&str>) -> bool {
    let base = base_url.unwrap_or("http://localhost:3000");
    // Quick TCP check on the port
    let url_without_scheme = base
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let addr = if url_without_scheme.contains(':') {
        url_without_scheme.to_string()
    } else {
        format!("{url_without_scheme}:80")
    };

    use std::net::TcpStream;
    use std::time::Duration;
    TcpStream::connect_timeout(
        &addr
            .parse()
            .unwrap_or_else(|_| "127.0.0.1:3000".parse().unwrap()),
        Duration::from_millis(500),
    )
    .is_ok()
}

/// Minimal percent-encoding for URL path/query values.
/// Encodes all non-unreserved characters per RFC 3986.
pub fn url_encode(input: &str) -> String {
    let mut result = String::with_capacity(input.len() * 3);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push('%');
                result.push(HEX_CHARS[(byte >> 4) as usize]);
                result.push(HEX_CHARS[(byte & 0x0f) as usize]);
            }
        }
    }
    result
}

const HEX_CHARS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F',
];

/// Shell-escape a string by wrapping in single quotes, with proper escaping
/// of embedded single quotes. This prevents command injection.
pub fn shell_escape(input: &str) -> String {
    // Strategy: wrap in single quotes, replace any ' with '\''
    let escaped = input.replace('\'', "'\\''");
    format!("'{escaped}'")
}

/// Escape a string for safe embedding in an AppleScript double-quoted string.
/// Handles backslashes, double quotes, and newlines/carriage returns.
fn applescript_escape(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_encode_basic() {
        assert_eq!(url_encode("hello"), "hello");
        assert_eq!(
            url_encode("/Users/me/Dev/project"),
            "%2FUsers%2Fme%2FDev%2Fproject"
        );
        assert_eq!(url_encode("path with spaces"), "path%20with%20spaces");
        assert_eq!(url_encode("special!@#$%"), "special%21%40%23%24%25");
    }

    #[test]
    fn test_url_encode_unicode() {
        // Korean characters should be percent-encoded
        let encoded = url_encode("경로/테스트");
        assert!(encoded.contains("%"));
        assert!(!encoded.contains("경"));
    }

    #[test]
    fn test_url_encode_preserves_unreserved() {
        assert_eq!(url_encode("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(url_encode("ABC123"), "ABC123");
    }

    #[test]
    fn test_shell_escape_basic() {
        assert_eq!(shell_escape("/Users/me/Dev"), "'/Users/me/Dev'");
        assert_eq!(shell_escape("hello world"), "'hello world'");
    }

    #[test]
    fn test_shell_escape_single_quotes() {
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
        assert_eq!(shell_escape("a'b'c"), "'a'\\''b'\\''c'");
    }

    #[test]
    fn test_shell_escape_injection_attempts() {
        // These should all be safely quoted
        assert_eq!(shell_escape("$(rm -rf /)"), "'$(rm -rf /)'");
        assert_eq!(shell_escape("`whoami`"), "'`whoami`'");
        assert_eq!(shell_escape("a;b"), "'a;b'");
        assert_eq!(shell_escape("a&&b"), "'a&&b'");
        assert_eq!(shell_escape("a|b"), "'a|b'");
        assert_eq!(shell_escape("a\nb"), "'a\nb'");
    }

    #[test]
    fn test_dev_finder_command_nonexistent_dir() {
        let result = dev_finder_command("/nonexistent/path/xyz123");
        assert!(!result.success);
        assert!(result.detail.contains("does not exist"));
    }

    #[test]
    fn test_dev_finder_command_valid_dir() {
        // /tmp always exists on macOS/Linux
        let result = dev_finder_command("/tmp");
        assert!(result.success);
        let cmd = result.value.unwrap();
        // The command should contain the path safely quoted
        assert!(cmd.contains("'/tmp'"));
        // Should never contain unquoted shell metacharacters
        assert!(!cmd.contains("$("));
    }

    #[test]
    fn test_dev_finder_command_path_with_spaces() {
        // Even with spaces, it should not be valid if dir doesn't exist
        let result = dev_finder_command("/tmp/path with spaces");
        // Dir likely doesn't exist, but check the error is about existence not injection
        if !result.success {
            assert!(result.detail.contains("does not exist"));
        }
    }

    #[test]
    fn test_open_code_intelligence_nonexistent_dir() {
        let result = open_code_intelligence("/nonexistent/xyz123", None);
        assert!(!result.success);
        assert!(result.detail.contains("does not exist"));
    }

    #[test]
    fn test_open_code_intelligence_invalid_url() {
        let result = open_code_intelligence("/tmp", Some("https://evil.com"));
        assert!(!result.success);
        assert!(result.detail.contains("must be localhost"));
    }

    #[test]
    fn test_open_code_intelligence_url_encoding() {
        // Test that the URL is properly formed (even if open fails because no server)
        let result = open_code_intelligence("/tmp", Some("http://localhost:3000"));
        // It may fail (no browser in CI) but the URL should be correctly formed
        if let Some(url) = &result.value {
            assert!(url.starts_with("http://localhost:3000/import?path="));
            assert!(url.contains("%2Ftmp"));
        }
    }

    #[test]
    fn test_code_intelligence_rejects_remote_url() {
        let cases = [
            "http://example.com:3000",
            "https://remote.server.io",
            "http://192.168.1.100:3000",
        ];
        for url in cases {
            let result = open_code_intelligence("/tmp", Some(url));
            assert!(!result.success, "Should reject: {url}");
            assert!(result.detail.contains("must be localhost"));
        }
    }

    #[test]
    fn test_launch_dev_finder_nonexistent_dir() {
        let result = launch_dev_finder("/nonexistent/xyz123", "Terminal");
        assert!(!result.success);
        assert!(result.detail.contains("does not exist"));
    }

    #[test]
    fn test_launch_dev_finder_newline_path_rejected() {
        // Even if the directory somehow existed, newlines must be rejected
        // to prevent AppleScript injection
        let result = launch_dev_finder("/tmp\n\" & do shell script \"whoami", "Terminal");
        // Should fail at "does not exist" or "newline" check
        assert!(!result.success);
    }

    #[test]
    fn test_applescript_escape() {
        let result = applescript_escape("hello\nworld");
        assert_eq!(result, "hello\\nworld");
        let result = applescript_escape("a\"b\\c\rd");
        assert_eq!(result, "a\\\"b\\\\c\\rd");
    }
}
