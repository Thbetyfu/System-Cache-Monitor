use std::process::Command;

/// Check if the current process is running with Administrator privileges on Windows.
pub fn is_admin() -> bool {
    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = Command::new("net").arg("session").output() {
            output.status.success()
        } else {
            false
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Set a Windows User Environment Variable.
/// This automatically persists to HKCU\Environment and broadcasts WM_SETTINGCHANGE.
pub fn set_user_env(name: &str, value: &str) -> std::io::Result<bool> {
    #[cfg(target_os = "windows")]
    {
        let cmd = format!(
            "[Environment]::SetEnvironmentVariable('{}', '{}', 'User')",
            name, value
        );
        let output = Command::new("powershell")
            .args(&["-NoProfile", "-Command", &cmd])
            .output()?;
        Ok(output.status.success())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (name, value);
        Ok(false)
    }
}

/// Remove a Windows User Environment Variable by setting it to null.
pub fn unset_user_env(name: &str) -> std::io::Result<bool> {
    #[cfg(target_os = "windows")]
    {
        let cmd = format!(
            "[Environment]::SetEnvironmentVariable('{}', $null, 'User')",
            name
        );
        let output = Command::new("powershell")
            .args(&["-NoProfile", "-Command", &cmd])
            .output()?;
        Ok(output.status.success())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = name;
        Ok(false)
    }
}

/// Get the value of a Windows User Environment Variable directly from Registry (HKCU).
pub fn get_user_env(name: &str) -> std::io::Result<Option<String>> {
    #[cfg(target_os = "windows")]
    {
        let cmd = format!(
            "[Environment]::GetEnvironmentVariable('{}', 'User')",
            name
        );
        let output = Command::new("powershell")
            .args(&["-NoProfile", "-Command", &cmd])
            .output()?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if stdout.is_empty() {
                Ok(None)
            } else {
                Ok(Some(stdout))
            }
        } else {
            Ok(None)
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = name;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_and_get_user_env() {
        #[cfg(target_os = "windows")]
        {
            let var_name = "CA_TEST_VAR_12345";
            let var_val = "TestValue6789";

            // Set it
            let success = set_user_env(var_name, var_val).unwrap();
            assert!(success);

            // Read it
            let val = get_user_env(var_name).unwrap();
            assert_eq!(val.as_deref(), Some(var_val));

            // Clean it up
            let success_cleanup = unset_user_env(var_name).unwrap();
            assert!(success_cleanup);

            // Read it again
            let val_after = get_user_env(var_name).unwrap();
            assert!(val_after.is_none());
        }
    }

    #[test]
    fn test_admin_privilege_check() {
        // Just verify it runs without crashing
        let _ = is_admin();
    }
}
