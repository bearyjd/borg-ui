use chrono::{DateTime, Utc};

/// Default archive-name template. The `{hostname}-{profile}` prefix is what
/// lets pruning be scoped per profile with `--glob-archives`: on a repository
/// shared by several machines (a normal borg topology), one machine's
/// retention policy must never delete another machine's archives. The pre-0.3
/// default was `{datetime}-{random}` — no prefix, nothing to scope on; see
/// [`is_legacy_default_archive_name`] for how those archives are handled.
pub const DEFAULT_TEMPLATE: &str = "{hostname}-{profile}-{datetime}-{random}";

pub struct TemplateContext<'a> {
    pub now: DateTime<Utc>,
    pub hostname: &'a str,
    pub profile: &'a str,
    pub random: &'a str,
}

pub fn expand(template: &str, ctx: &TemplateContext) -> String {
    let date = ctx.now.format("%Y-%m-%d").to_string();
    let time = ctx.now.format("%H%M%S").to_string();
    let datetime = ctx.now.format("%Y-%m-%dT%H%M%S").to_string();

    template
        .replace("{date}", &date)
        .replace("{time}", &time)
        .replace("{datetime}", &datetime)
        .replace("{hostname}", &slugify(ctx.hostname))
        .replace("{profile}", &slugify(ctx.profile))
        .replace("{random}", ctx.random)
}

fn slugify(s: &str) -> String {
    let base: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = base.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "unknown".into()
    } else {
        trimmed
    }
}

/// Derives the `--glob-archives` pattern that scopes pruning to the archives
/// this template produces. Stable variables (`{hostname}`, `{profile}`)
/// expand to their real (slugified) values — exactly as [`expand`] does — and
/// time-varying variables (`{date}`, `{time}`, `{datetime}`, `{random}`)
/// become `*` wildcards, so the glob matches the profile's *historical*
/// archives too, not just names minted after this code shipped.
///
/// Returns `None` when the template cannot scope pruning safely:
/// - the glob would start with `*` (no discriminating prefix — e.g. the
///   pre-0.3 default `{datetime}-{random}`), or
/// - it contains characters we never emit in archive names (unresolved
///   `{unknown}` variables, glob metacharacters like `?`/`[`).
pub fn prune_glob(template: &str, hostname: &str, profile: &str) -> Option<String> {
    let expanded = template
        .replace("{date}", "*")
        .replace("{time}", "*")
        .replace("{datetime}", "*")
        .replace("{random}", "*")
        .replace("{hostname}", &slugify(hostname))
        .replace("{profile}", &slugify(profile));

    // Collapse runs of adjacent wildcards (`{datetime}{random}` -> `**`).
    let mut glob = String::with_capacity(expanded.len());
    for c in expanded.chars() {
        if c == '*' && glob.ends_with('*') {
            continue;
        }
        glob.push(c);
    }

    let scoped = !glob.is_empty() && !glob.starts_with('*');
    let safe = glob
        .chars()
        .all(|c| c == '*' || c.is_alphanumeric() || matches!(c, '-' | '_' | '.'));
    (scoped && safe).then_some(glob)
}

/// True when `name` looks like an archive created by the pre-0.3 default
/// template `{datetime}-{random}` (e.g. `2026-05-24T143015-ab12`). Those
/// names carry no hostname/profile prefix, so a scoped prune can no longer
/// manage them — and on a shared repository they are indistinguishable from
/// another machine's legacy archives, so it is never safe to widen the prune
/// glob to cover them automatically. Callers use this to *warn* instead.
pub fn is_legacy_default_archive_name(name: &str) -> bool {
    let b = name.as_bytes();
    // DDDD-DD-DDTDDDDDD-HHHH
    b.len() == 22
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-'
        && b[8..10].iter().all(u8::is_ascii_digit)
        && b[10] == b'T'
        && b[11..17].iter().all(u8::is_ascii_digit)
        && b[17] == b'-'
        && b[18..].iter().all(u8::is_ascii_hexdigit)
}

pub fn random_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:04x}", (nanos as u32) & 0xffff)
}

