use chrono::{DateTime, Utc};

pub const DEFAULT_TEMPLATE: &str = "{datetime}-{random}";

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
        assert_eq!(result, "2026-05-24T143015-ab12");
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
}
