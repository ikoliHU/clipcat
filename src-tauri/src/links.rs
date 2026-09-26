pub const REPOSITORY: &str = "https://github.com/catninth/clipcat";
pub const LICENSE: &str = "https://github.com/catninth/cutcat/blob/main/LICENSE";

pub fn project_url(target: &str) -> Option<&'static str> {
    match target {
        "repository" => Some(REPOSITORY),
        "license" => Some(LICENSE),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_project_destinations_can_be_opened() {
        assert_eq!(project_url("repository"), Some(REPOSITORY));
        assert_eq!(project_url("license"), Some(LICENSE));
        for value in ["file:///C:/Windows", "javascript:alert(1)", "https://example.com", "--help"] {
            assert!(project_url(value).is_none());
        }
    }
}
