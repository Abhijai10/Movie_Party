#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
}

#[cfg(test)]
mod tests {
    use super::Platform;

    #[test]
    fn phase_zero_identity_module_is_available() {
        assert_eq!(Platform::MacOs, Platform::MacOs);
    }
}
