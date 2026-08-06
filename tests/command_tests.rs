use ferrite::tools::command::check_dangerous_command;

#[test]
fn blocks_recursive_rm() {
    let msg = check_dangerous_command("rm -rf /tmp/data").expect("should be blocked");
    assert!(msg.contains("安全性攔截"));
    assert!(msg.contains("rm -rf"));
}

#[test]
fn blocks_recursive_rm_variant() {
    let msg = check_dangerous_command("rm -r /tmp/data").expect("should be blocked");
    assert!(msg.contains("安全性攔截"));
}

#[test]
fn blocks_format_command() {
    let msg = check_dangerous_command("format c:").expect("should be blocked");
    assert!(msg.contains("安全性攔截"));
    assert!(msg.contains("format"));
}

#[test]
fn blocks_mkfs_command() {
    let msg = check_dangerous_command("mkfs.ext4 /dev/sda1").expect("should be blocked");
    assert!(msg.contains("安全性攔截"));
}

#[test]
fn blocks_dd_disk_write() {
    let msg = check_dangerous_command("dd if=/dev/zero of=/dev/sda").expect("should be blocked");
    assert!(msg.contains("安全性攔截"));
}

#[test]
fn blocks_windows_forced_delete() {
    let msg = check_dangerous_command("del /f /q file.txt").expect("should be blocked");
    assert!(msg.contains("安全性攔截"));
    assert!(msg.contains("del /f"));
}

#[test]
fn blocks_windows_recursive_rd() {
    let msg = check_dangerous_command("rd /s /q C:\\temp").expect("should be blocked");
    assert!(msg.contains("安全性攔截"));
}

#[test]
fn allows_safe_commands() {
    assert!(check_dangerous_command("echo hello").is_none());
    assert!(check_dangerous_command("cargo build").is_none());
    assert!(check_dangerous_command("git status").is_none());
    assert!(check_dangerous_command("npm test").is_none());
}

#[test]
fn detecting_is_case_insensitive() {
    let msg = check_dangerous_command("RM -RF /").expect("should be blocked");
    assert!(msg.contains("安全性攔截"));
}