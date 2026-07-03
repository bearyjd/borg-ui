use std::path::Path;
use std::process::Stdio;

use chrono::Utc;
use serde::Serialize;
use tokio::io::AsyncWriteExt;

use crate::history::{self, DeliveryEvent};
use crate::profiles::{Profile, SmtpTlsMode};

const WEBHOOK_ACCOUNT_PREFIX: &str = "report-webhook:";
const SMTP_ACCOUNT_PREFIX: &str = "report-smtp-password:";

#[derive(Debug, Clone, Serialize)]
pub struct ReportPayload<'a> {
    pub event: &'a str,
    pub severity: &'a str,
    pub summary: &'a str,
    pub timestamp: String,
}

pub fn webhook_account(profile_id: &str) -> String {
    format!("{WEBHOOK_ACCOUNT_PREFIX}{profile_id}")
}

pub fn smtp_account(profile_id: &str) -> String {
    format!("{SMTP_ACCOUNT_PREFIX}{profile_id}")
}

pub fn validate_preferences(profile: &Profile) -> Result<(), String> {
    let settings = &profile.reporting;
    if settings.stale_after_hours == 0 || settings.failure_threshold == 0 {
        return Err("reporting thresholds must be greater than zero".into());
    }
    if settings.enabled && !settings.webhook_enabled && !settings.smtp_enabled {
        return Err("enable at least one reporting channel".into());
    }
    if settings.smtp_enabled {
        for (name, value) in [
            ("SMTP host", settings.smtp_host.as_str()),
            ("SMTP username", settings.smtp_username.as_str()),
            ("From address", settings.email_from.as_str()),
            ("To address", settings.email_to.as_str()),
        ] {
            if value.trim().is_empty() || value.contains(['\r', '\n', '\0']) {
                return Err(format!("{name} is empty or invalid"));
            }
        }
        if settings.smtp_port == 0 {
            return Err("SMTP port cannot be zero".into());
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn deliver(
    config_dir: &Path,
    profile: &Profile,
    event: &str,
    severity: &str,
    summary: &str,
    delivery_id: Option<String>,
    attempt: u8,
    only_channel: Option<&str>,
) -> Result<(), String> {
    if !profile.reporting.enabled {
        return Ok(());
    }
    let payload = ReportPayload {
        event,
        severity,
        summary,
        timestamp: Utc::now().to_rfc3339(),
    };
    let id = delivery_id.unwrap_or_else(|| {
        format!(
            "report-{}-{}",
            event,
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        )
    });
    let mut failed = false;
    if profile.reporting.webhook_enabled && only_channel.is_none_or(|channel| channel == "webhook")
    {
        let result = deliver_webhook(profile, &payload).await;
        failed |= result.is_err();
        record(config_dir, profile, &id, "webhook", event, attempt, result).await;
    }
    if profile.reporting.smtp_enabled && only_channel.is_none_or(|channel| channel == "smtp") {
        let result = deliver_smtp(profile, &payload).await;
        failed |= result.is_err();
        record(config_dir, profile, &id, "smtp", event, attempt, result).await;
    }
    if failed {
        Err("one or more report deliveries failed".into())
    } else {
        Ok(())
    }
}

async fn deliver_webhook(
    profile: &Profile,
    payload: &ReportPayload<'_>,
) -> Result<(), DeliveryFailure> {
    let url = crate::keychain::get_passphrase(&webhook_account(&profile.id))
        .map_err(|_| DeliveryFailure::permanent("webhook credential unavailable"))?
        .ok_or_else(|| DeliveryFailure::permanent("webhook credential missing"))?;
    if !url.starts_with("https://") {
        return Err(DeliveryFailure::permanent("webhook must use HTTPS"));
    }
    let response = reqwest::Client::new()
        .post(url)
        .json(payload)
        .send()
        .await
        .map_err(|_| DeliveryFailure::transient("webhook transport failure"))?;
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(DeliveryFailure {
            message: format!("webhook returned HTTP {}", status.as_u16()),
            transient: status.as_u16() == 429 || status.is_server_error(),
        })
    }
}

async fn deliver_smtp(
    profile: &Profile,
    payload: &ReportPayload<'_>,
) -> Result<(), DeliveryFailure> {
    let settings = &profile.reporting;
    let password = crate::keychain::get_passphrase(&smtp_account(&profile.id))
        .map_err(|_| DeliveryFailure::permanent("SMTP credential unavailable"))?
        .ok_or_else(|| DeliveryFailure::permanent("SMTP credential missing"))?;
    if password.contains(['\r', '\n', '\0', '"']) {
        return Err(DeliveryFailure::permanent(
            "SMTP credential contains unsupported characters",
        ));
    }
    let url = smtp_url(
        &settings.smtp_host,
        settings.smtp_port,
        &settings.smtp_tls_mode,
    );
    let body = format!(
        "From: {}\r\nTo: {}\r\nSubject: BorgUI {}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}\r\n",
        settings.email_from, settings.email_to, payload.event, payload.summary
    );
    let mut file = tempfile::Builder::new()
        .prefix(".borgui-report-")
        .tempfile()
        .map_err(|_| DeliveryFailure::permanent("cannot create report message"))?;
    use std::io::Write;
    file.write_all(body.as_bytes())
        .map_err(|_| DeliveryFailure::permanent("cannot write report message"))?;
    let mut command = tokio::process::Command::new("curl");
    command
        .args(["--silent", "--show-error", "--fail", "--ssl-reqd"])
        .arg("--url")
        .arg(url)
        .args(["--mail-from", &settings.email_from])
        .args(["--mail-rcpt", &settings.email_to])
        .arg("--upload-file")
        .arg(file.path())
        .args(["--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|_| DeliveryFailure::transient("SMTP transport unavailable"))?;
    let config = format!(
        "user = \"{}:{}\"\n",
        settings
            .smtp_username
            .replace('\\', "\\\\")
            .replace('"', "\\\""),
        password.replace('\\', "\\\\")
    );
    child
        .stdin
        .take()
        .ok_or_else(|| DeliveryFailure::transient("SMTP input unavailable"))?
        .write_all(config.as_bytes())
        .await
        .map_err(|_| DeliveryFailure::transient("SMTP input failure"))?;
    let output = child
        .wait_with_output()
        .await
        .map_err(|_| DeliveryFailure::transient("SMTP transport failure"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(DeliveryFailure {
            message: "SMTP delivery failed".into(),
            transient: output.status.code() != Some(67),
        })
    }
}

fn smtp_url(host: &str, port: u16, mode: &SmtpTlsMode) -> String {
    let scheme = match mode {
        SmtpTlsMode::StartTls => "smtp",
        SmtpTlsMode::ImplicitTls => "smtps",
    };
    format!("{scheme}://{host}:{port}")
}

struct DeliveryFailure {
    message: String,
    transient: bool,
}

impl DeliveryFailure {
    fn permanent(message: &str) -> Self {
        Self {
            message: message.into(),
            transient: false,
        }
    }
    fn transient(message: &str) -> Self {
        Self {
            message: message.into(),
            transient: true,
        }
    }
}

async fn record(
    config_dir: &Path,
    profile: &Profile,
    id: &str,
    channel: &str,
    report_kind: &str,
    attempt: u8,
    result: Result<(), DeliveryFailure>,
) {
    let (outcome, transient, error_message) = match result {
        Ok(()) => ("success", false, None),
        Err(error) => ("failure", error.transient, Some(error.message)),
    };
    let _ = history::append_delivery(
        config_dir,
        DeliveryEvent {
            id: id.into(),
            timestamp: Utc::now().to_rfc3339(),
            profile_id: profile.id.clone(),
            channel: channel.into(),
            report_kind: report_kind.into(),
            outcome: outcome.into(),
            attempt,
            transient,
            error_message,
        },
    )
    .await;
}

pub async fn report_backup_outcome(
    config_dir: &Path,
    profile: &Profile,
    report: &crate::scheduled::RunReport,
) {
    if report.skipped_reason.is_some() {
        return;
    }
    if report.error.is_some() || report.destination_failures > 0 {
        let _ = deliver(
            config_dir,
            profile,
            "failure",
            "red",
            "An automatic backup failed. Open BorgUI for redacted diagnostics.",
            None,
            1,
            None,
        )
        .await;
        return;
    }
    let previous_failed = history::load(config_dir)
        .await
        .ok()
        .and_then(|events| {
            events
                .iter()
                .rev()
                .filter(|event| event.kind == "backup")
                .nth(1)
                .map(|event| event.outcome != "success")
        })
        .unwrap_or(false);
    if previous_failed {
        let _ = deliver(
            config_dir,
            profile,
            "recovery",
            "green",
            "Automatic backups recovered after a failure.",
            None,
            1,
            None,
        )
        .await;
    }
}

pub async fn run_daily(config_dir: &Path) -> Result<(), String> {
    let data = crate::profiles::load(config_dir).await?;
    let profile = data
        .active()
        .cloned()
        .ok_or_else(|| "no active profile".to_string())?;
    for pending in history::pending_deliveries(config_dir, &profile.id).await? {
        let _ = deliver(
            config_dir,
            &profile,
            &pending.report_kind,
            "amber",
            "Retrying a previously unsuccessful BorgUI report.",
            Some(pending.id),
            pending.attempt + 1,
            Some(&pending.channel),
        )
        .await;
    }
    if profile.reporting.enabled && profile.reporting.daily_digest {
        let latest = history::load(config_dir)
            .await?
            .into_iter()
            .rev()
            .find(|event| event.kind == "backup");
        let (severity, summary) = match latest {
            Some(event) if event.outcome == "success" => (
                "green",
                "Daily digest: the latest recorded backup succeeded.",
            ),
            Some(_) => ("red", "Daily digest: the latest recorded backup failed."),
            None => ("amber", "Daily digest: no backup has been recorded yet."),
        };
        let _ = deliver(
            config_dir, &profile, "digest", severity, summary, None, 1, None,
        )
        .await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_contains_no_paths_or_secrets() {
        let payload = serde_json::to_string(&ReportPayload {
            event: "failure",
            severity: "red",
            summary: "Backup failed",
            timestamp: "2026-07-02T00:00:00Z".into(),
        })
        .unwrap();
        assert!(!payload.contains("password"));
        assert!(!payload.contains("C:\\"));
    }

    #[test]
    fn smtp_modes_use_starttls_and_implicit_schemes() {
        assert_eq!(
            smtp_url("mail.example.com", 587, &SmtpTlsMode::StartTls),
            "smtp://mail.example.com:587"
        );
        assert_eq!(
            smtp_url("mail.example.com", 465, &SmtpTlsMode::ImplicitTls),
            "smtps://mail.example.com:465"
        );
    }
}
