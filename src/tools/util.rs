use std::{borrow::Cow, env};

/// "~/blightmud" => "/home/yourname/blightmud" (Unix) or "C:\Users\yourname\blightmud" (Windows)
pub fn expand_tilde(path: &str) -> Cow<str> {
    if let Some(sub_path) = path.strip_prefix('~') {
        let home = if cfg!(windows) {
            env::var("USERPROFILE").or_else(|_| env::var("HOME"))
        } else {
            env::var("HOME")
        };
        Cow::from(home.expect("HOME or USERPROFILE must be set") + sub_path)
    } else {
        Cow::from(path)
    }
}

#[cfg(test)]
mod util_tests {
    use super::*;

    #[test]
    fn homedir_expansion() {
        env::set_var("HOME", "/home/what");
        assert_eq!("/home/what/blightmud", expand_tilde("~/blightmud"));

        env::set_var("HOME", "/Users/cindi");
        assert_eq!(
            "/Users/cindi/blightmud/data",
            expand_tilde("~/blightmud/data")
        );

        assert_eq!("/leave/it/alone", expand_tilde("/leave/it/alone"));
        assert_eq!("/leave/~/alone", expand_tilde("/leave/~/alone"));
    }
}
