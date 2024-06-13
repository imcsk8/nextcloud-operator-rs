use k8s_openapi::api::apps::v1::{Deployment};
use k8s_openapi::api::core::v1::{
    LocalObjectReference,
    PersistentVolumeClaim,
    PersistentVolumeClaimSpec,
    ResourceRequirements,
    Service,
};
use kube::api::{
    ObjectMeta,
    DeleteParams,
    Patch,
    PatchParams,
    ListParams,
};

use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::api::networking::v1::Ingress;
use kube::{Api, Client, Error, Resource};
use std::collections::BTreeMap;
use crate::Nextcloud;
use std::sync::{Arc};
use log::{info};
use sha2::{Digest, Sha256};
use futures::StreamExt;
// Local modules
use crate::crd::{NextcloudStatus};
use crate::element::*;
use crate::error::{NextcloudError};
use crate::ingress::*;

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

/// Apply the changes
pub async fn apply(
    client: Client,
    name: &str,
    nextcloud_object: Arc<Nextcloud>,
    namespace: &str,
) -> Result<NextcloudStatus, NextcloudError> {
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

    let image_pull_secrets = vec![
        LocalObjectReference {
            name: Some(nextcloud_object.spec.image_pull_secret.clone()),
        },
    ];

    let mut php = NextcloudElement {
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
        volumes: Vec::new(),
        volume_mounts: Vec::new(),
    };

    let mut ingress = NextcloudElement {
        name: "ingress".to_string(),
        prefix: "nextcloud".to_string(),
        image: nextcloud_object.spec.nginx_image.clone(),
        namespace: namespace.to_string(),
        replicas: nextcloud_object.spec.replicas,
        container_port: 80,
        node_port: Some(30001),
        labels: labels.clone(),
        selector: BTreeMap::from([
            ("endpoint".to_string(), "php-fpm".to_string())
        ]),
        image_pull_secrets: image_pull_secrets.clone(),
        annotations: annotations.clone(),
        volumes: Vec::new(),
        volume_mounts: Vec::new(),
    };

    let patch_params = PatchParams {
        field_manager: Some("nextcloud_field_manager".to_string()),
        ..PatchParams::default()
    };

    let list_params = ListParams::default();
    // Use the nginx element
    let pvc = php.create_pvc()?;

    let app_pvc = create_app_pvc(namespace, "1Gi",
        labels.clone(),
        annotations.clone())?;

    let pvc_api: Api<PersistentVolumeClaim> = Api::namespaced(client.clone(), namespace);
    //let mut pvcs = match pcv_api.list(&list_params).await {
    let php_pvc_name = match pvc.meta().name.clone() { //TODO: remove unwrap
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

    let app_pvc_name = match app_pvc.meta().name.clone() { //TODO: remove unwrap
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



    let pvc_list = match pvc_api.list(&list_params).await {
        Ok(l) => l,
        Err(e) => {
            return Err(NextcloudError::KubeError { source:e });
        }
    };

    let pvc_len = pvc_list.items.len();
    info!("NUM PVs {}", pvc_len);
    // No PVC yet
    //TODO: check each resouce
    if pvc_len < 2 {
        info!("Creating PVC: {}", php_pvc_name);
        let _result = pvc_api
            .patch(
                php_pvc_name.as_str(),
                &patch_params,
                &Patch::Apply(&pvc)
            )
            .await?;

        info!("Creating PVC: {}", app_pvc_name);
        let _result = pvc_api
            .patch(
                app_pvc_name.as_str(),
                &patch_params,
                &Patch::Apply(&app_pvc)
            )
            .await?;

    } else {
        info!("PV {} {} already exists", php_pvc_name, app_pvc_name);
    }

    // Create the deployment defined above
    let deployment_api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let service_api: Api<Service> = Api::namespaced(client.clone(), namespace.clone());

    let state_hash = create_hash(
        name,
        php.replicas,
        php.image.clone()
    );

    // state hash
    annotations.insert("state_hash".to_owned(), state_hash.clone());
    global_state_hash = format!("{}:{}", global_state_hash, state_hash.clone())
        .to_owned();

    // Volumes
    php.add_pvc_volume(php_pvc_name.clone(), "php-pvc".to_string());
    php.add_pvc_volume(app_pvc_name.clone(), "app-pvc".to_string());
    //ingress.add_pvc_volume(pvc_volume_name.clone(), ingress.name.clone());
    php.add_config_volume("php-www".to_string(), "php-www".to_string());
    php.add_volume_mount("php-www".to_string(),
        "/etc/php-fpm.d/".to_string(), true);
    php.add_volume_mount("app-pvc".to_string(),
        DOCUMENT_ROOT.to_string(), false);
    /*ingress.add_volume_mount(app_pvc_name.clone(),
        DOCUMENT_ROOT.to_string(), false);*/

    let deployment = php.as_deployment()?;

    //info!("PHP deployment: {:?}", &deployment);

    //let _ret = deployment_api
    deployment_api
        .patch(&php.name, &patch_params, &Patch::Apply(&deployment))
        .await?;
    info!("Done applying Deployment: {}", php.name);
    let service = php.create_service()?;
    let service_name = service.clone().metadata.name.unwrap_or("ERROR".to_string());
    let _result = service_api
        .patch(
            service_name.as_str(),
            &patch_params,
            &Patch::Apply(&service)
        )
        .await?;

    /*let deployment = ingress.as_deployment()?;
    deployment_api
        .patch(&ingress.name, &patch_params, &Patch::Apply(&deployment))
        .await?;
    info!("Done nginx applying Deployment: {}", ingress.name);
    let service = ingress.create_service()?;
    let service_name = service.clone().metadata.name.unwrap_or("ERROR".to_string());
    let _result = service_api
        .patch(
            service_name.as_str(),
            &patch_params,
            &Patch::Apply(&service)
        )
        .await?;
    */
    /*let ret = nextcloud_object.is_installed(client.clone(), "nginx").await;
    info!("iS INSTALLED: {:?}", ret);*/
    info!("Done applying Service: {}", service_name);

    let output = match php.exec(
        client.clone(),
        vec!["stat", "-c", "%U", "/usr/share/nginx/html/config"])
        .await {
        Ok(o)  => {
            info!("--- EXEC output: {}", o);
            if o != "nginx" {
                php.apply_permissions(client.clone()).await?;
            }
        }
        Err(e) => {info!("--- ERROR EXEC!: {:?}", e);}
    };


    // Create ingress
    ingress.create_ingress(client.clone()).await?;
    info!("Done applying Ingress: {}", ingress.name);

    //checar estatus y agregar el nuevo si ha cambiado

    // TODO check if we need a success object
    Ok(NextcloudStatus {
        installed: false,
        configured: 0,
        maintenance: false,
        last_backup: "N/A".to_string(),
        state_hash: global_state_hash,
    })
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
    let elements = vec!["ingress", "php-fpm"];
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

    // Delete ingress 
    let api: Api<Ingress> = Api::namespaced(client.clone(), namespace);
    match api.delete("ingress", &delete_params).await {
        Ok(r)  => info!("ingress delete Ok response: {:?}", r),
        Err(e) => {
            info!("ingress delete Err response: {:?}", e);
        }
    };

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



/// Creates a PersistentVolumeClaim
/// https://docs.rs/k8s-openapi/0.21.0/k8s_openapi/api/core/v1/struct.PersistentVolumeClaimSpec.html
/// https://kubernetes.io/docs/concepts/storage/persistent-volumes/#class-1
pub fn create_app_pvc(namespace: &str,
    size: &str,
    labels: BTreeMap<String, String>,
    annotations: BTreeMap<String, String>)
    -> Result<PersistentVolumeClaim, Error> {
    Ok(PersistentVolumeClaim {
            metadata: ObjectMeta {
                name: Some("pvc-nextcloud-app".to_string()),
                namespace: Some(namespace.to_owned()),
                labels: Some(labels.clone()),
                annotations: Some(annotations.clone()),
                ..ObjectMeta::default()
            },
            spec: Some(PersistentVolumeClaimSpec {
                access_modes: Some(vec!["ReadWriteOnce".to_string()]),
                //TODO: make this a parameter storage_class_name: Some(""),
                resources: Some(ResourceRequirements {
                    requests: Some(
                              BTreeMap::from([
                                  ("storage".to_string(), Quantity(size.to_string())),
                              ])
                            ),
                  ..Default::default()
                }),
                selector: Some(LabelSelector {
                    match_expressions: None,
                    match_labels: Some(labels.clone()),
                }),
                ..PersistentVolumeClaimSpec::default()
            }),
            ..PersistentVolumeClaim::default()
        }
    )
}


