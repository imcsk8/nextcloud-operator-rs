use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container,
    ContainerPort,
    Pod,
    PodSpec,
    PodTemplateSpec,
    LocalObjectReference,
    Service,
    ServiceSpec,
    ServicePort,
    PersistentVolumeClaim,
    PersistentVolumeClaimSpec,
    PersistentVolumeClaimVolumeSource,
    ResourceRequirements,
    Volume,
    VolumeMount,
    //Does not exist VolumeResourceRequirements,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::core::subresource::AttachParams;
use kube::api::{
    DeleteParams,
    ObjectMeta,
    PostParams,
    Patch,
    PatchParams,
    ListParams,
};
use kube::{Api, Client, Error, Resource, ResourceExt};
use std::collections::BTreeMap;
use crate::Nextcloud;
use std::sync::Arc;
use log::{info, debug};
use sha2::{Digest, Sha256};
use thiserror::Error;
use indoc::formatdoc;
use tokio::io::{self, BufReader, AsyncReadExt, AsyncBufReadExt};
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use crate::crd::NextcloudStatus;


/// Represents NextCloud deployments
#[derive(Debug)]
struct NextcloudElement {
    name: String,
    prefix: String,
    image: String,
    namespace: String,
    replicas: i32,
    container_port: i32,
    node_port: Option<i32>,
    labels: BTreeMap<String, String>,
    selector: BTreeMap<String, String>,
    image_pull_secrets: Vec<LocalObjectReference>,
    annotations: BTreeMap<String, String>,
}

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

/// Nextcloud application management<
/*pub trait NextcloudApp {
    async fn is_installed(&self, client: Client, name: &str)
        -> Result<bool, NextcloudError>;
    //fn get_pod(&self, name: String) -> Result<Pod, NextcloudError>;
}


impl NextcloudApp for Nextcloud {
    async fn is_installed(&self, client: Client, pod_name: &str) -> Result<bool, NextcloudError> {
        //let pod = self.get_pod("nginx")?
        let occ = "/usr/share/nginx/html/occ"; // Hardcoded because we're a particular setup
        //TODO: revisar una manera mas precisa de revisar si nextcloud está instalado
        let installed_command = formatdoc! {"
    service: BTreeMap<String, String>,
              timeout 10 sudo -u nginx /usr/bin/php {} status | \
              grep installed | cut -d':' -f 2 | sed 's/     //'
        ", occ};
        let params = AttachParams::default();
        let pods: Api<Pod> = Api::namespaced(
            client,
            &self.meta().namespace.unwrap().as_str()
        );
        params.stdout(true);
        let ret = match pods.exec(pod_name, installed_command.chars(), &params).await {
            Ok(r) => r.stdout().unwrap(),
            Err(e) =>  { return Err(NextcloudError::DeployError(e.to_string())); }

        };
        let mut buf = String::new();
        ret.read_line(&mut buf).await.unwrap();
        info!("----- RET {:?}", buf);
        Ok(true)
    }

    /*fn get_pod(&self, name: String) -> Result<Pod, NextcloudError> {
        let pods: Api<Pod> = Api::namespaced(client, &namespace);
        match pods.get(name) {
            Ok(p)  => p,
            Err(e) => {
                return NextcloudError(e);
            }
        }
    }*/

}

