use std::fmt;

#[derive(Debug)]
pub enum GitpkgError {
    Io(std::io::Error),
    HomeNotFound,
    PackageNotFound(String),
    BuildFailed(String),
    CloneFailed,
    Git(String),
    Parse(String),
    #[allow(dead_code)]
    PackageManagerNotFound,
    Cancelled,
}

impl fmt::Display for GitpkgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitpkgError::Io(e) => write!(f, "{}", e),
            GitpkgError::HomeNotFound => write!(f, "HOME environment variable is not set"),
            GitpkgError::PackageNotFound(pkg) => write!(f, "Package {} is not installed", pkg),
            GitpkgError::BuildFailed(repo) => write!(f, "Build failed for {}", repo),
            GitpkgError::CloneFailed => write!(f, "Git clone failed"),
            GitpkgError::Git(msg) => write!(f, "Git error: {}", msg),
            GitpkgError::Parse(msg) => write!(f, "{}", msg),
            GitpkgError::PackageManagerNotFound => write!(f, "No package manager detected"),
            GitpkgError::Cancelled => write!(f, "Operation cancelled"),
        }
    }
}

impl std::error::Error for GitpkgError {}

impl From<std::io::Error> for GitpkgError {
    fn from(e: std::io::Error) -> Self {
        GitpkgError::Io(e)
    }
}

impl From<toml::ser::Error> for GitpkgError {
    fn from(e: toml::ser::Error) -> Self {
        GitpkgError::Parse(e.to_string())
    }
}

impl From<toml::de::Error> for GitpkgError {
    fn from(e: toml::de::Error) -> Self {
        GitpkgError::Parse(e.to_string())
    }
}
