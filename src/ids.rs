//! Resource IDs from assets/resource.h.

pub const IDI_APP: u16 = 1;
pub const IDI_SUSPENDED: u16 = 2;

pub const IDD_SETTINGS: u16 = 101;
pub const IDD_HISTORY: u16 = 102;

pub const IDC_HK_TOGGLE: i32 = 1001;
pub const IDC_HK_EMERGENCY: i32 = 1002;
pub const IDC_MINIMIZE: i32 = 1003;
pub const IDC_NOTIFY: i32 = 1004;
pub const IDC_AUTOSTART: i32 = 1005;
pub const IDC_BLACKLIST: i32 = 1006;
pub const IDC_MIN_DELAY: i32 = 1007;
pub const IDC_MIN_DELAY_SPIN: i32 = 1008;
pub const IDC_RESUME_DELAY: i32 = 1009;
pub const IDC_RESUME_DELAY_SPIN: i32 = 1010;
pub const IDC_SHOW_HISTORY: i32 = 1011;

pub const IDC_HISTORY_LIST: i32 = 1101;
pub const IDC_OPEN_LOG: i32 = 1102;
pub const IDC_CLEAR_LOG: i32 = 1103;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_resource_header() {
        let rust: &[(&str, i32)] = &[
            ("IDI_APP", IDI_APP.into()),
            ("IDI_SUSPENDED", IDI_SUSPENDED.into()),
            ("IDD_SETTINGS", IDD_SETTINGS.into()),
            ("IDD_HISTORY", IDD_HISTORY.into()),
            ("IDC_HK_TOGGLE", IDC_HK_TOGGLE),
            ("IDC_HK_EMERGENCY", IDC_HK_EMERGENCY),
            ("IDC_MINIMIZE", IDC_MINIMIZE),
            ("IDC_NOTIFY", IDC_NOTIFY),
            ("IDC_AUTOSTART", IDC_AUTOSTART),
            ("IDC_BLACKLIST", IDC_BLACKLIST),
            ("IDC_MIN_DELAY", IDC_MIN_DELAY),
            ("IDC_MIN_DELAY_SPIN", IDC_MIN_DELAY_SPIN),
            ("IDC_RESUME_DELAY", IDC_RESUME_DELAY),
            ("IDC_RESUME_DELAY_SPIN", IDC_RESUME_DELAY_SPIN),
            ("IDC_SHOW_HISTORY", IDC_SHOW_HISTORY),
            ("IDC_HISTORY_LIST", IDC_HISTORY_LIST),
            ("IDC_OPEN_LOG", IDC_OPEN_LOG),
            ("IDC_CLEAR_LOG", IDC_CLEAR_LOG),
        ];
        let header: Vec<(String, i32)> = include_str!("../assets/resource.h")
            .lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                (parts.next()? == "#define").then_some(())?;
                let name = parts.next()?;
                let value = parts.next()?.parse().ok()?;
                name.starts_with("ID").then(|| (name.to_string(), value))
            })
            .filter(|(name, _)| name != "IDC_STATIC")
            .collect();

        assert_eq!(
            header.len(),
            rust.len(),
            "resource.h and ids.rs differ in size"
        );
        for (name, value) in rust {
            assert!(
                header.contains(&(name.to_string(), *value)),
                "{name} = {value} missing or different in resource.h"
            );
        }
    }
}
