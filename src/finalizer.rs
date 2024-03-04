use crate::crd::Nextcloud;
use kube::api::{Patch, PatchParams};
use kube::{Api, Client, Error};
use serde_json::{json, Value};
use kube::CustomResourceExt;

/// Adds a finalizer record into an `Echo` kind of resource. If the finalizer already exists,
/// this action has no effect.
///
/// # Arguments:
/// - `client` - Kubernetes client to modify the `Echo` resource with.
/// - `name` - Name of the `Echo` resource to modify. Existence is not verified
/// - `namespace` - Namespace where the `Echo` resource with given `name` resides.
///
/// Note: Does not check for resource's existence for simplicity.
pub async fn add(client: Client, name: &str, namespace: &str) -> Result<Nextcloud, Error> {
    let api: Api<Nextcloud> = Api::namespaced(client, namespace);
    let finalizer: Value = json!({
        "metadata": {
            "finalizers": ["nextcloud/finalizer"]
        }
    });

    let patch: Patch<&Value> = Patch::Merge(&finalizer);
    api.patch(name, &PatchParams::default(), &patch).await
}

/// Removes all finalizers from an `Nextcloud` resource. If there are no finalizers already, this
/// action has no effect.
///
/// # Arguments:
/// - `client` - Kubernetes client to modify the `Nextcloud` resource with.
/// - `name` - Name of the `Nextcloud` resource to modify. Existence is not verified
/// - `namespace` - Namespace where the `Nextcloud` resource with given `name` resides.
///
/// Note: Does not check for resource's existence for simplicity.
pub async fn delete(client: Client, name: &str, namespace: &str) -> Result<Nextcloud, Error> {
    let api: Api<Nextcloud> = Api::namespaced(client, namespace);
    let finalizer: Value = json!({
        "metadata": {
            "finalizers": null
        }
    });

    let patch: Patch<&Value> = Patch::Merge(&finalizer);
    //api.patch(name, &PatchParams::default(), &patch).await
    let mut params = PatchParams::default();
    params.field_manager = Some("Nextcloud".to_string());

    info!("CRD {:?}", Nextcloud::crd());
    let mut nc = Nextcloud::crd();
    nc.metadata.finalizers = None;
    info!("PATCH PARAMS {:?}", &params.clone().force());

    api.patch(name, &params.force(), &Patch::Apply(&nc)).await
}
