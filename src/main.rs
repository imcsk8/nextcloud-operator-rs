use std::sync::Arc;

use futures::stream::StreamExt;
use kube::runtime::watcher::Config;
use kube::Resource;
use kube::ResourceExt;
use kube::{client::Client, runtime::controller::Action, runtime::Controller, Api};
use tokio::time::Duration;

use crate::crd::{Nextcloud, create_crd};

pub mod crd;
mod nextcloud;
mod finalizer;

#[tokio::main]
async fn main() {
    // First, a Kubernetes client must be obtained using the `kube` crate
    // The client will later be moved to the custom controller
    let kubernetes_client: Client = Client::try_default()
        .await
        .expect("Expected a valid KUBECONFIG environment variable.");


    println!("---- Before creating crd ---");
    create_crd(kubernetes_client.clone()).await;
    println!("---- After creating crd ---");

    // Preparation of resources used by the `kube_runtime::Controller`
    let crd_api: Api<Nextcloud> = Api::all(kubernetes_client.clone());
    let context: Arc<ContextData> = Arc::new(ContextData::new(kubernetes_client.clone()));

    // The controller comes from the `kube_runtime` crate and manages the reconciliation process.
    // It requires the following information:
    // - `kube::Api<T>` this controller "owns". In this case, `T = Nextcloud`, as this controller owns the `Nextcloud` resource,
    // - `kube::runtime::watcher::Config` can be adjusted for precise filtering of `Nextcloud` resources before the actual reconciliation, e.g. by label,
    // - `reconcile` function with reconciliation logic to be called each time a resource of `Nextcloud` kind is created/updated/deleted,
    // - `on_error` function to call whenever reconciliation fails.
    Controller::new(crd_api.clone(), Config::default())
        .run(reconcile, on_error, context)
        .for_each(|reconciliation_result| async move {
            match reconciliation_result {
                Ok(nextcloud_resource) => {
                    println!("VA AL LOG? Reconciliation successful. Resource: {:?}", nextcloud_resource);
                }
                Err(reconciliation_err) => {
                    eprintln!("VA AL LOG? Reconciliation error: {:?}", reconciliation_err)
                }
            }
        })
        .await;
}

/// Context injected with each `reconcile` and `on_error` method invocation.
struct ContextData {
    /// Kubernetes client to make Kubernetes API requests with. Required for K8S resource management.
    client: Client,
}

impl ContextData {
    /// Constructs a new instance of ContextData.
    ///
    /// # Arguments:
    /// - `client`: A Kubernetes client to make Kubernetes REST API requests with. Resources
    /// will be created and deleted with this client.
    pub fn new(client: Client) -> Self {
        ContextData { client }
    }
}

/// Action to be taken upon an `Nextcloud` resource during reconciliation
enum NextcloudAction {
    /// Create the subresources, this includes spawning `n` pods with Nextcloud service
    Create,
    /// Delete all subresources created in the `Create` phase
    Delete,
    /// This `Nextcloud` resource is in desired state and requires no actions to be taken
    NoOp,
}

