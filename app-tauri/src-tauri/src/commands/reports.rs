//! Outbound report settings, secret status, and test delivery.

use super::*;

#[derive(Debug, Serialize)]
pub struct ReportingSecretStatus {
    pub webhook_configured: bool,
    pub smtp_password_configured: bool,
}

#[tauri::command]
pub async fn reporting_secret_status(
    app: tauri::AppHandle,
) -> Result<ReportingSecretStatus, String> {
    let data = read_profiles(&app).await?;
    let id = &data.require_active()?.id;
    Ok(ReportingSecretStatus {
        webhook_configured: keychain::has_passphrase(&crate::reporting::webhook_account(id))?,
        smtp_password_configured: keychain::has_passphrase(&crate::reporting::smtp_account(id))?,
    })
}

#[tauri::command]
pub async fn save_reporting_settings(
    app: tauri::AppHandle,
    settings: profiles::ReportPreferences,
    webhook_url: Option<String>,
    smtp_password: Option<String>,
) -> Result<(), String> {
    let mut data = read_profiles(&app).await?;
    let profile = data.require_active_mut()?;
    profile.reporting = settings.clone();
    crate::reporting::validate_preferences(profile)?;
    if let Some(url) = webhook_url {
        if url.is_empty() {
            keychain::clear_passphrase(&crate::reporting::webhook_account(&profile.id))?;
        } else if !url.starts_with("https://") || url.contains(['\r', '\n', '\0']) {
            return Err("webhook URL must be a valid HTTPS URL".into());
        } else {
            keychain::set_passphrase(&crate::reporting::webhook_account(&profile.id), &url)?;
        }
    }
    if let Some(password) = smtp_password {
        if password.is_empty() {
            keychain::clear_passphrase(&crate::reporting::smtp_account(&profile.id))?;
        } else {
            keychain::set_passphrase(&crate::reporting::smtp_account(&profile.id), &password)?;
        }
    }
    if settings.webhook_enabled
        && !keychain::has_passphrase(&crate::reporting::webhook_account(&profile.id))?
    {
        return Err("configure the HTTPS webhook URL before enabling webhooks".into());
    }
    if settings.smtp_enabled
        && !keychain::has_passphrase(&crate::reporting::smtp_account(&profile.id))?
    {
        return Err("configure the SMTP password before enabling email".into());
    }
    let profile_id = profile.id.clone();
    write_profiles(&app, &data).await?;
    const TASK: &str = "BorgUI-Daily-Health-Report";
    if settings.enabled && settings.daily_digest {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        borg_platform_win::scheduler::schedule_backup(
            TASK,
            &exe.to_string_lossy(),
            "--scheduled-health-report",
            &borg_platform_win::scheduler::Schedule::Daily { hour: 9, minute: 0 },
            false,
        )
        .await
        .map_err(|error| error.to_string())?;
    } else {
        let _ = borg_platform_win::scheduler::unschedule_backup(TASK).await;
    }
    tracing::debug!(profile_id, "reporting settings updated");
    Ok(())
}

#[tauri::command]
pub async fn send_test_report(app: tauri::AppHandle) -> Result<(), String> {
    let data = read_profiles(&app).await?;
    let profile = data.require_active()?.clone();
    if !profile.reporting.enabled {
        return Err("enable outbound reporting before sending a test".into());
    }
    crate::reporting::validate_preferences(&profile)?;
    let dir = config_dir(&app).await?;
    crate::reporting::deliver(
        &dir,
        &profile,
        "digest",
        "green",
        "BorgUI test report delivered successfully.",
        None,
        1,
        None,
    )
    .await
}
