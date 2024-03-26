use thiserror::Error;

/// All errors possible to occur during reconciliation
#[derive(Debug, Error)]
pub enum NextcloudError {
    /// Any error originating from the `kube-rs` crate
    #[error("Kubernetes reported error: {source}")]
    KubeError {
        #[from]
        source: kube::Error,
    },
    /// Error in user input or Nextcloud resource definition, typically missing fields.
    #[error("Invalid Nextcloud CRD: {0}")]
    UserInputError(String),
    #[error("Deploy Nextcloud Error: {0}")]
    DeployError(String),
}


