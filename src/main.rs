use std::sync::Arc;
use std::collections::BTreeMap;
use futures::{pin_mut, TryStreamExt, StreamExt};
use kube::runtime::watcher::Config;
use kube::Resource;
use kube::ResourceExt;
use kube::{
    client::Client,
    runtime::controller::Action,
    runtime::Controller,
    runtime::{watcher, WatchStreamExt},
    Api,
    api::WatchEvent,
    api::ListParams,
};

use futures::stream::Stream;
//use kube_runtime::watcher;
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::api::events::v1::Event;
use tokio::time::Duration;
use kube::runtime::controller::Error as KubeContError;
//use log::{info, debug};
use pretty_env_logger;
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};



#[macro_use] extern crate log;

use crate::crd::{Nextcloud, create_crd};

pub mod crd;
mod nextcloud;
mod finalizer;

#[tokio::main]
//for watcher experiment async fn main() -> anyhow::Result<()> {
async fn main() -> Result <(), NextcloudError> {
    pretty_env_logger::init_timed();
    // First, a Kubernetes client must be obtained using the `kube` crate
    // The client will later be moved to the custom controller
    info!("--------- WTF!!");
    let kubernetes_client: Client = Client::try_default()
        .await
        .expect("Expected a valid KUBECONFIG environment variable.");


    debug!("---- Before creating crd ---");
    create_crd(kubernetes_client.clone()).await;
    debug!("---- After creating crd ---");

    // Preparation of resources used by the `kube_runtime::Controller`
    let crd_api: Api<Nextcloud> = Api::all(kubernetes_client.clone());
    //let events: Api<Event> = Api::default_namespaced(kubernetes_client.clone());
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
                    info!("Reconciliation successful. Resource: {:?}", nextcloud_resource);
                },
                Err(reconciliation_err) => {
                    match reconciliation_err {
                        KubeContError::ReconcilerFailed(err, obj) => {
                            info!("Nextcloud Reconciliation error: {:?}",
                                err);
                        },
                        _ => {},
                    }
                }
            }
        }).await;
    Ok(())
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
    /// Update subresurces and replicas
    Update,
    /// Delete all subresources created in the `Create` phase
    Delete,
    /// This `Nextcloud` resource is in desired state and requires no actions to be taken
    NoOp,
}