async fn reconcile(nextcloud: Arc<Nextcloud>, context: Arc<ContextData>) -> Result<Action, Error> {
    let client: Client = context.client.clone(); // The `Client` is shared -> a clone from the reference is obtained

    // The resource of `Nextcloud` kind is required to have a namespace set. However, it is not guaranteed
    // the resource will have a `namespace` set. Therefore, the `namespace` field on object's metadata
    // is optional and Rust forces the programmer to check for it's existence first.
    let namespace: String = match nextcloud.namespace() {
        None => {
            // If there is no namespace to deploy to defined, reconciliation ends with an error immediately.
            return Err(Error::UserInputError(
                "Expected Nextcloud resource to be namespaced. Can't deploy to an unknown namespace."
                    .to_owned(),
            ));
        }
        // If namespace is known, proceed. In a more advanced version of the operator, perhaps
        // the namespace could be checked for existence first.
        Some(namespace) => namespace,
    };
    let name = nextcloud.name_any(); // Name of the Nextcloud resource is used to name the subresources as well.

    // Performs action as decided by the `determine_action` function.
    return match determine_action(&nextcloud) {
        NextcloudAction::Create => {
            // Creates a deployment with `n` Nextcloud service pods, but applies a finalizer first.
            // Finalizer is applied first, as the operator might be shut down and restarted
            // at any time, leaving subresources in intermediate state. This prevents leaks on
            // the `Nextcloud` resource deletion.

            // Apply the finalizer first. If that fails, the `?` operator invokes automatic conversion
            // of `kube::Error` to the `Error` defined in this crate.
            finalizer::add(client.clone(), &name, &namespace).await?;
            // Invoke creation of a Kubernetes built-in resource named deployment with `n` nextcloud service pods.
            println!("Creating php-fpm endpoint");
            nextcloud::deploy(client, &name, nextcloud.spec.replicas, &namespace).await?;
            Ok(Action::requeue(Duration::from_secs(10)))
        }
        NextcloudAction::Delete => {
            // Deletes any subresources related to this `Nextcloud` resources. If and only if all subresources
            // are deleted, the finalizer is removed and Kubernetes is free to remove the `Nextcloud` resource.

            //First, delete the deployment. If there is any error deleting the deployment, it is
            // automatically converted into `Error` defined in this crate and the reconciliation is ended
            // with that error.
            // Note: A more advanced implementation would check for the Deployment's existence.
            nextcloud::delete(client.clone(), &name, &namespace).await?;

            // Once the deployment is successfully removed, remove the finalizer to make it possible
            // for Kubernetes to delete the `Nextcloud` resource.
            finalizer::delete(client, &name, &namespace).await?;
            Ok(Action::await_change()) // Makes no sense to delete after a successful delete, as the resource is gone
        }
        // The resource is already in desired state, do nothing and re-check after 10 seconds
        NextcloudAction::NoOp => Ok(Action::requeue(Duration::from_secs(10))),
    };
}

/// Resources arrives into reconciliation queue in a certain state. This function looks at
/// the state of given `Nextcloud` resource and decides which actions needs to be performed.
/// The finite set of possible actions is represented by the `NextcloudAction` enum.
///
/// # Arguments
/// - `echo`: A reference to `Nextcloud` being reconciled to decide next action upon.
/// TODO: check for more resource status
/// meta() -> https://docs.rs/kube/0.88.1/kube/core/struct.ObjectMeta.html
fn determine_action(nextcloud: &Nextcloud) -> NextcloudAction {
    return if nextcloud.meta().deletion_timestamp.is_some() {
        NextcloudAction::Delete
    } else if nextcloud
        .meta()
        .finalizers
        .as_ref()
        .map_or(true, |finalizers| finalizers.is_empty())
    {
        NextcloudAction::Create
    } else {
        NextcloudAction::NoOp
    };
}

/// Actions to be taken when a reconciliation fails - for whatever reason.
/// Prints out the error to `stderr` and requeues the resource for another reconciliation after
/// five seconds.
///
/// # Arguments
/// - `nextcloud`: The erroneous resource.
/// - `error`: A reference to the `kube::Error` that occurred during reconciliation.
/// - `_context`: Unused argument. Context Data "injected" automatically by kube-rs.
fn on_error(nextcloud: Arc<Nextcloud>, error: &Error, _context: Arc<ContextData>) -> Action {
    eprintln!("Reconciliation error:\n{:?}.\n{:?}", error, nextcloud);
    Action::requeue(Duration::from_secs(5))
}

/// All errors possible to occur during reconciliation
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Any error originating from the `kube-rs` crate
    #[error("Kubernetes reported error: {source}")]
    KubeError {
        #[from]
        source: kube::Error,
    },
    /// Error in user input or Nextcloud resource definition, typically missing fields.
    #[error("Invalid Nextcloud CRD: {0}")]
    UserInputError(String),
}
