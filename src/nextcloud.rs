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

struct NextcloudElement {
    name: String,
    prefix: String,
    image: String,
    namespace: String,
    replicas: i32,
    container_port: i32,
    labels: BTreeMap<String, String>,
    image_pull_secrets: Vec<LocalObjectReference>,
    annotations: BTreeMap<String, String>,
}


/// NextcloudElement implementation
impl NextcloudElement {

    /// returns a new deployment of `n` pods with the `imcsk8/nextcloud:latest` docker image inside,
    /// where `n` is the number of `replicas` given.
    ///
    /// # Arguments
    /// - `client` - A Kubernetes client to create the deployment with.
    /// - `name` - Name of the deployment to be created
    /// - `replicas` - Number of pod replicas for the Deployment to contain
    /// - `namespace` - Namespace to create the Kubernetes Deployment in.
    ///
    pub fn as_deployment(&self) -> Result<Deployment, Error> {

        Ok(Deployment {
            metadata: ObjectMeta {
                name: Some(self.name.to_owned()),
                namespace: Some(self.namespace.to_owned()),
                labels: Some(self.labels.clone()),
                annotations: Some(self.annotations.clone()),
                ..ObjectMeta::default()
            },
            spec: Some(DeploymentSpec {
                replicas: Some(self.replicas),
                selector: LabelSelector {
                    match_expressions: None,
                    match_labels: Some(self.labels.clone()),
                },
                template: PodTemplateSpec {
                    spec: Some(PodSpec {
                        containers: vec![
                            Container {
                                name: format!("{}-{}", self.prefix, self.name).to_string(),
                                image: Some(self.image.clone()),
                                ports: Some(vec![ContainerPort {
                                    container_port: self.container_port,
                                    ..ContainerPort::default()
                                }]),
                            ..Container::default()
                            },
                        ],
                        image_pull_secrets: Some(self.image_pull_secrets.clone()),
                        ..PodSpec::default()
                    }),
                    metadata: Some(ObjectMeta {
                        labels: Some(self.labels.clone()),
                        ..ObjectMeta::default()
                    }),
                },
                ..DeploymentSpec::default()
            }),
            ..Deployment::default()
        })
    }
}

pub async fn apply(
    client: Client,
    name: &str,
    nextcloud_object: Arc<Nextcloud>,
    namespace: &str,
) -> Result<(), Error> {

    info!("Applying Nextcloud elements: {}", name);
    let deployments = vec![
      "php-fpm",
      "nginx",
    ];

    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    let mut annotations: BTreeMap<String, String> = BTreeMap::new();
    labels.insert("app".to_owned(), name.to_owned());

    info!("---- PULL SECRET? {:?}", nextcloud_object.spec.image_pull_secret.clone());
    let image_pull_secrets = vec![
        LocalObjectReference {
            name: Some(nextcloud_object.spec.image_pull_secret.clone()),
        },
    ];

    let nextcloud_elements = vec![
        NextcloudElement {
            name: "php-fpm".to_string(),
            prefix: "php-fpm".to_string(),
            image: nextcloud_object.spec.php_image.clone(),
            namespace: namespace.to_string(),
            replicas: nextcloud_object.spec.replicas, //TODO: should we use different replica size for each deployment?
            container_port: 9000,
            labels: labels.clone(),
            image_pull_secrets: image_pull_secrets.clone(),
            annotations: annotations.clone(),
        },
        /*NextcloudElement {
            name: "nginx".to_string(),
            prefix: "nginx".to_string(),
            image: nextcloud_object.spec.nginx_image.clone(),
            namespace: namespace.to_string(),
            replicas: nextcloud_object.spec.replicas,
            container_port: 80,
            labels: labels.clone(),
            image_pull_secrets: image_pull_secrets.clone(),
            annotations: annotations.clone(),
        },*/
    ];

    let patch_params = PatchParams {
        field_manager: Some("nextcloud_field_manager".to_string()),
        ..PatchParams::default()
    };

    for dep in nextcloud_elements {
        let state_hash = create_hash(
            name,
            dep.replicas,
            dep.image.clone()
        );
        annotations.insert("state_hash".to_owned(), state_hash);
        let deployment = dep.as_deployment()?;

        // Create the deployment defined above
        let deployment_api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
        let ret = deployment_api
            .patch(name, &patch_params, &Patch::Apply(&deployment))
            .await;
        info!("Done Deploying: {}", dep.name);
    }

    Ok(())
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
pub fn create_hash(name: &str,
    replicas: i32,
    image: String,
) -> String {
    let state_string = format!("{}-{}-{}",
        name,
        replicas.to_string(),
        image
    );
    let mut hasher = Sha256::new();
    hasher.update(state_string.as_bytes());
    hasher.finalize()
      .iter()
      .map(|byte| format!("{:02x}", byte))
      .collect::<String>()
}
