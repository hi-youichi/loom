use std::path::{Path, PathBuf};

pub struct ShellOutput {
    pub stdout: String,
    pub stderr: String,
    pub pid: Option<u32>,
    pub timed_out: bool,
    pub stdout_file: Option<PathBuf>,
    pub stderr_file: Option<PathBuf>,
}

pub fn format_shell_output(output: &ShellOutput) -> String {
    if output.timed_out {
        format_timed_out_output(output)
    } else if output.stderr.is_empty() {
        output.stdout.clone()
    } else if output.stdout.is_empty() {
        format!("stderr:\n{}", output.stderr)
    } else {
        format!("stdout:\n{}\nstderr:\n{}", output.stdout, output.stderr)
    }
}

pub fn format_timed_out_output(output: &ShellOutput) -> String {
    let pid = output.pid.unwrap_or(0);
    let stdout_size = format_size(output.stdout.len());
    let mut text = format!("Command timed out (PID: {})\n", pid);
    if !output.stdout.is_empty() {
        text.push_str(&format!("\n--- Partial Output ({}) ---\n", stdout_size));
        text.push_str(&output.stdout);
    }
    if !output.stderr.is_empty() {
        let stderr_size = format_size(output.stderr.len());
        text.push_str(&format!("\n--- Partial Stderr ({}) ---\n", stderr_size));
        text.push_str(&output.stderr);
    }
    if let Some(stdout_file) = &output.stdout_file {
        text.push_str("\n--- Output Files ---\n");
        text.push_str(&format!("stdout: {}\n", stdout_file.display()));
    }
    if let Some(stderr_file) = &output.stderr_file {
        text.push_str(&format!("stderr: {}\n", stderr_file.display()));
    }
    if let Some(stdout_file) = &output.stdout_file {
        text.push_str(&format!(
            "\nUse `cat {}` to read the latest output.\nUse `kill {}` to stop the process.",
            stdout_file.display(),
            pid
        ));
    }
    text
}

pub fn format_terminal_timed_out_output(terminal_id: &str, output: &str) -> String {
    let output_size = format_size(output.len());
    let mut text = format!("Command timed out (terminal: {})\n", terminal_id);
    if !output.is_empty() {
        text.push_str(&format!(
            "\n--- Partial Output ({}) ---\n{}",
            output_size, output
        ));
    }
    text.push_str(&format!(
        "\nUse terminal operations to check on this process (terminal ID: {}).",
        terminal_id
    ));
    text
}

pub fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub fn shell_output_dir(base_dir: &Path) -> PathBuf {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    base_dir.join(".anureo").join("shell").join(today)
}

pub fn create_output_file(path: &Path) -> std::io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
}

pub fn generate_run_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:08x}", (nanos as u32).wrapping_add(std::process::id()))
}

pub fn make_relative(path: &Path, base_dir: &Path) -> PathBuf {
    path.strip_prefix(base_dir).unwrap_or(path).to_path_buf()
}
