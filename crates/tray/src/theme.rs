/// Colour scheme used for the tray's transparent dragon icon.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// White when connected, dark grey when disconnected.
    #[default]
    Dark,
    /// Black when connected, light grey when disconnected.
    Light,
}

impl Theme {
    /// Parse Tailscale-compatible theme names. The `nobg` variants are aliases because this
    /// application's icons always have a transparent background.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "dark" | "dark:nobg" => Some(Self::Dark),
            "light" | "light:nobg" => Some(Self::Light),
            _ => None,
        }
    }
}

/// Parse `--theme dark`, `--theme=light`, and their `nobg` aliases.
pub fn parse_theme_args<I>(args: I) -> Result<Theme, String>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let mut theme = Theme::default();
    while let Some(argument) = args.next() {
        let value = if argument == "--theme" {
            Some(
                args.next()
                    .ok_or_else(|| "--theme requires dark or light".to_owned())?,
            )
        } else {
            argument.strip_prefix("--theme=").map(str::to_owned)
        };
        if let Some(value) = value {
            theme = Theme::from_name(&value)
                .ok_or_else(|| format!("unsupported theme {value:?}; expected dark or light"))?;
        }
    }
    Ok(theme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_tailscale_theme_conventions() {
        assert_eq!(Theme::from_name("dark"), Some(Theme::Dark));
        assert_eq!(Theme::from_name("dark:nobg"), Some(Theme::Dark));
        assert_eq!(Theme::from_name("light"), Some(Theme::Light));
        assert_eq!(Theme::from_name("light:nobg"), Some(Theme::Light));
        assert_eq!(Theme::from_name("blue"), None);
    }

    #[test]
    fn arguments_default_to_dark_and_accept_both_forms() {
        assert_eq!(parse_theme_args(Vec::new()), Ok(Theme::Dark));
        assert_eq!(
            parse_theme_args(["--theme=light".to_owned()]),
            Ok(Theme::Light)
        );
        assert_eq!(
            parse_theme_args(["--theme".to_owned(), "dark:nobg".to_owned()]),
            Ok(Theme::Dark)
        );
    }

    #[test]
    fn invalid_arguments_have_actionable_errors() {
        assert_eq!(
            parse_theme_args(["--theme".to_owned()]),
            Err("--theme requires dark or light".to_owned())
        );
        assert_eq!(
            parse_theme_args(["--theme=blue".to_owned()]),
            Err("unsupported theme \"blue\"; expected dark or light".to_owned())
        );
    }
}