*/

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

        let mut selector = self.labels.clone();
        selector.append(&mut self.selector.clone());
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
                                volume_mounts: Some(vec![
                                    VolumeMount {
                                        mount_path: "/usr/share/nginx/html".to_string(),
                                        name: format!("pv-{}-{}", self.prefix, self.name),
                                        read_only: Some(false),
                                        ..VolumeMount::default()
                                    }

                                ]),
                            ..Container::default()
                            },
                        ],

                        volumes: Some(vec![Volume {
                            name: format!("pv-{}-{}", self.prefix, self.name),
                            persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                                //claim_name: format!("pvc-{}-{}", self.prefix, self.name),
                                claim_name: "pvc-nextcloud-nginx".to_string(),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }]),
                        image_pull_secrets: Some(self.image_pull_secrets.clone()),
                        ..PodSpec::default()
                    }),
                    metadata: Some(ObjectMeta {
                        labels: Some(selector),
                        ..ObjectMeta::default()
                    }),
                },
                ..DeploymentSpec::default()
            }),
            ..Deployment::default()
        })
    }

    /// returns a new service for the NextcloudElement object
    pub fn create_service(&self)
        -> Result<Service, Error> {
        Ok(Service {
            metadata: ObjectMeta {
                name: Some(format!("service-{}-{}", self.prefix, self.name)),
                namespace: Some(self.namespace.to_owned()),
                labels: Some(self.labels.clone()),
                annotations: Some(self.annotations.clone()),
                ..ObjectMeta::default()
            },
            spec: Some(ServiceSpec {
                type_: Some("LoadBalancer".to_string()),
                ports: Some(vec![
                    ServicePort {
                        node_port: self.node_port,
                        port: self.container_port,
                        target_port: Some(IntOrString::Int(self.container_port)),
                        //TODO: check if we need other protocols
                        protocol: Some("TCP".to_string()),
                        ..Default::default()
                    },
                // The selector must contain the object labels
                ]),
                selector: Some(self.selector.clone()),
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    /// Creates a PersistentVolumeClaim
    /// https://docs.rs/k8s-openapi/0.21.0/k8s_openapi/api/core/v1/struct.PersistentVolumeClaimSpec.html
    /// https://kubernetes.io/docs/concepts/storage/persistent-volumes/#class-1
    fn create_pvc(&self)
        -> Result<PersistentVolumeClaim, Error> {
        Ok(PersistentVolumeClaim {
                metadata: ObjectMeta {
                    name: Some(format!("pvc-{}-{}", self.prefix, self.name)),
                    namespace: Some(self.namespace.to_owned()),
                    labels: Some(self.labels.clone()),
                    annotations: Some(self.annotations.clone()),
                    ..ObjectMeta::default()
                },
                spec: Some(PersistentVolumeClaimSpec {
                    access_modes: Some(vec!["ReadWriteOnce".to_string()]),
                    //TODO: make this a parameter storage_class_name: Some(""),
                    resources: Some(ResourceRequirements {
                        requests: Some(
                                  BTreeMap::from([
                                      ("storage".to_string(), Quantity("5Gi".to_string())),
                                  ])
                                ),
                      ..Default::default()
                    }),
                    selector: Some(LabelSelector {
                        match_expressions: None,
                        match_labels: Some(self.labels.clone()),
                    }),
                    ..PersistentVolumeClaimSpec::default()
                }),
                ..PersistentVolumeClaim::default()
            }
        )
    }
}

pub async fn apply(
    client: Client,
    name: &str,
    nextcloud_object: Arc<Nextcloud>,
    namespace: &str,
) -> Result<(), NextcloudError> {

    let mut global_state_hash = "HASH".to_string();
    info!("Applying Nextcloud elements: {}", name);
    let deployments = vec![
      "php-fpm",
      "nginx",
    ];

    let mut annotations: BTreeMap<String, String> = BTreeMap::new();
    annotations.insert("installed".to_owned(), "false".to_owned());

    let labels = BTreeMap::from([
        ("app".to_owned(), name.to_owned()),
    ]);

    info!("---- PULL SECRET? {:?}", nextcloud_object.spec.image_pull_secret.clone());
    let image_pull_secrets = vec![
        LocalObjectReference {
            name: Some(nextcloud_object.spec.image_pull_secret.clone()),
        },
    ];

    let nextcloud_elements = vec![
        NextcloudElement {
            name: "php-fpm".to_string(),
            prefix: "nextcloud".to_string(),
            image: nextcloud_object.spec.php_image.clone(),
            namespace: namespace.to_string(),
            replicas: nextcloud_object.spec.replicas, //TODO: should we use different replica size for each deployment?
            container_port: 9000,
            node_port: Some(30000), // TODO: revisar
            labels: labels.clone(),
            selector: BTreeMap::from([
                ("endpoint".to_string(), "php-fpm".to_string())
            ]),
            image_pull_secrets: image_pull_secrets.clone(),
            annotations: annotations.clone(),
        },
        NextcloudElement {
            name: "nginx".to_string(),
            prefix: "nextcloud".to_string(),
            image: nextcloud_object.spec.nginx_image.clone(),
            namespace: namespace.to_string(),
            replicas: nextcloud_object.spec.replicas,
            container_port: 80,
            node_port: Some(30001),
            labels: labels.clone(),
            selector: BTreeMap::from([
                ("endpoint".to_string(), "nginx".to_string())
            ]),
            image_pull_secrets: image_pull_secrets.clone(),
            annotations: annotations.clone(),
        },
    ];

    let patch_params = PatchParams {
        field_manager: Some("nextcloud_field_manager".to_string()),
        ..PatchParams::default()
    };

    let list_params = ListParams::default();
    // Use the nginx element
    let pvc = nextcloud_elements[1].create_pvc()?;
    let pvc_api: Api<PersistentVolumeClaim> = Api::namespaced(client.clone(), namespace);
    //let mut pvcs = match pcv_api.list(&list_params).await {
    let pvc_name = match pvc.meta().name.clone() { //TODO: remove unwrap
        Some(n) => n,
        None    => {
            info!("Error getting PVC resource name");
            return Err(
                NextcloudError::DeployError(
                    "Error getting PVC resource name".to_string()
                )
            );
        }
    };

    info!("-------- PVCS {:?}", pvc_api.list(&list_params).await.unwrap().items.len());
    let pvc_list = match pvc_api.list(&list_params).await {
        Ok(l) => l,
        Err(e) => {
            return Err(NextcloudError::KubeError { source:e });
        }
    };
    let pvc_len = pvc_list.items.len();
    // No PVC yet
    if pvc_len <= 0 {
        info!("Creating PVC: {}", pvc_name);
        let _result = pvc_api
            .patch(
                pvc_name.as_str(),
                &patch_params,
                &Patch::Apply(&pvc)
            )
            .await?;
    } else {
        info!("PV {} already exists", pvc_name);
    }

    // Create the deployment defined above
    let deployment_api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let service_api: Api<Service> = Api::namespaced(client.clone(), namespace.clone());
    for dep in nextcloud_elements {
        //info!("CREATING ELEMENT: {:?}", dep);
        let state_hash = create_hash(
            name,
            dep.replicas,
            dep.image.clone()
        );

        // state hash
        annotations.insert("state_hash".to_owned(), state_hash.clone());
        global_state_hash = format!("{}:{}", global_state_hash, state_hash.clone())
            .to_owned();

        let deployment = dep.as_deployment()?;
        //info!("Deployment: {:?}", &deployment);

        let _ret = deployment_api
            .patch(&dep.name, &patch_params, &Patch::Apply(&deployment))
            .await;
        //info!("RESULT Deployment: {:?}", _ret);
        info!("Done applying Deployment: {}", dep.name);
        let service = dep.create_service()?;
        let service_name = service.clone().metadata.name.unwrap_or("ERROR".to_string());
        let _result = service_api
            .patch(
                service_name.as_str(),
                &patch_params,
                &Patch::Apply(&service)
            )
            .await?;

        /*let ret = nextcloud_object.is_installed(client.clone(), "nginx").await;
        info!("iS INSTALLED: {:?}", ret);*/
        info!("Done applying Service: {}", service_name);
    }


    let status = NextcloudStatus {
        installed: false,
        configured: 0,
        maintenance: false,
        last_backup: "N/A".to_string(),
        state_hash: global_state_hash,
    };
//checar estatus y agregar el nuevo si ha cambiado
    info!("---- STATUS: {:?}", status);

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
pub async fn delete_elements(client: Client, namespace: &str) ->
    Result<(), Error> {
    info!("--------- Deleting Nextcloud deployments for: in namespace {}", &namespace);

    //TODO: Check how to avoid having two for loops
    let api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let delete_params = DeleteParams::default();
    //let mut error = ("".to_string(), false);
    let elements = vec!["nginx", "php-fpm"];
    for elem in elements.iter() {
        match api.delete(elem, &delete_params).await {
            Ok(r)  => info!("{} delete Ok response: {:?}", &elem, r),
            Err(e) => {
                info!("{} delete Err response: {:?}", &elem, e);
                /*info!("{} delete Err response: {:?}", &elem, e);
                error = (elem.to_string(), true);*/
            }
        };
    }


    info!("--------- Deleting Nextcloud services for namespace {}", &namespace);
    let api: Api<Service> = Api::namespaced(client.clone(), namespace);
    let elements = vec!["service-nextcloud-nginx", "service-nextcloud-php-fpm"];
    for elem in elements.iter() {
        match api.delete(elem, &delete_params).await {
            Ok(r)  => info!("{} delete Ok response: {:?}", &elem, r),
            Err(e) => {
                info!("{} delete Err response: {:?}", &elem, e);
                /*info!("{} delete Err response: {:?}", &elem, e);
                error = (elem.to_string(), true);*/
            }
        };
    }

    Ok(())
}



/* TODO: check trait bounds problem
/// Delete a list of resources
pub async fn delete_list<T>(api: Api<T>, elements: Vec<&str>) -> Result<(), Error> {
    let delete_params = DeleteParams::default();
    let mut error = ("".to_string(), false);
    for elem in elements.iter() {
        match api.delete(elem, &delete_params).await {
            Ok(r)  => info!("{} delete Ok response: {:?}", &elem, r),
            Err(e) => {
                info!("{} delete Err response: {:?}", &elem, e);
                error = (elem.to_string(), true);
            }
        };
    }

    if ! error.1 {
        Ok(())
    } else {
        Err("Could not delete {}, check the logs", error.0)
    }
}
*/


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
