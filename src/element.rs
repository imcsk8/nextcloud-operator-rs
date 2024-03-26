use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container,
    ContainerPort,
    ConfigMapVolumeSource,
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
    ObjectMeta,
    ListParams,
};
use kube::{Api, Client, Error, ResourceExt};
use std::collections::BTreeMap;
use log::{info};
use sha2::{Digest};
use tokio_util::io::ReaderStream;
use futures::StreamExt;
use std::fmt::Debug;
// Local modules
use crate::crd::{NextcloudStatus, NextcloudResource};
use crate::error::{NextcloudError};

pub static DOCUMENT_ROOT: &str = "/usr/share/nginx/html";

/// Represents NextCloud deployments
#[derive(Debug)]
pub struct NextcloudElement {
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
    volumes: Vec<Volume>,
    volume_mounts: Vec<VolumeMount>,
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
                    metadata: Some(ObjectMeta {
                        name: Some(self.name.to_owned()),
                        namespace: Some(self.namespace.to_owned()),
                        labels: Some(selector.clone()),
                        annotations: Some(self.annotations.clone()),
                        ..ObjectMeta::default()
                    }),
                    spec: Some(PodSpec {
                        containers: vec![
                            Container {
                                name: format!("{}-{}", self.prefix, self.name).to_string(),
                                image: Some(self.image.clone()),
                                ports: Some(vec![ContainerPort {
                                    container_port: self.container_port,
                                    ..ContainerPort::default()
                                }]),
                                volume_mounts: Some(self.volume_mounts.clone()),
                            ..Container::default()
                            },
                        ],
                        volumes: Some(self.volumes.clone()),
                        image_pull_secrets: Some(self.image_pull_secrets.clone()),
                        ..PodSpec::default()
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
                //annotations: Some(self.annotations.clone()),
                //TODO: add labes to selectors
                annotations: Some(self.selector.clone()),
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


    /// Add a volume from a configmap to be mounted on the container
    /// Arguments:
    /// config_map: Name of the config map
    /// name: Name of the volume
    fn add_config_volume(&mut self, config_map: String, name: String, )
        -> Result<(), Error> {
        self.volumes.push(Volume {
            name: name,
            config_map: Some(ConfigMapVolumeSource {
                name: Some(config_map),
                ..Default::default()
            }),
            ..Default::default()
        });
        // TODO: add proper error management
        Ok(())
    }


    /// Add a volume from a persistent volume claim
    /// Arguments
    /// pvc_name: Name of the PersistentVolumeClaim
    /// name: Name of the Volume
    fn add_pvc_volume(&mut self, pvc_name: String, name: String)
        -> Result<(), Error> {
        self.volumes.push(Volume {
            name: name.clone(),
            persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                claim_name: pvc_name.clone(),
                ..Default::default()
            }),
            ..Default::default()
        });
        // TODO: add proper error management
        Ok(())
    }

    /// Add a volume mount for the container
    /// Arguments:
    /// name: name of the Volume to mount
    /// mount_path: path where the Volume is going to mount inside the container
    /// read_only: true if the volume is read only
    fn add_volume_mount(
        &mut self,
        name: String,
        mount_path: String,
        read_only: bool)
        -> Result<(), Error> {
        // Define the volume mount for the ConfigMap
        self.volume_mounts.push(VolumeMount {
            name: name,
            mount_path: mount_path,
            read_only: Some(read_only),
            ..Default::default()
        });
        // TODO: add proper error management
        Ok(())
    }


    /// Executes a command and returns the output as a string
    async fn exec(
        &self,
        client: Client,
        command: Vec<&str>
    ) -> Result<String, NextcloudError> {
        let pods: Api<Pod> = Api::namespaced(client, &self.namespace);
        let lp = ListParams::default().labels("endpoint=php-fpm");
        let pod =  pods.list(&lp).await?.items[0].clone();
        let pod_name = pod.name_any();
        info!("Executing command on pod: {}", &pod_name);
        let mut attached = pods
            .exec(
                &pod_name,
                command,
                &AttachParams::default().stdout(true),
            )
            .await?;
        let stdout = ReaderStream::new(attached.stdout().unwrap());
        let out = stdout
            .filter_map(|r| async { r.ok().and_then(|v| String::from_utf8(v.to_vec()).ok()) })
            .collect::<Vec<_>>()
            .await
            .join("");
        attached.join().await.unwrap();
        Ok(out.to_string())
    }

    /// Update permissions for Nextcloud
    // This is temporary just for tests
    async fn apply_permissions(&self, client: Client)
    -> Result<(), NextcloudError> {
        info!("Enabling apache user to write to config file TODO REMOVE THIS");
        let output = match self.exec(
            client.clone(),
            vec!["chown", "-R", "apache:apache", "/usr/share/nginx/html/config"])
            .await {
                //TODO: Convert this to debug
                Ok(o)  => {info!("--- Exec output: {}", o);}
                Err(e) => {
                    info!("--- ERROR Exec!: {:?}", e);
                }
        };

        info!("Enabling apache user to write to data directory");
        let output = match self.exec(
            client.clone(),
            vec!["chown", "-R", "apache:apache", "/usr/share/nginx/html/data"])
            .await {
            //TODO: Convert this to debug
            Ok(o)  => {info!("--- Exec output: {}", o);}
            Err(e) => {info!("--- ERROR Exec!: {:?}", e);}
        };

        info!("Enabling apache user to write to apps directory");
        let output = match self.exec(
            client.clone(),
            vec!["chown", "-R", "apache:apache", "/usr/share/nginx/html/apps"])
            .await {
            //TODO: Convert this to debug
            Ok(o)  => {info!("--- Exec output: {}", o);}
            Err(e) => {info!("--- ERROR Exec!: {:?}", e);}
        };

        Ok(()) // TODO: add error return
    }
}



