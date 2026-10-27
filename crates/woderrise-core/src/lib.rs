//! Woderrise Core domain models, paths, errors, and schema serialization.
//!
//! Provides pure domain logic for FreeDesktop Wayland configurations,
//! FreeDesktop XDG Base Directory path resolution, traversal-resistant sanitization,
//! JSON schema definitions (Catalog, StagingManifest, Journal WAL), and atomic durability.

pub mod error;
pub mod models;
pub mod paths;
pub mod schema;

pub use error::{
    DomainError, IoError, PathError, Result, SecurityError, SerializationError, TransactionError,
    WoderriseError,
};
pub use models::*;
pub use paths::*;
pub use schema::*;

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8Path;

    #[test]
    fn test_core_reexports() {
        let comp = ConfigComponent::Hyprland;
        assert_eq!(comp.as_str(), "hyprland");
        assert_eq!(comp.default_entrypoint(), "hyprland.conf");

        let waybar = ConfigComponent::Waybar;
        assert_eq!(waybar.as_str(), "waybar");
        assert_eq!(waybar.default_entrypoint(), "config.jsonc");
        assert_eq!(waybar.default_config_subpath(), "waybar");
    }

    #[test]
    fn test_schema_constants() {
        assert_eq!(CURRENT_CATALOG_VERSION, 1);
        assert_eq!(CURRENT_MANIFEST_VERSION, 1);
        assert_eq!(CURRENT_JOURNAL_VERSION, 1);
        assert!(CATALOG_SCHEMA_V1.starts_with("https://woderrise.dev/schemas/"));
        assert!(MANIFEST_SCHEMA_V1.starts_with("https://woderrise.dev/schemas/"));
        assert!(JOURNAL_SCHEMA_V1.starts_with("https://woderrise.dev/schemas/"));
    }

    #[test]
    fn test_journal_default() {
        let journal = Journal::default();
        assert_eq!(journal.version, 1);
        assert!(journal.active_transaction.is_none());
        assert!(journal.transactions.is_empty());
    }

    #[test]
    fn test_catalog_default() {
        let catalog = Catalog::default();
        assert_eq!(catalog.version, 1);
        assert!(catalog.repositories.is_empty());
    }

    #[test]
    fn test_path_sanitization_invariants() {
        assert!(sanitize_relative_path(Utf8Path::new("valid/path.conf")).is_ok());
        assert!(sanitize_relative_path(Utf8Path::new("../traversal")).is_err());
        assert!(sanitize_relative_path(Utf8Path::new("/absolute")).is_err());
        assert!(sanitize_relative_path(Utf8Path::new("null\0byte")).is_err());
    }
}
