use thiserror::Error;

#[derive(Error, Debug)]
pub enum LauncherError {
    #[error("Version {0} not found")]
    #[allow(dead_code)]
    VersionNotFound(String),

    #[error("Java not found. Please set JAVA_HOME or use --runtime")]
    #[allow(dead_code)]
    JavaNotFound,

    #[error("Authentication required but not found. Please run 'mclc login'.")]
    AuthNotFound,
}

#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, LauncherError>;