async fn reconcile(nextcloud: Arc<Nextcloud>, context: Arc<ContextData>) -> Result<Action, NextcloudError> {
    let client: Client = context.client.clone(); // The `Client` is shared -> a clone from the reference is obtained

    // The resource of `Nextcloud` kind is required to have a namespace set. However, it is not guaranteed
    // the resource will have a `namespace` set. Therefore, the `namespace` field on object's metadata
    // is optional and Rust forces the programmer to check for it's existence first.
    let namespace: String = match nextcloud.namespace() {
        None => {
            // If there is no namespace to deploy to defined, reconciliation ends with an error immediately.
            return Err(NextcloudError::UserInputError(
                "Expected Nextcloud resource to be namespaced. Can't deploy to an unknown namespace."
                    .to_owned(),
            ));
        }
        // If namespace is known, proceed. In a more advanced version of the operator, perhaps
        // the namespace could be checked for existence first.
        Some(namespace) => namespace,
    };
    let name = nextcloud.name_any(); // Name of the Nextcloud resource is used to name the subresources as well.
    info!("Nextcloud resource name: {}", name);
    info!("Nextcloud resource UUID: {}", nextcloud.uid().unwrap());
    info!("Pod List for: {}", name);
    match check_component_status(client.clone(), namespace.clone()).await {
        Ok(c) => info!("Component status ok {:?}", c),
        Err(e) => info!("Component status NOT ok {:?}", e),
    };

    // Performs action as decided by the `determine_action` function.
    return match determine_action(&nextcloud, client.clone()).await.unwrap() {
        NextcloudAction::Create | NextcloudAction::Update => {
            // Creates a deployment with `n` Nextcloud service pods, but applies a finalizer first.
            // Finalizer is applied first, as the operator might be shut down and restarted
            // at any time, leaving subresources in intermediate state. This prevents leaks on
            // the `Nextcloud` resource deletion.

            // Apply the finalizer first. If that fails, the `?` operator
            // invokes automatic conversion
            // of `kube::Error` to the `Error` defined in this crate.
            finalizer::add(client.clone(), &name, &namespace).await?;
            // Invoke creation/update of a Kubernetes built-in resource named
            // deployment with `n` nextcloud service pods.

            info!("Creating/Updating php-fpm endpoint");
            nextcloud::apply(client, &name, nextcloud, &namespace).await?;
            Ok(Action::requeue(Duration::from_secs(10)))
        }
        NextcloudAction::Delete => {
            // Deletes any subresources related to this `Nextcloud` resources. If and only if all subresources
            // are deleted, the finalizer is removed and Kubernetes is free to remove the `Nextcloud` resource.

            //First, delete the deployment. If there is any error deleting the deployment, it is
            // automatically converted into `Error` defined in this crate and the reconciliation is ended
            // with that error.
            // Note: A more advanced implementation would check for the Deployment's existence.
            debug!("----------- DELETING THIS BITCH!!!! NAME: {} NAMESPACE: {}", &name, &namespace);
            match nextcloud::delete(client.clone(), &name, &namespace).await {
                Ok(d)  => info!("Deployment deleted"),
                Err(e) => info!("No deployments for Nextcloud: {}", name),
            };

            // Once the deployment is successfully removed, remove the finalizer to make it possible
            // for Kubernetes to delete the `Nextcloud` resource.
            finalizer::delete(client, &name, &namespace).await?;
            info!("Nextcloud resource: {} deleted", name);
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
async fn determine_action(nextcloud: &Nextcloud, client: Client) ->
    Result<NextcloudAction, String> {

    let updating = is_update(&nextcloud, client).await?;
    if updating {
        info!("--- ES UPDATE {:?}", updating);
    } else {
        info!("--- NO ES UPDATE {:?}", updating);
    }

    //info!("NEXTCLOUD? {:?}", &nextcloud);
    let nc = nextcloud.meta();
    let name = match &nc.name {
        Some(n) => n,
        None    => "N/A",
    };
    return if nc.deletion_timestamp.is_some() {
        info!("Deleting: {}", name);
        Ok(NextcloudAction::Delete)
    } else if nc.finalizers
        .as_ref()
        .map_or(true, |finalizers| finalizers.is_empty())
    {
        info!("Creating: {}", name);
        Ok(NextcloudAction::Create)

    } else if updating {
        Ok(NextcloudAction::Update)
    } else {
        info!("Nothing to do for: {}", name);
        Ok(NextcloudAction::NoOp)
    };
}

async fn is_update(nc: &Nextcloud, client: Client) ->
    Result<bool, String> {

    let meta = nc.meta();
    /*let current_state_hash = meta.annotations.unwrap().get("state_hash");
    let new_state_hash = nextcloud::create_hash(meta.name, nc.spec.replicas,
        nc.spec.php_image.clone());*/
    let annotations = match get_annotations(client.clone(),
        nc.metadata.namespace.clone().unwrap()).await {
        Ok(a) => a,
        _     => {
            // No annotations means that the object is being created
            info!("Is creating");
            return Ok(true);
        },


    };

    info!("----- PHP IMAGE: {:?}", nc.spec.php_image.clone());


    // Check hash for both nginx and php separately
    let new_state_hash = nextcloud::create_hash(&meta.name.as_ref().unwrap(), nc.spec.replicas,
        nc.spec.php_image.clone());

    //let current_state_hash = annotations.get("state_hash").unwrap_or("N/A".to_string()); //TODO: use other default
    let current_state_hash = annotations.get("state_hash").unwrap();
    info!("--- NEW STATE HASH: {:?}", new_state_hash);
    info!("--- CURRENT STATE HASH: {:?}", current_state_hash);

    // If the state hashes are different we're updating
    if current_state_hash != &new_state_hash {
        return Ok(true);
    }
    Ok(false)
}


/// Actions to be taken when a reconciliation fails - for whatever reason.
/// Prints out the error to `stderr` and requeues the resource for another reconciliation after
/// five seconds.
///
/// # Arguments
/// - `nextcloud`: The erroneous resource.
/// - `error`: A reference to the `kube::Error` that occurred during reconciliation.
/// - `_context`: Unused argument. Context Data "injected" automatically by kube-rs.
fn on_error(nextcloud: Arc<Nextcloud>, error: &NextcloudError, _context: Arc<ContextData>) -> Action {
    //eprintln!("Nextcloud Reconciliation error:\n{:?}\n", error);
    info!("Error: {:?}", error);
    Action::requeue(Duration::from_secs(5))
}

/// Check pod and container status
async fn check_component_status(client: Client, namespace: String) -> Result <(), String> {
    let pods: Api<Pod> = Api::namespaced(client.clone(), &namespace);
    let list_params = ListParams::default();
    //ListParams::default().labels("nextcloud=" + name); // for this app only
    //for p in pods.list(&list_params).await? {
    for p in pods.list(&list_params).await.unwrap() {
        let pod_name = p.name_any();
        info!("Checking pod: {} status", pod_name);
        let pod_status = p.status.clone().unwrap();
        match pod_status.pod_ip.clone() {
            Some(i) => {
                info!("🙌 Pod IP: {}", pod_status.pod_ip.clone().unwrap());
            },
            None => info!("😢 No IP available for: {}", pod_name),
        };

        pod_status.conditions.clone().unwrap().iter().for_each( move |s| {
                if s.status == "False" {
                    error!("😢 Condition: {}, Status: {}", s.type_, s.status);
                    error!("😢 Reason: {}, Message: {}",
                        s.reason.clone().unwrap(), s.message.clone().unwrap()
                    );
                } else {
                    info!("😍 Condition: {}, Status: {}", s.type_, s.status);
                }
        });

        pod_status.container_statuses.clone().unwrap().iter().for_each( move |c| {
            if !c.ready {
                if let Some(state) = c.state.clone().unwrap().waiting {
                    let reason = format!("{}, {}",
                        state.message.unwrap(),
                        state.reason.unwrap()
                    );
                    error!("😢 Container: {} not ready, reason: {}", c.name, reason);
                }
            } else {
                info!("🚀 Container: {} ready to go!", c.name);
            }

        });
        //info!("Container status: {:?}", pod_status.container_statuses.clone());
    }
    Ok(())
}

async fn get_annotations(client: Client, namespace: String) ->
    Result <BTreeMap<std::string::String, std::string::String>, String> {
    let list_params = ListParams::default();
    let deployments: Api<Deployment> = Api::namespaced(client.clone(), &namespace);
    let mut deployment = deployments.list(&list_params).await.unwrap();
    //info!("ITEMS: {}", &deployment.items);
    // We only have one deployment
    let my_deployment = deployment.items.pop();
    if my_deployment.is_none() {
        return Err("No annotations".to_string());
    }
    match &my_deployment.unwrap().metadata.annotations {
        Some(a) =>  {
            Ok(a.clone())
        },
        None => Err("There are no annotations!!!".to_string())
    }
}

/// All errors possible to occur during reconciliation
#[derive(Debug, thiserror::Error)]
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
