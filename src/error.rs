//! Error types for flowghetti.

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to read directory {dir}: {source}")]
    ReadDir {
        dir: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read {path}: {source}")]
    ReadFile {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("HCL parse error in {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: hcl::Error,
    },

    #[error(
        "no glowwiththeflow module found: expected a `module` block declaring both `ressources` and `flows`"
    )]
    NoModule,
}
