use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{Container, ContainerPort, PodSpec, PodTemplateSpec, LocalObjectReference};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use kube::api::{DeleteParams, ObjectMeta, PostParams, Patch, PatchParams};
use kube::{Api, Client, Error, Resource};
use std::collections::BTreeMap;
use crate::Nextcloud;
use std::sync::Arc;
use log::{info, debug};
use sha2::{Digest, Sha256};

/// Creates a new deployment of `n` pods with the `imcsk8/nextcloud:latest` docker image inside,
/// where `n` is the number of `replicas` given.
///
/// # Arguments
/// - `client` - A Kubernetes client to create the deployment with.
/// - `name` - Name of the deployment to be created
/// - `replicas` - Number of pod replicas for the Deployment to contain
/// - `namespace` - Namespace to create the Kubernetes Deployment in.
///
/// Note: It is assumed the resource does not already exists for simplicity. Returns an `Error` if it does.
pub async fn deploy(
    client: Client,
    name: &str,
    nextcloud_object: Arc<Nextcloud>,
    namespace: &str,
) -> Result<Deployment, Error> {

    info!("Deploying: {}", name);

    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    let mut annotations: BTreeMap<String, String> = BTreeMap::new();
    labels.insert("app".to_owned(), name.to_owned());
    //TODO: use a real hashing function
    let state_hash = create_hash(name, nextcloud_object.spec.replicas,
        nextcloud_object.spec.php_image.clone(),
        nextcloud_object.spec.nginx_image.clone());
    annotations.insert("state_hash".to_owned(), state_hash.to_owned());
    info!("---- PULL SECRET? {:?}", nextcloud_object.spec.image_pull_secret.clone());
    let image_pull_secrets = vec![
        LocalObjectReference {
            name: Some(nextcloud_object.spec.image_pull_secret.clone()),
        },
    ];
    /*TODO: check if we can get it as mutable nextcloud_object.metadata
        .annotations.as_mut().unwrap()
        .insert("state_hash".to_owned(), state_hash.to_owned());
    */

    // Definition of the deployment. Alternatively, a YAML representation could be used as well.
    let deployment: Deployment = Deployment {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            namespace: Some(namespace.to_owned()),
            labels: Some(labels.clone()),
            annotations: Some(annotations),
            ..ObjectMeta::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(nextcloud_object.spec.replicas),
            selector: LabelSelector {
                match_expressions: None,
                match_labels: Some(labels.clone()),
            },
            template: PodTemplateSpec {
                spec: Some(PodSpec {
                    containers: vec![
                        Container {
                            name: format!("php-fpm-{}",name).to_string(),
                            image: Some(nextcloud_object.spec.php_image.clone()),
                            ports: Some(vec![ContainerPort {
                                container_port: 9000,
                                ..ContainerPort::default()
                            }]),
                        ..Container::default()
                        },
                        Container {
                            name: format!("nginx-{}",name).to_string(),
                            image: Some(nextcloud_object.spec.nginx_image.clone()),
                            ports: Some(vec![ContainerPort {
                                container_port: 80,
                                ..ContainerPort::default()
                            }]),
                        ..Container::default()
                        },
                    ],
                    image_pull_secrets: Some(image_pull_secrets),
                    ..PodSpec::default()
                }),
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..ObjectMeta::default()
                }),
            },
            ..DeploymentSpec::default()
        }),
        ..Deployment::default()
    };

    // let ssapply = PatchParams::apply("crd_apply_example").force();
    let patch_params = PatchParams {
        field_manager: Some("nextcloud_field_manager".to_string()),
        ..PatchParams::default()
    };
    // Create the deployment defined above
    let deployment_api: Api<Deployment> = Api::namespaced(client, namespace);
    let ret = deployment_api
        //.create(&PostParams::default(), &deployment)
        .patch(name, &patch_params, &Patch::Apply(&deployment))
        .await;

        //foos.patch("baz", &ssapply, &Patch::Apply(&foo)).await?;
    info!("Done Deploying: {}", name);
    ret
}

/// Deletes an existing deployment.
///
/// # Arguments:
/// - `client` - A Kubernetes client to delete the Deployment with
/// - `name` - Name of the deployment to delete
/// - `namespace` - Namespace the existing deployment resides in
///
/// Note: It is assumed the deployment exists for simplicity. Otherwise returns an Error.
/// https://docs.rs/kube/0.88.1/kube/struct.Api.html#method.delete
pub async fn delete(client: Client, name: &str, namespace: &str) -> Result<(), Error> {
    let api: Api<Deployment> = Api::namespaced(client, namespace);
    match api.delete(name, &DeleteParams::foreground()).await {
        Ok(r) => {
            info!("Resource deleted successfully {:?}", r);
            Ok(())
        },
        Err(e) => {
            info!("Error deleting resource: {:?}", e);
            Err(e)
        }
    }
}


/// Creates a sha256 hash from the given attributes
pub fn create_hash(name: &str, replicas: i32, php_image: String,
    nginx_image: String) -> String {
    let state_string = format!("{}-{}-{}-{}", name, replicas.to_string(),
				php_image, nginx_image);
		let mut hasher = Sha256::new();
		hasher.update(state_string.as_bytes());
		hasher.finalize()
			.iter()
			.map(|byte| format!("{:02x}", byte))
			.collect::<String>()
}