pub fn current_hostname() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn fixed_ctx<'a>(profile: &'a str, hostname: &'a str) -> TemplateContext<'a> {
        TemplateContext {
            now: Utc.with_ymd_and_hms(2026, 5, 24, 14, 30, 15).unwrap(),
            hostname,
            profile,
            random: "ab12",
        }
    }

    #[test]
    fn default_template_expands_correctly() {
        let ctx = fixed_ctx("default", "mybox");
        let result = expand(DEFAULT_TEMPLATE, &ctx);
        assert_eq!(result, "mybox-default-2026-05-24T143015-ab12");
    }

    #[test]
    fn default_template_carries_profile_prefix_for_prune_scoping() {
        // Regression: the pre-0.3 default had no prefix, so prune could not
        // be scoped and deleted other machines' archives on shared repos.
        let ctx = fixed_ctx("Work Laptop", "MY BOX!");
        let name = expand(DEFAULT_TEMPLATE, &ctx);
        let glob = prune_glob(DEFAULT_TEMPLATE, ctx.hostname, ctx.profile).unwrap();
        assert_eq!(glob, "MY-BOX-Work-Laptop-*-*");
        assert!(name.starts_with("MY-BOX-Work-Laptop-"));
    }

    #[test]
    fn date_variable() {
        let ctx = fixed_ctx("default", "mybox");
        assert_eq!(expand("{date}", &ctx), "2026-05-24");
    }

    #[test]
    fn time_variable() {
        let ctx = fixed_ctx("default", "mybox");
        assert_eq!(expand("{time}", &ctx), "143015");
    }

    #[test]
    fn hostname_variable() {
        let ctx = fixed_ctx("default", "mybox");
        assert_eq!(expand("{hostname}-snap", &ctx), "mybox-snap");
    }

    #[test]
    fn hostname_slugified() {
        let ctx = fixed_ctx("default", "MY BOX!");
        assert_eq!(expand("{hostname}", &ctx), "MY-BOX");
    }

    #[test]
    fn profile_variable() {
        let ctx = fixed_ctx("Work Laptop", "mybox");
        assert_eq!(expand("{profile}-{date}", &ctx), "Work-Laptop-2026-05-24");
    }

    #[test]
    fn empty_hostname_falls_back() {
        let ctx = fixed_ctx("default", "");
        assert_eq!(expand("{hostname}", &ctx), "unknown");
    }

    #[test]
    fn literal_text_preserved() {
        let ctx = fixed_ctx("default", "mybox");
        assert_eq!(expand("snap-{date}-end", &ctx), "snap-2026-05-24-end");
    }

    #[test]
    fn unknown_variables_left_in_place() {
        let ctx = fixed_ctx("default", "mybox");
        assert_eq!(expand("{unknown}-{date}", &ctx), "{unknown}-2026-05-24");
    }

    #[test]
    fn random_suffix_is_4_hex() {
        let r = random_suffix();
        assert_eq!(r.len(), 4);
        assert!(r.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn slugify_strips_specials() {
        assert_eq!(slugify("a b/c"), "a-b-c");
        assert_eq!(slugify("foo.bar_baz-qux"), "foo.bar_baz-qux");
        assert_eq!(slugify("!!!"), "unknown");
        assert_eq!(slugify(""), "unknown");
    }

    #[test]
    fn prune_glob_matches_custom_templates_historical_archives() {
        // Stable variables expand, time-varying ones become wildcards, so the
        // glob also matches archives this template created in the past.
        assert_eq!(
            prune_glob("{hostname}-snap-{date}", "mybox", "default").as_deref(),
            Some("mybox-snap-*")
        );
        assert_eq!(
            prune_glob("pc-{date}-{time}", "mybox", "default").as_deref(),
            Some("pc-*-*")
        );
        assert_eq!(
            prune_glob("{profile}.{datetime}{random}", "mybox", "Work Laptop").as_deref(),
            Some("Work-Laptop.*")
        );
    }

    #[test]
    fn prune_glob_refuses_templates_without_a_prefix() {
        // The old default and anything else starting with a time-varying
        // variable has no discriminating prefix -> scoping is impossible.
        assert_eq!(prune_glob("{datetime}-{random}", "mybox", "default"), None);
        assert_eq!(prune_glob("{random}-backup", "mybox", "default"), None);
        assert_eq!(prune_glob("", "mybox", "default"), None);
    }

    #[test]
    fn prune_glob_refuses_unexpected_characters() {
        // Unresolved variables and glob metacharacters must never reach borg.
        assert_eq!(prune_glob("{unknown}-{date}", "mybox", "default"), None);
        assert_eq!(prune_glob("what?-{date}", "mybox", "default"), None);
        assert_eq!(prune_glob("a[b]-{date}", "mybox", "default"), None);
    }

    #[test]
    fn colliding_slugs_share_a_prune_scope() {
        // Documented limitation: slugify maps distinct raw names onto the same
        // slug ("Work Laptop" and "work?laptop" both scope as one prefix on a
        // shared repo — but note slugs are case-sensitive, so "Work Laptop"
        // and "work laptop" do NOT collide). Machines/profiles whose slugs
        // collide share one prune scope and one retention policy. This is a
        // narrowing-only hazard within the shared prefix, never a widening to
        // unrelated archives; users avoid it by choosing distinct names.
        let a = prune_glob(DEFAULT_TEMPLATE, "my box", "Work Laptop").unwrap();
        let b = prune_glob(DEFAULT_TEMPLATE, "my?box", "Work?Laptop").unwrap();
        assert_eq!(a, b);
        assert_eq!(a, "my-box-Work-Laptop-*-*");
        let c = prune_glob(DEFAULT_TEMPLATE, "my box", "work laptop").unwrap();
        assert_ne!(a, c, "case differences keep scopes distinct");
    }

    #[test]
    fn legacy_default_names_are_detected() {
        assert!(is_legacy_default_archive_name("2026-05-24T143015-ab12"));
        assert!(is_legacy_default_archive_name("2024-01-01T000000-FFFF"));
    }

    #[test]
    fn prefixed_and_custom_names_are_not_legacy() {
        // New default output.
        assert!(!is_legacy_default_archive_name(
            "mybox-default-2026-05-24T143015-ab12"
        ));
        assert!(!is_legacy_default_archive_name("my-backup_2024.01.15"));
        assert!(!is_legacy_default_archive_name("2026-05-24T143015-ab1"));
        assert!(!is_legacy_default_archive_name("2026-05-24T143015-zzzz"));
        assert!(!is_legacy_default_archive_name(""));
    }
}
